//! `<audio>`: a servo-media [`Player`] that demuxes with symphonia and plays
//! through SDL2 (see [`super::device`]).
//!
//! Servo's element pushes raw response bytes in as the fetch progresses and the
//! protocol is driven from our side: nothing flows until the player emits
//! `NeedData` (the element's data source starts locked, and every new fetch —
//! including the one a seek starts — locks again), backpressure is `push_data`
//! returning `Err(EnoughData)` (the element cancels the fetch), and the only way
//! to get bytes at an offset is the `SeekData(offset, SeekLock)` handshake (the
//! element starts a `Range` fetch there and unlocks). `MetadataUpdated` then
//! `StateChanged(Paused)` are what make the element playable at all — ready
//! state moves HaveNothing -> HaveMetadata -> HaveEnoughData on exactly those.
//!
//! Trait methods run on the script thread under the element's mutex and must
//! never block; everything slow lives on one decoder thread per player
//! ([`pipeline`]), which owns the SDL device and does the blocking `SeekData`
//! handshakes ([`source`]). The SDL audio thread only drains the PCM queue.
//! H.264 video tracks are routed to [`super::video`], which presents against
//! this player's clock.

use std::ops::Range;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use servo_base::generic_channel::GenericCallback;
use servo_media::player::video::VideoFrameRenderer;
use servo_media::player::{PlaybackState, Player, PlayerError, PlayerEvent, StreamType};
use servo_media::streams::registry::MediaStreamId;
use servo_media::traits::{MediaInstance, MediaInstanceError};
use servo_media::SupportsMediaType;

mod pipeline;
mod shared;
mod source;
#[cfg(test)]
mod tests;

use self::pipeline::spawn_decoder;
pub(crate) use self::shared::Shared;
use self::shared::{lock, CallbackSink, EventSink, BYTE_HIGH_WATER};

pub struct SdlAudioPlayer {
    id: usize,
    stream_type: StreamType,
    shared: Arc<Shared>,
    // Script-thread-only state; the element serializes access through its mutex.
    can_resume: std::cell::Cell<bool>,
    playback_rate: std::cell::Cell<f64>,
    rate_warned: std::cell::Cell<bool>,
    stopped: std::cell::Cell<bool>,
}

impl SdlAudioPlayer {
    pub fn new(
        id: usize,
        stream_type: StreamType,
        observer: GenericCallback<PlayerEvent>,
        video_renderer: Option<Arc<Mutex<dyn VideoFrameRenderer>>>,
    ) -> Self {
        Self::with_sink(
            id,
            stream_type,
            Box::new(CallbackSink(Mutex::new(observer))),
            video_renderer,
        )
    }

    fn with_sink(
        id: usize,
        stream_type: StreamType,
        events: Box<dyn EventSink>,
        video_renderer: Option<Arc<Mutex<dyn VideoFrameRenderer>>>,
    ) -> Self {
        let shared = Arc::new(Shared::new(events));
        spawn_decoder(shared.clone(), stream_type, video_renderer);
        Self {
            id,
            stream_type,
            shared,
            can_resume: std::cell::Cell::new(false),
            playback_rate: std::cell::Cell::new(1.0),
            rate_warned: std::cell::Cell::new(false),
            stopped: std::cell::Cell::new(false),
        }
    }

    fn shut_down(&self) {
        self.shared.quit.store(true, Ordering::SeqCst);
        self.shared.paused.store(true, Ordering::SeqCst);
        self.shared.clock_freeze();
        lock(&self.shared.pcm).queue.clear();
        self.shared.wake_all();
    }
}

impl Drop for SdlAudioPlayer {
    /// The decoder thread is detached, never joined: it may be blocked in a
    /// `SeekLock` handshake that only resolves once the element's task queue
    /// drops our event. It exits on its own once the flags and channels say so.
    fn drop(&mut self) {
        self.shut_down();
    }
}

impl Player for SdlAudioPlayer {
    fn play(&self) -> Result<(), PlayerError> {
        if !self.shared.paused.swap(false, Ordering::SeqCst) {
            return Ok(());
        }
        self.can_resume.set(false);
        self.shared.clock_run();
        self.shared.wake_all();
        self.shared
            .events
            .send(PlayerEvent::StateChanged(PlaybackState::Playing));
        Ok(())
    }

    fn pause(&self) -> Result<(), PlayerError> {
        if self.shared.paused.swap(true, Ordering::SeqCst) {
            return Ok(());
        }
        self.can_resume.set(true);
        self.shared.clock_freeze();
        self.shared.wake_all();
        self.shared
            .events
            .send(PlayerEvent::StateChanged(PlaybackState::Paused));
        Ok(())
    }

    fn paused(&self) -> bool {
        self.shared.paused.load(Ordering::SeqCst)
    }

    fn can_resume(&self) -> bool {
        self.can_resume.get()
    }

    fn stop(&self) -> Result<(), PlayerError> {
        if self.stopped.replace(true) {
            return Ok(());
        }
        self.can_resume.set(false);
        self.shut_down();
        self.shared
            .events
            .send(PlayerEvent::StateChanged(PlaybackState::Stopped));
        Ok(())
    }

    fn seek(&self, time: f64) -> Result<(), PlayerError> {
        if self.stream_type != StreamType::Seekable {
            return Err(PlayerError::NonSeekableStream);
        }
        if matches!(lock(&self.shared.meta).duration_secs, Some(duration) if time > duration) {
            return Err(PlayerError::SeekOutOfRange);
        }
        self.shared
            .pending_seek
            .store(time.max(0.0).to_bits(), Ordering::SeqCst);
        self.shared.wake_all();
        Ok(())
    }

    fn seekable(&self) -> Vec<Range<f64>> {
        let response_seekable = lock(&self.shared.stream).response_seekable;
        if self.stream_type == StreamType::Seekable && response_seekable {
            if let Some(duration) = lock(&self.shared.meta).duration_secs {
                return vec![0.0..duration];
            }
        }
        self.buffered()
    }

    fn buffered(&self) -> Vec<Range<f64>> {
        let (base, head, total) = {
            let stream = lock(&self.shared.stream);
            (stream.base, stream.head(), stream.total_len)
        };
        let duration = lock(&self.shared.meta).duration_secs;
        if let (Some(total), Some(duration)) = (total.filter(|total| *total > 0), duration) {
            let scale = duration / total as f64;
            return vec![base as f64 * scale..head as f64 * scale];
        }
        // No sizes to scale by: report what has actually been decoded.
        let pcm = lock(&self.shared.pcm);
        if pcm.decoded_secs > pcm.base_secs {
            return vec![pcm.base_secs..pcm.decoded_secs];
        }
        vec![]
    }

    fn set_mute(&self, muted: bool) -> Result<(), PlayerError> {
        self.shared.muted.store(muted, Ordering::Relaxed);
        Ok(())
    }

    fn muted(&self) -> bool {
        self.shared.muted.load(Ordering::Relaxed)
    }

    fn set_volume(&self, volume: f64) -> Result<(), PlayerError> {
        self.shared
            .volume
            .store(volume.clamp(0.0, 1.0).to_bits(), Ordering::Relaxed);
        Ok(())
    }

    fn volume(&self) -> f64 {
        f64::from_bits(self.shared.volume.load(Ordering::Relaxed))
    }

    fn set_input_size(&self, size: u64) -> Result<(), PlayerError> {
        lock(&self.shared.stream).total_len = Some(size);
        Ok(())
    }

    fn set_seekable(&self, seekable: bool) -> Result<(), PlayerError> {
        lock(&self.shared.stream).response_seekable = seekable;
        Ok(())
    }

    /// Accepted but not applied: nothing here can time-stretch yet. Servo sets
    /// 1.0 before every play, so normal pages never hit the warning.
    fn set_playback_rate(&self, playback_rate: f64) -> Result<(), PlayerError> {
        self.playback_rate.set(playback_rate);
        if playback_rate != 1.0 && !self.rate_warned.replace(true) {
            log::warn!("audio: playbackRate {playback_rate} requested; playing at 1.0");
        }
        Ok(())
    }

    fn playback_rate(&self) -> f64 {
        self.playback_rate.get()
    }

    fn push_data(&self, data: Vec<u8>) -> Result<(), PlayerError> {
        if self.stopped.get() {
            return Err(PlayerError::BufferPushFailed);
        }
        let mut stream = lock(&self.shared.stream);
        if self.stream_type == StreamType::Seekable
            && stream.data.len() + data.len() > BYTE_HIGH_WATER
        {
            // The element cancels the fetch on this; the decoder refetches at the
            // head once it drains. The refused chunk is re-downloaded then.
            stream.stalled = true;
            return Err(PlayerError::EnoughData);
        }
        stream.data.extend_from_slice(&data);
        // A live stream has no refetch, so shed the oldest bytes instead.
        if stream.data.len() > BYTE_HIGH_WATER {
            let n = stream.data.len() - BYTE_HIGH_WATER;
            stream.data.drain(..n);
            stream.base += n as u64;
            stream.read_pos = stream.read_pos.max(stream.base);
        }
        drop(stream);
        self.shared.bytes_cv.notify_all();
        Ok(())
    }

    fn end_of_stream(&self) -> Result<(), PlayerError> {
        lock(&self.shared.stream).eos = true;
        self.shared.bytes_cv.notify_all();
        Ok(())
    }

    fn set_stream(&self, _: &MediaStreamId, _: bool) -> Result<(), PlayerError> {
        Err(PlayerError::SetStreamFailed)
    }

    fn render_use_gl(&self) -> bool {
        false
    }

    fn set_audio_track(&self, stream_index: i32, enabled: bool) -> Result<(), PlayerError> {
        // Exactly one track is ever reported, so only index 0 exists.
        if stream_index != 0 {
            return Err(PlayerError::SetTrackFailed);
        }
        self.shared.track_enabled.store(enabled, Ordering::Relaxed);
        Ok(())
    }

    fn set_video_track(&self, stream_index: i32, enabled: bool) -> Result<(), PlayerError> {
        if stream_index != 0 {
            return Err(PlayerError::SetTrackFailed);
        }
        self.shared
            .video_track_enabled
            .store(enabled, Ordering::Relaxed);
        Ok(())
    }
}

impl MediaInstance for SdlAudioPlayer {
    fn get_id(&self) -> usize {
        self.id
    }

    fn mute(&self, val: bool) -> Result<(), MediaInstanceError> {
        self.set_mute(val).map_err(|_| MediaInstanceError)
    }

    fn suspend(&self) -> Result<(), MediaInstanceError> {
        self.pause().map_err(|_| MediaInstanceError)
    }

    /// Only resumes a suspend-pause; a user-paused element stays paused.
    fn resume(&self) -> Result<(), MediaInstanceError> {
        if !self.can_resume() {
            return Ok(());
        }
        self.play().map_err(|_| MediaInstanceError)
    }
}

/// `canPlayType` and `<source type>` filtering. Probably for what plays
/// outright, Maybe for containers that may hide an unsupported codec, No for
/// the rest. `allow_video` is the `[video] enabled` switch.
pub fn can_play_type(media_type: &str, allow_video: bool) -> SupportsMediaType {
    let media_type = media_type.trim().to_ascii_lowercase();
    let mut parts = media_type.split(';');
    let essence = parts.next().unwrap_or("").trim();
    let codecs: Vec<&str> = parts
        .filter_map(|param| param.trim().strip_prefix("codecs="))
        .flat_map(|list| list.trim_matches('"').split(','))
        .map(str::trim)
        .filter(|codec| !codec.is_empty())
        .collect();

    let container = match essence {
        "audio/mpeg" | "audio/mp3" | "audio/x-mp3" => Container::Plain,
        "audio/wav" | "audio/x-wav" | "audio/wave" | "audio/vnd.wave" => Container::Plain,
        "audio/flac" | "audio/x-flac" => Container::Plain,
        "audio/aac" | "audio/aacp" => Container::Plain,
        "audio/ogg" | "application/ogg" => Container::Ogg,
        "audio/mp4" | "audio/x-m4a" | "audio/m4a" => Container::Mp4,
        "video/mp4" | "video/x-m4v" if allow_video => Container::Mp4Video,
        _ => return SupportsMediaType::No,
    };

    if codecs.is_empty() {
        return match container {
            Container::Plain => SupportsMediaType::Probably,
            // The container may hide a codec we have no decoder for
            // (opus in ogg, alac in audio mp4, hevc/av1 in video mp4).
            Container::Ogg | Container::Mp4 | Container::Mp4Video => SupportsMediaType::Maybe,
        };
    }
    let supported = |codec: &&str| match container {
        Container::Plain => true,
        Container::Ogg => matches!(*codec, "vorbis" | "flac"),
        Container::Mp4 => *codec == "mp4a" || codec.starts_with("mp4a.40"),
        Container::Mp4Video => {
            *codec == "mp4a"
                || codec.starts_with("mp4a.40")
                || *codec == "avc1"
                || codec.starts_with("avc1.")
        }
    };
    if codecs.iter().all(supported) {
        SupportsMediaType::Probably
    } else {
        SupportsMediaType::No
    }
}

enum Container {
    /// The MIME essence pins the codec.
    Plain,
    Ogg,
    Mp4,
    Mp4Video,
}
