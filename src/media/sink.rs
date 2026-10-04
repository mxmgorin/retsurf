//! WebAudio output: a servo-media [`AudioSink`] playing through the mixer.
//!
//! Servo's render thread parks as soon as [`AudioSink::has_enough_data`] is true and
//! only [`AudioRenderThreadMsg::SinkNeedData`] wakes it, so draining the queue must
//! send that message — the hand-off is the whole protocol.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};

use servo_media::audio::audio_node::ChannelInterpretation;
use servo_media::audio::block::Chunk;
use servo_media::audio::render_thread::{AudioRenderThreadMsg, SinkEosCallback};
use servo_media::audio::sink::{AudioSink, AudioSinkError};
use servo_media::streams::MediaSocket;

use super::device::{lock, BUFFER_FRAMES, CHANNELS};
use super::mixer::{self, mix_from, Attachment, MixSource};

/// Queue depth ahead of the device: the render thread idles above this mark and is
/// woken below it. Four device buffers (~93 ms) rides out a slow render pass.
const QUEUE_TARGET_SAMPLES: usize = 4 * BUFFER_FRAMES as usize * CHANNELS as usize;

/// Shared between Servo's render thread and SDL's audio thread. One mutex covers
/// both fields: the mixer needs them together and each section is a memcpy.
#[derive(Default)]
struct Queue {
    /// Interleaved stereo samples waiting to be played.
    samples: VecDeque<f32>,
    /// Wakes the render thread when `samples` runs low; set by [`AudioSink::init`].
    notify: Option<Sender<AudioRenderThreadMsg>>,
}

/// Drains the queue into the mix and wakes the render thread once the queue falls
/// below target.
impl MixSource for Mutex<Queue> {
    fn mix_into(&self, out: &mut [f32]) {
        let mut queue = lock(self);
        mix_from(out, &mut queue.samples, 1.0);

        if queue.samples.len() < QUEUE_TARGET_SAMPLES {
            if let Some(notify) = &queue.notify {
                let _ = notify.send(AudioRenderThreadMsg::SinkNeedData);
            }
        }
    }
}

/// The sink servo-media pushes rendered audio into. One per `AudioContext`.
#[derive(Default)]
pub struct SdlAudioSink {
    /// Attached on the first [`AudioSink::play`]: a page may build an `AudioContext`
    /// and never start it, and an idle open device keeps the hardware powered.
    attachment: RefCell<Option<Attachment>>,
    queue: Arc<Mutex<Queue>>,
    /// Set by [`AudioSink::init`], always before the render thread plays the sink.
    sample_rate: Cell<f32>,
    /// Set for a `MediaStreamDestinationNode`: it routes into a `MediaStream` we have
    /// no backend for, so the sink stays silent instead of opening a device.
    stream_only: Cell<bool>,
}

impl SdlAudioSink {
    /// Nowhere to put audio: a `MediaStreamDestinationNode`, or output off in config.
    fn silent(&self) -> bool {
        self.stream_only.get() || !crate::media::settings().output
    }
}

impl AudioSink for SdlAudioSink {
    fn init(
        &self,
        sample_rate: f32,
        render_thread_channel: Sender<AudioRenderThreadMsg>,
    ) -> Result<(), AudioSinkError> {
        self.sample_rate.set(sample_rate);
        lock(&self.queue).notify = Some(render_thread_channel);
        Ok(())
    }

    /// Claims the sink for a `MediaStreamDestinationNode`. `Err` would panic the
    /// render thread (`MediaStreamDestinationNode::new` unwraps), so accept and drop.
    fn init_stream(&self, _: u8, _: f32, _: Box<dyn MediaSocket>) -> Result<(), AudioSinkError> {
        self.stream_only.set(true);
        Ok(())
    }

    fn play(&self) -> Result<(), AudioSinkError> {
        if self.silent() {
            return Ok(());
        }
        let mut attachment = self.attachment.borrow_mut();
        if attachment.is_none() {
            let source: Arc<dyn MixSource> = self.queue.clone();
            *attachment = Some(mixer::attach(self.sample_rate.get(), source).map_err(|e| {
                log::warn!("audio: could not open playback device: {e}");
                AudioSinkError::Backend(e)
            })?);
        }
        attachment
            .as_ref()
            .expect("attached just above, or already present")
            .set_active(true);
        Ok(())
    }

    fn stop(&self) -> Result<(), AudioSinkError> {
        if let Some(attachment) = self.attachment.borrow().as_ref() {
            attachment.set_active(false);
        }
        Ok(())
    }

    /// A silent sink claims to be full: nothing drains its queue, so the render thread
    /// would spin instead of parking.
    fn has_enough_data(&self) -> bool {
        self.silent() || lock(&self.queue).samples.len() >= QUEUE_TARGET_SAMPLES
    }

    fn push_data(&self, chunk: Chunk) -> Result<(), AudioSinkError> {
        if self.silent() {
            return Ok(());
        }
        // No block at all means nothing is connected, which is silence.
        let mut block = chunk.blocks.into_iter().next().unwrap_or_default();
        // The destination node already mixed to `CHANNELS`; this only normalizes the
        // silent and mono shorthands a block can carry.
        block.mix(CHANNELS, ChannelInterpretation::Speakers);
        lock(&self.queue).samples.extend(block.interleave());
        Ok(())
    }

    /// Only an `OfflineAudioContext` fires this; a real-time sink never finishes.
    fn set_eos_callback(&self, _: SinkEosCallback) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use servo_media::audio::block::{Block, FRAMES_PER_BLOCK_USIZE};

    /// The destination node guarantees the channel *count*, not the representation, so
    /// interleaving must survive silent, mono-shorthand, and explicit stereo blocks.
    #[test]
    fn push_data_always_yields_stereo_frames() {
        let stereo = FRAMES_PER_BLOCK_USIZE * CHANNELS as usize;

        let mut mono = Block::default();
        mono.data_mut().fill(0.5);
        let cases = [
            (Chunk::default(), vec![0.0; stereo]),
            (Chunk::explicit_silence(), vec![0.0; stereo]),
            // Mono is upmixed by duplication, so both channels carry the sample.
            (
                Chunk {
                    blocks: [mono].into_iter().collect(),
                },
                vec![0.5; stereo],
            ),
        ];

        for (chunk, expected) in cases {
            let sink = SdlAudioSink::default();
            sink.push_data(chunk).unwrap();
            let queue = lock(&sink.queue);
            assert_eq!(queue.samples.iter().copied().collect::<Vec<_>>(), expected);
        }
    }

    /// The render thread parks whenever the sink is full, so the fill mark must be
    /// reached by pushing blocks and released by draining them.
    #[test]
    fn has_enough_data_tracks_the_queue() {
        let sink = SdlAudioSink::default();
        assert!(!sink.has_enough_data());

        while !sink.has_enough_data() {
            sink.push_data(Chunk::explicit_silence()).unwrap();
        }
        assert_eq!(lock(&sink.queue).samples.len(), QUEUE_TARGET_SAMPLES);

        lock(&sink.queue).samples.pop_front();
        assert!(!sink.has_enough_data());
    }
}
