//! `decodeAudioData`: symphonia decodes the file, rubato retunes it to the context rate.
//!
//! Servo wants planar f32 back through [`AudioDecoderCallbacks`] in order: `ready` with
//! the channel count (it sizes its per-channel `Vec`s from that), one `progress` per
//! channel, then `eos`. `progress` takes a channel position mask, not an index — Servo
//! recovers the index as `log2(mask)`.
//!
//! Resampling is ours: Servo builds the `AudioBuffer` at the context rate, never the
//! file's, so a 48 kHz file in a 44.1 kHz context would play sharp.

use std::io::Cursor;

use rubato::audioadapter_buffers::direct::SequentialSliceOfVecs;
use rubato::{Async, FixedAsync, PolynomialDegree, Resampler};
use servo_media::audio::decoder::{
    AudioDecoder, AudioDecoderCallbacks, AudioDecoderError, AudioDecoderOptions,
};
use symphonia::core::codecs::audio::AudioDecoderOptions as SymphoniaOptions;
use symphonia::core::codecs::CodecParameters;
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatOptions, TrackType};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;

/// Channel ceiling: `progress` identifies a channel by a bit in a `u32` mask.
const MAX_CHANNELS: usize = u32::BITS as usize;

/// Frames per resampler pass; only sets how much rubato buffers internally.
const RESAMPLE_CHUNK_FRAMES: usize = 1024;

/// Better than linear on the common 48 -> 44.1 kHz step, far cheaper than sinc.
const RESAMPLE_DEGREE: PolynomialDegree = PolynomialDegree::Cubic;

/// A fixed ratio, so rubato never needs headroom for a ratio change.
const RESAMPLE_RATIO_HEADROOM: f64 = 1.0;

pub struct SymphoniaAudioDecoder;

impl AudioDecoder for SymphoniaAudioDecoder {
    fn decode(
        &self,
        data: Vec<u8>,
        callbacks: AudioDecoderCallbacks,
        options: Option<AudioDecoderOptions>,
    ) {
        let target_rate = options.unwrap_or_default().sample_rate;
        let max_seconds = crate::media::settings().max_decode_seconds;
        match decode_planar(data, target_rate, max_seconds) {
            Ok(planes) => {
                callbacks.ready(planes.len() as u32);
                // Hand each plane over by value so Servo's copy is the only one left.
                for (i, plane) in planes.into_iter().enumerate() {
                    callbacks.progress(Box::new(plane), 1 << i);
                }
                callbacks.eos();
            }
            Err(e) => {
                log::warn!("audio: decode failed: {e:?}");
                callbacks.error(e);
            }
        }
    }
}

/// Decodes `data` to planar f32 at `target_rate`, refusing clips longer than
/// `max_seconds` (`0` is unlimited).
fn decode_planar(
    data: Vec<u8>,
    target_rate: f32,
    max_seconds: u32,
) -> Result<Vec<Vec<f32>>, AudioDecoderError> {
    // `decodeAudioData` passes no file name or MIME type, so the probe has only bytes.
    let stream = MediaSourceStream::new(Box::new(Cursor::new(data)), Default::default());
    let mut reader = symphonia::default::get_probe()
        .probe(
            &Hint::new(),
            stream,
            FormatOptions::default(),
            MetadataOptions::default(),
        )
        .map_err(from_symphonia)?;

    let track = reader
        .first_track_known_codec(TrackType::Audio)
        .ok_or(AudioDecoderError::InvalidMediaFormat)?;
    let track_id = track.id;
    let params = track
        .codec_params
        .as_ref()
        .and_then(CodecParameters::audio)
        .ok_or(AudioDecoderError::InvalidMediaFormat)?;
    let mut decoder = symphonia::default::get_codecs()
        .make_audio_decoder(params, &SymphoniaOptions::default())
        .map_err(from_symphonia)?;

    let mut planes: Vec<Vec<f32>> = Vec::new();
    let mut chunk: Vec<Vec<f32>> = Vec::new();
    let mut rate = None;
    let mut frames = 0;
    loop {
        let packet = match reader.next_packet() {
            Ok(Some(packet)) => packet,
            Ok(None) => break,
            // The container ends mid-frame; keep what decoded.
            Err(SymphoniaError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
                break
            }
            Err(e) => return Err(from_symphonia(e)),
        };
        if packet.track_id != track_id {
            continue;
        }

        let audio = match decoder.decode(&packet) {
            Ok(audio) => audio,
            // One malformed packet: the rest of the file usually still decodes.
            Err(SymphoniaError::DecodeError(e)) => {
                log::debug!("audio: skipping malformed packet: {e}");
                continue;
            }
            // Parameters changed mid-stream; one `AudioBuffer` cannot represent that.
            Err(SymphoniaError::ResetRequired) => break,
            Err(e) => return Err(from_symphonia(e)),
        };
        if audio.frames() == 0 {
            continue;
        }
        if audio.num_planes() > MAX_CHANNELS {
            return Err(AudioDecoderError::InvalidMediaFormat);
        }

        // Checked as we go: a check at the end has already paid the memory.
        frames += audio.frames();
        if max_seconds > 0
            && frames as u64 > u64::from(max_seconds) * u64::from(audio.spec().rate())
        {
            return Err(AudioDecoderError::Backend(format!(
                "clip is longer than the {max_seconds}s decode limit"
            )));
        }

        audio.copy_to_vecs_planar(&mut chunk);
        rate = Some(audio.spec().rate());
        if planes.is_empty() {
            planes.resize(chunk.len(), Vec::new());
        } else if planes.len() != chunk.len() {
            // Servo reads the buffer length off channel 0, so planes must stay equal.
            return Err(AudioDecoderError::InvalidMediaFormat);
        }
        for (plane, decoded) in planes.iter_mut().zip(&chunk) {
            plane.extend_from_slice(decoded);
        }
    }

    // An empty result would resolve the promise with an unusable 0-length buffer.
    let (Some(rate), false) = (rate, planes.iter().all(Vec::is_empty)) else {
        return Err(AudioDecoderError::InvalidMediaFormat);
    };
    if rate as f32 == target_rate {
        return Ok(planes);
    }
    resample(planes, rate as f32, target_rate)
}

/// Resamples planar audio from `from` to `to`, preserving the channel count.
fn resample(input: Vec<Vec<f32>>, from: f32, to: f32) -> Result<Vec<Vec<f32>>, AudioDecoderError> {
    let channels = input.len();
    let frames_in = input[0].len();
    let ratio = f64::from(to) / f64::from(from);
    let mut resampler = Async::<f32>::new_poly(
        ratio,
        RESAMPLE_RATIO_HEADROOM,
        RESAMPLE_DEGREE,
        RESAMPLE_CHUNK_FRAMES,
        channels,
        FixedAsync::Input,
    )
    .map_err(|e| AudioDecoderError::Backend(e.to_string()))?;

    // Rubato needs room for its startup delay and a trailing pass.
    let capacity = resampler.process_all_needed_output_len(frames_in);
    let mut output = vec![vec![0.0; capacity]; channels];
    let source = SequentialSliceOfVecs::new(&input, channels, frames_in)
        .map_err(|e| AudioDecoderError::Backend(e.to_string()))?;
    let mut sink = SequentialSliceOfVecs::new_mut(&mut output, channels, capacity)
        .map_err(|e| AudioDecoderError::Backend(e.to_string()))?;
    // Trims the startup delay, so the output starts on the first real frame.
    let (_, frames_out) = resampler
        .process_all_into_buffer(&source, &mut sink, frames_in, None)
        .map_err(|e| AudioDecoderError::Backend(e.to_string()))?;

    for plane in &mut output {
        plane.truncate(frames_out);
    }
    Ok(output)
}

fn from_symphonia(error: SymphoniaError) -> AudioDecoderError {
    match error {
        SymphoniaError::DecodeError(_) => AudioDecoderError::InvalidSample,
        SymphoniaError::Unsupported(_) => AudioDecoderError::InvalidMediaFormat,
        SymphoniaError::IoError(_) => AudioDecoderError::BufferReadFailed,
        e => AudioDecoderError::Backend(e.to_string()),
    }
}

/// A 16-bit PCM wav holding a [`TONE_HZ`] sine, the same tone on every channel.
/// Shared with the player tests.
#[cfg(test)]
pub(crate) fn synth_wav(rate: u32, channels: u16, frames: u32) -> Vec<u8> {
    let data_len = frames * u32::from(channels) * 2;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * u32::from(channels) * 2).to_le_bytes());
    out.extend_from_slice(&(channels * 2).to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for frame in 0..frames {
        let phase = std::f32::consts::TAU * TONE_HZ * frame as f32 / rate as f32;
        let sample = (phase.sin() * TONE_PEAK * f32::from(i16::MAX)) as i16;
        for _ in 0..channels {
            out.extend_from_slice(&sample.to_le_bytes());
        }
    }
    out
}

/// Test tone frequency; odd on purpose so resampling errors shift it visibly.
#[cfg(test)]
pub(crate) const TONE_HZ: f32 = 441.0;

#[cfg(test)]
pub(crate) const TONE_PEAK: f32 = 0.5;

#[cfg(test)]
mod tests {
    use super::*;
    use servo_media::audio::decoder::AudioDecoderCallbacksBuilder;
    use std::sync::{Arc, Mutex};

    use super::synth_wav as wav;

    /// Zero crossings per second, i.e. twice the tone's frequency.
    fn frequency(plane: &[f32], rate: f32) -> f32 {
        let crossings = plane
            .windows(2)
            .filter(|w| (w[0] < 0.0) != (w[1] < 0.0))
            .count();
        crossings as f32 / 2.0 / (plane.len() as f32 / rate)
    }

    #[test]
    fn decodes_planar_at_the_context_rate() {
        let planes = decode_planar(wav(44_100, 2, 44_100), 44_100.0, 0).unwrap();
        assert_eq!(planes.len(), 2);
        assert_eq!(planes[0].len(), 44_100);
        assert_eq!(planes[1].len(), 44_100);
        assert!((frequency(&planes[0], 44_100.0) - TONE_HZ).abs() < 1.0);
    }

    /// Retuned, not pitch-shifted: same tone and duration, different frame count.
    #[test]
    fn resamples_to_the_context_rate() {
        let planes = decode_planar(wav(48_000, 1, 48_000), 44_100.0, 0).unwrap();
        assert_eq!(planes.len(), 1);
        let frames = planes[0].len() as f32;
        assert!(
            (frames - 44_100.0).abs() < 44_100.0 * 0.01,
            "expected ~44100 frames, got {frames}"
        );
        assert!((frequency(&planes[0], 44_100.0) - TONE_HZ).abs() < 1.0);
        let peak = planes[0].iter().fold(0.0f32, |a, s| a.max(s.abs()));
        assert!((peak - TONE_PEAK).abs() < 0.01, "peak {peak}");
    }

    /// Servo indexes by `log2(mask)`, so masks are `1 << i` and `ready` comes first.
    #[test]
    fn reports_channels_as_position_masks() {
        let ready = Arc::new(Mutex::new(None));
        let masks = Arc::new(Mutex::new(Vec::new()));
        let eos = Arc::new(Mutex::new(false));

        let (ready_, masks_, eos_) = (ready.clone(), masks.clone(), eos.clone());
        let callbacks = AudioDecoderCallbacksBuilder::default()
            .ready(move |channels| *ready_.lock().unwrap() = Some(channels))
            .progress(move |buffer, mask| {
                masks_
                    .lock()
                    .unwrap()
                    .push((mask, (*buffer).as_ref().len()));
            })
            .eos(move || *eos_.lock().unwrap() = true)
            .build();
        SymphoniaAudioDecoder.decode(wav(44_100, 2, 1_000), callbacks, None);

        assert_eq!(*ready.lock().unwrap(), Some(2));
        assert_eq!(*masks.lock().unwrap(), vec![(1, 1_000), (2, 1_000)]);
        assert!(*eos.lock().unwrap());
    }

    /// The cap refuses a long clip mid-decode, before it can exhaust the board.
    #[test]
    fn rejects_a_clip_past_the_decode_cap() {
        let two_seconds = wav(44_100, 1, 88_200);
        assert!(decode_planar(two_seconds.clone(), 44_100.0, 1).is_err());
        assert!(decode_planar(two_seconds.clone(), 44_100.0, 2).is_ok());
        assert!(decode_planar(two_seconds, 44_100.0, 0).is_ok());
    }

    /// `decodeAudioData` rejects on junk instead of resolving with an empty buffer.
    #[test]
    fn rejects_undecodable_data() {
        assert!(decode_planar(b"not audio at all".to_vec(), 44_100.0, 0).is_err());
        assert!(decode_planar(Vec::new(), 44_100.0, 0).is_err());
        assert!(decode_planar(wav(44_100, 2, 0), 44_100.0, 0).is_err());
    }
}
