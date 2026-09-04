use std::ffi::c_void;
use std::io;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::thread;

use servo_media::player::metadata::Metadata;
use servo_media::player::video::VideoFrameRenderer;
use servo_media::player::{PlaybackState, PlayerEvent, StreamType};
use symphonia::core::codecs::audio::AudioDecoder as SymphoniaDecoder;
use symphonia::core::codecs::audio::AudioDecoderOptions as SymphoniaOptions;
use symphonia::core::codecs::video::well_known::extra_data::VIDEO_EXTRA_DATA_ID_AVC_DECODER_CONFIG;
use symphonia::core::codecs::video::well_known::CODEC_ID_H264;
use symphonia::core::codecs::CodecParameters;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo, Track, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::{MetadataOptions, StandardTag};
use symphonia::core::units::{Time, TimeBase, Timestamp};

use super::shared::{lock, player_callback, wait, Pcm, Shared};
use super::source::ByteReader;
use crate::media::device::{Device, CHANNELS};
use crate::media::video::VideoPipeline;

/// Decoded PCM buffered ahead of the device; rides out refetch latency.
const PCM_TARGET_SECONDS: f64 = 1.0;

/// Minimum advance between `PositionChanged` events.
const POSITION_EVENT_SECONDS: f64 = 0.25;

pub(super) fn spawn_decoder(
    shared: Arc<Shared>,
    stream_type: StreamType,
    video_renderer: Option<Arc<Mutex<dyn VideoFrameRenderer>>>,
) {
    let result = thread::Builder::new()
        .name("media-player".into())
        .spawn(move || decoder_thread(shared, stream_type, video_renderer));
    if let Err(e) = result {
        log::warn!("audio: could not spawn the player decoder thread: {e}");
    }
}

/// Everything slow: probe, decode, device, seeks, refetch handshakes.
fn decoder_thread(
    shared: Arc<Shared>,
    stream_type: StreamType,
    video_renderer: Option<Arc<Mutex<dyn VideoFrameRenderer>>>,
) {
    // Unlock the element's initial fetch; nothing arrives before this.
    shared.events.send(PlayerEvent::NeedData);

    let seekable = stream_type == StreamType::Seekable;
    let source = ByteReader {
        shared: shared.clone(),
        seekable,
    };

    match run_pipeline(&shared, source, seekable, video_renderer) {
        Ok(()) => {}
        Err(message) => {
            // A quit mid-read surfaces as a decode error; it is not one.
            if !shared.quit.load(Ordering::SeqCst) {
                log::warn!("audio: player failed: {message}");
                shared.events.send(PlayerEvent::Error(message));
            }
        }
    }
    // The device (owned inside run_pipeline) is already closed here, so the SDL
    // callback can no longer observe `shared`.
}

/// Audio-track half of the pipeline.
struct AudioPipe {
    track_id: u32,
    time_base: Option<TimeBase>,
    decoder: Box<dyn SymphoniaDecoder>,
    rate: u32,
    channels: usize,
    target_samples: usize,
    /// Accurate seeks land before the target; trim decoded output up to it.
    discard_until: Option<f64>,
}

/// Video-track half: conversion and handoff to the video thread.
struct VideoPipe {
    track_id: u32,
    time_base: Option<TimeBase>,
    pipeline: VideoPipeline,
    /// The channel filled up; drop packets until the next IDR fits.
    lagging: bool,
}

/// Duration in seconds from the track's own units.
fn track_duration_secs(track: &Track) -> Option<f64> {
    let (tb, duration) = (track.time_base?, track.duration?);
    Some(
        tb.calc_time_saturating(Timestamp::new(duration.get() as i64))
            .as_secs_f64(),
    )
}

fn run_pipeline(
    shared: &Arc<Shared>,
    source: ByteReader,
    seekable: bool,
    video_renderer: Option<Arc<Mutex<dyn VideoFrameRenderer>>>,
) -> Result<(), String> {
    let stream = MediaSourceStream::new(Box::new(source), Default::default());
    let mut reader = symphonia::default::get_probe()
        .probe(
            &Hint::new(),
            stream,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(|e| format!("probe failed: {e}"))?;

    // Audio setup; the decoder is built here, rate/channels are pinned by the
    // preroll below.
    let audio_track = reader
        .first_track_known_codec(TrackType::Audio)
        .and_then(|track| {
            let params = track
                .codec_params
                .as_ref()
                .and_then(CodecParameters::audio)?;
            let decoder = symphonia::default::get_codecs()
                .make_audio_decoder(params, &SymphoniaOptions::default())
                .map_err(|e| log::warn!("audio: decoder init failed: {e}"))
                .ok()?;
            Some((
                track.id,
                track.time_base,
                decoder,
                params.sample_rate,
                track.num_frames,
                track_duration_secs(track),
            ))
        });

    // Video setup: H.264 only, and only when the element renders pictures.
    let video = video_renderer
        .filter(|_| crate::media::settings().video)
        .and_then(|renderer| {
            let track = reader.tracks().iter().find(|track| {
                track
                    .codec_params
                    .as_ref()
                    .and_then(CodecParameters::video)
                    .is_some_and(|params| params.codec == CODEC_ID_H264)
            })?;
            let params = track
                .codec_params
                .as_ref()
                .and_then(CodecParameters::video)?;
            let extra = params
                .extra_data
                .iter()
                .find(|data| data.id == VIDEO_EXTRA_DATA_ID_AVC_DECODER_CONFIG)?;
            let pipeline = VideoPipeline::spawn(shared.clone(), renderer, &extra.data)?;
            Some((
                VideoPipe {
                    track_id: track.id,
                    time_base: track.time_base,
                    pipeline,
                    lagging: false,
                },
                (params.width.unwrap_or(0), params.height.unwrap_or(0)),
                track_duration_secs(track),
            ))
        });
    let (video_dims, video_duration) = match &video {
        Some((_, dims, duration)) => (Some(*dims), *duration),
        None => (None, None),
    };
    let mut video = video.map(|(pipe, ..)| pipe);
    if audio_track.is_none() && video.is_none() {
        return Err("no decodable track".into());
    }

    let title = reader.metadata().skip_to_latest().and_then(|revision| {
        revision.media.tags.iter().find_map(|tag| match &tag.std {
            Some(StandardTag::TrackTitle(title)) => Some(title.to_string()),
            _ => None,
        })
    });

    // Audio preroll: one decoded packet pins the true sample rate before
    // anything is announced. Video packets met on the way are routed onward.
    let mut interleaved = Vec::new();
    let mut audio: Option<AudioPipe> = None;
    let mut audio_duration = None;
    if let Some((track_id, time_base, mut decoder, params_rate, num_frames, duration)) = audio_track
    {
        let (rate, channels, first_secs) = loop {
            let packet = match reader.next_packet() {
                Ok(Some(packet)) if packet.track_id == track_id => packet,
                Ok(Some(packet)) => {
                    route_video_packet(&mut video, &packet, true);
                    continue;
                }
                Ok(None) => return Err("stream ended before any audio decoded".into()),
                Err(e) => return Err(format!("demux failed: {e}")),
            };
            match decoder.decode(&packet) {
                Ok(buffer) if buffer.frames() > 0 => {
                    let rate = buffer.spec().rate();
                    let channels = buffer.num_planes();
                    buffer.copy_to_vec_interleaved::<f32>(&mut interleaved);
                    let secs = packet_secs(&packet.pts, time_base, f64::from(rate));
                    break (rate, channels, secs);
                }
                Ok(_) => continue,
                Err(SymphoniaError::DecodeError(e)) => {
                    log::debug!("audio: skipping malformed packet: {e}");
                    continue;
                }
                Err(e) => return Err(format!("decode failed: {e}")),
            }
        };
        audio_duration = num_frames
            .map(|frames| frames as f64 / f64::from(params_rate.unwrap_or(rate)))
            .or(duration);
        lock(&shared.clock).audio_rate = Some(rate);
        audio = Some(AudioPipe {
            track_id,
            time_base,
            decoder,
            rate,
            channels,
            target_samples: (PCM_TARGET_SECONDS * f64::from(rate)) as usize * CHANNELS as usize,
            discard_until: None,
        });
        let mut pcm = lock(&shared.pcm);
        push_stereo(&mut pcm, &interleaved, channels);
        pcm.decoded_secs =
            first_secs + interleaved.len() as f64 / channels as f64 / f64::from(rate);
    }

    let duration_secs = match (audio_duration, video_duration) {
        (Some(a), Some(v)) => Some(a.max(v)),
        (a, v) => a.or(v),
    };
    {
        let mut meta = lock(&shared.meta);
        meta.duration_secs = duration_secs;
        meta.sample_rate = audio.as_ref().map(|a| a.rate);
    }
    let (response_seekable, is_live) = {
        let stream = lock(&shared.stream);
        (stream.response_seekable, stream.total_len.is_none())
    };
    let (width, height) = video_dims.unwrap_or((0, 0));
    shared.events.send(PlayerEvent::MetadataUpdated(Metadata {
        duration: duration_secs.map(std::time::Duration::from_secs_f64),
        width: u32::from(width),
        height: u32::from(height),
        format: if video.is_some() { "video" } else { "audio" }.into(),
        is_seekable: seekable && response_seekable,
        video_tracks: video
            .iter()
            .map(|_| format!("video/h264/{width}x{height}"))
            .collect(),
        audio_tracks: audio
            .iter()
            .map(|a| format!("audio/{}ch/{}Hz", a.channels, a.rate))
            .collect(),
        is_live,
        title,
    }));
    // Prerolled: this is what moves the element to HaveEnoughData -> canplay.
    shared
        .events
        .send(PlayerEvent::StateChanged(PlaybackState::Paused));
    shared.probed.store(true, Ordering::SeqCst);

    let mut device: Option<Device> = None;
    let mut device_paused = true;
    let mut open_failed = false;
    // The demuxer ran dry for good; cleared only by a seek (`loop` attribute).
    let mut at_eof = false;
    let mut eos_sent = false;
    let mut eos_announced = false;

    'main: loop {
        if shared.quit.load(Ordering::SeqCst) {
            break;
        }

        if let Some(target) = shared.take_pending_seek() {
            if let Some(a) = &mut audio {
                let mut pcm = lock(&shared.pcm);
                pcm.queue.clear();
                pcm.pushed_frames = 0;
                pcm.base_secs = target;
                pcm.decoded_secs = target;
                pcm.last_emitted_pos = target;
                drop(pcm);
                a.decoder.reset();
            }
            shared.clock_jump(target);
            if let Some(v) = &mut video {
                v.pipeline.flush.flush_to(target);
                v.lagging = false;
            }
            match seek_to(&mut *reader, target, &audio, &mut video) {
                Ok(()) => {
                    if let Some(a) = &mut audio {
                        a.discard_until = Some(target);
                    }
                }
                // Report done anyway: silence beats an element stuck `seeking`.
                Err(e) => log::warn!("audio: seek to {target:.2}s failed: {e}"),
            }
            at_eof = false;
            eos_sent = false;
            eos_announced = false;
            shared.events.send(PlayerEvent::SeekDone(target));
        }

        // The device follows the flags; the decoder thread is its only owner.
        // Nothing left to play also pauses it, or an ended element would keep
        // the hardware powered ticking silence.
        if let Some(a) = &audio {
            let queue_empty = lock(&shared.pcm).queue.is_empty();
            let want_play = !(shared.paused.load(Ordering::SeqCst) || (at_eof && queue_empty));
            if want_play && device.is_none() && !open_failed {
                let userdata = Arc::as_ptr(shared) as *mut c_void;
                match Device::open(a.rate as f32, player_callback, userdata) {
                    Ok(opened) => device = Some(opened),
                    Err(e) => {
                        // Decoding continues; the queue fills and everything parks.
                        log::warn!("audio: could not open playback device: {e}");
                        open_failed = true;
                    }
                }
            }
            if device_paused == want_play {
                if let Some(device) = &device {
                    device.set_paused(!want_play);
                    device_paused = !want_play;
                }
            }
        }

        if at_eof {
            if !eos_sent {
                eos_sent = true;
                if let Some(v) = &video {
                    v.pipeline.send_eos();
                }
            }
            let mut pcm = lock(&shared.pcm);
            let audio_drained = pcm.queue.is_empty();
            if let Some(a) = &audio {
                let playing = !(shared.paused.load(Ordering::SeqCst) || (at_eof && audio_drained));
                emit_position(shared, &mut pcm, a.rate, playing);
            }
            let video_done = video.as_ref().is_none_or(|v| v.pipeline.is_done());
            if audio_drained && video_done && !eos_announced {
                eos_announced = true;
                if audio.is_some() {
                    // Land currentTime on the clip's end, not the last 250 ms
                    // tick, before the element hears `ended`.
                    let end = pcm.decoded_secs;
                    if end > pcm.last_emitted_pos {
                        pcm.last_emitted_pos = end;
                        shared.events.send(PlayerEvent::PositionChanged(end));
                    }
                }
                drop(pcm);
                shared.events.send(PlayerEvent::EndOfStream);
                continue 'main;
            }
            // Draining (the callback wakes us) or fully idle (a trait call does).
            let _woken = wait(&shared.work_cv, pcm);
            continue 'main;
        }

        if let Some(a) = &audio {
            let mut pcm = lock(&shared.pcm);
            let playing = !shared.paused.load(Ordering::SeqCst);
            emit_position(shared, &mut pcm, a.rate, playing);
            if pcm.queue.len() >= a.target_samples {
                // Full: wait for the SDL callback (or a state change), then
                // re-evaluate everything.
                let _woken = wait(&shared.work_cv, pcm);
                continue 'main;
            }
        }

        let packet = match reader.next_packet() {
            Ok(Some(packet)) => packet,
            Ok(None) => {
                at_eof = true;
                continue 'main;
            }
            Err(SymphoniaError::IoError(e)) if e.kind() == io::ErrorKind::UnexpectedEof => {
                // Also how a read aborted by quit or a pending seek surfaces;
                // the loop top sorts the three cases apart.
                if !shared.quit.load(Ordering::SeqCst) && !shared.seek_pending() {
                    at_eof = true;
                }
                continue 'main;
            }
            Err(e) => {
                if shared.quit.load(Ordering::SeqCst) {
                    break 'main;
                }
                shared
                    .events
                    .send(PlayerEvent::Error(format!("demux failed: {e}")));
                break 'main;
            }
        };

        if video
            .as_ref()
            .is_some_and(|v| v.track_id == packet.track_id)
        {
            let has_audio = audio.is_some();
            route_video_packet(&mut video, &packet, has_audio);
            continue 'main;
        }
        let Some(a) = &mut audio else {
            continue 'main;
        };
        if packet.track_id != a.track_id {
            continue 'main;
        }

        let buffer = match a.decoder.decode(&packet) {
            Ok(buffer) => buffer,
            Err(SymphoniaError::DecodeError(e)) => {
                log::debug!("audio: skipping malformed packet: {e}");
                continue 'main;
            }
            // A mid-stream format change cannot continue on one device config.
            Err(SymphoniaError::ResetRequired) => {
                log::info!("audio: stream parameters changed mid-play; ending");
                at_eof = true;
                continue 'main;
            }
            Err(e) => {
                shared
                    .events
                    .send(PlayerEvent::Error(format!("decode failed: {e}")));
                break 'main;
            }
        };
        if buffer.frames() == 0 {
            continue 'main;
        }
        if buffer.spec().rate() != a.rate {
            log::info!("audio: sample rate changed mid-play; ending");
            at_eof = true;
            continue 'main;
        }

        let pkt_start = packet_secs(&packet.pts, a.time_base, f64::from(a.rate));
        let frames = buffer.frames();
        buffer.copy_to_vec_interleaved::<f32>(&mut interleaved);
        let mut samples = interleaved.as_slice();

        // An accurate seek lands before the target; trim up to it.
        if let Some(target) = a.discard_until {
            let pkt_end = pkt_start + frames as f64 / f64::from(a.rate);
            if pkt_end <= target {
                continue 'main;
            }
            let skip = (((target - pkt_start) * f64::from(a.rate)).max(0.0) as usize).min(frames);
            samples = &samples[skip * a.channels..];
            a.discard_until = None;
        }

        let mut pcm = lock(&shared.pcm);
        push_stereo(&mut pcm, samples, a.channels);
        pcm.decoded_secs = pkt_start + frames as f64 / f64::from(a.rate);
    }

    // Close (joins SDL's audio thread) before `shared`'s Arc can drop: the
    // callback userdata points into it.
    drop(device);
    Ok(())
}

/// Seek to `target`. Video must restart at an IDR, but symphonia's mp4 seek is
/// sample-accurate: scan backward in steps until the first video packet after
/// the seek point is a keyframe (or the file start), and hand it onward. Audio
/// packets consumed by the scan are pre-target; the audio trim covers them.
fn seek_to(
    reader: &mut dyn FormatReader,
    target: f64,
    audio: &Option<AudioPipe>,
    video: &mut Option<VideoPipe>,
) -> Result<(), SymphoniaError> {
    const KEYFRAME_SCAN_STEP: f64 = 2.0;

    let seek_track = video
        .as_ref()
        .map(|v| v.track_id)
        .or(audio.as_ref().map(|a| a.track_id));
    let mut attempt = target;
    loop {
        let time = Time::try_from_secs_f64(attempt.max(0.0)).unwrap_or(Time::ZERO);
        reader.seek(
            SeekMode::Accurate,
            SeekTo::Time {
                time,
                track_id: seek_track,
            },
        )?;
        if video.is_none() {
            return Ok(());
        }
        loop {
            let packet = match reader.next_packet()? {
                Some(packet) => packet,
                None => return Ok(()),
            };
            let Some(v) = video.as_ref() else {
                return Ok(());
            };
            if packet.track_id != v.track_id {
                continue;
            }
            if attempt <= 0.0 || v.pipeline.is_keyframe(&packet.data) {
                let has_audio = audio.is_some();
                route_video_packet(video, &packet, has_audio);
                return Ok(());
            }
            attempt -= KEYFRAME_SCAN_STEP;
            break;
        }
    }
}

/// Hands a video packet to the video thread. With audio, a full channel means
/// video is lagging: drop until the next IDR fits, audio stays smooth. Without
/// audio the blocking send IS the demux pacing.
fn route_video_packet(
    video: &mut Option<VideoPipe>,
    packet: &symphonia::core::packet::Packet,
    has_audio: bool,
) {
    let Some(v) = video else {
        return;
    };
    if v.track_id != packet.track_id {
        return;
    }
    // MPEG's conventional timescale as the no-timebase fallback.
    let pts = packet_secs(&packet.pts, v.time_base, 90_000.0);
    let accepted = v
        .pipeline
        .send_sample(pts, &packet.data, !has_audio, v.lagging);
    if has_audio {
        if accepted == v.lagging {
            log::debug!(
                "video: {} at {pts:.3}s",
                if accepted { "resynced" } else { "lagging" }
            );
        }
        v.lagging = !accepted;
    }
}

fn packet_secs(pts: &Timestamp, time_base: Option<TimeBase>, fallback_hz: f64) -> f64 {
    match time_base {
        Some(tb) => tb.calc_time_saturating(*pts).as_secs_f64(),
        // Audio timebases are essentially always 1/rate; video 1/90000.
        None => pts.get() as f64 / fallback_hz,
    }
}

/// Interleaved anything -> interleaved stereo: mono duplicates, wider keeps the
/// front pair.
fn push_stereo(pcm: &mut Pcm, samples: &[f32], channels: usize) {
    match channels {
        1 => {
            for &sample in samples {
                pcm.queue.push_back(sample);
                pcm.queue.push_back(sample);
            }
        }
        2 => pcm.queue.extend(samples.iter().copied()),
        n => {
            for frame in samples.chunks_exact(n) {
                pcm.queue.push_back(frame[0]);
                pcm.queue.push_back(frame[1]);
            }
        }
    }
    pcm.pushed_frames += samples.len() as u64 / channels.max(1) as u64;
}

fn emit_position(shared: &Shared, pcm: &mut Pcm, rate: u32, playing: bool) {
    if !playing {
        return;
    }
    let played = pcm
        .pushed_frames
        .saturating_sub(pcm.queue.len() as u64 / u64::from(CHANNELS));
    let position = pcm.base_secs + played as f64 / f64::from(rate);
    if position - pcm.last_emitted_pos >= POSITION_EVENT_SECONDS {
        pcm.last_emitted_pos = position;
        shared.events.send(PlayerEvent::PositionChanged(position));
    }
}
