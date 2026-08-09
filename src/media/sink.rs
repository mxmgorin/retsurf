//! WebAudio output: a servo-media [`AudioSink`] backed by an SDL2 audio device.
//!
//! Servo's render thread parks as soon as [`AudioSink::has_enough_data`] is true and
//! only [`AudioRenderThreadMsg::SinkNeedData`] wakes it, so draining the queue must
//! send that message — the hand-off is the whole protocol.
//!
//! The device runs through raw `sdl2::sys`: rust-sdl2's safe `AudioDevice` owns an
//! `!Send` `AudioSubsystem`, but an `AudioSink` must be `Send`. Keeping
//! `SDL_INIT_AUDIO` up for the process is [`crate::media::init`]'s job.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::ffi::{c_int, c_void};
use std::mem::MaybeUninit;
use std::ptr;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex, MutexGuard};

use sdl2::sys;
use servo_media::audio::block::Chunk;
use servo_media::audio::audio_node::ChannelInterpretation;
use servo_media::audio::render_thread::{AudioRenderThreadMsg, SinkEosCallback};
use servo_media::audio::sink::{AudioSink, AudioSinkError};
use servo_media::streams::MediaSocket;

/// Device buffer in frames (SDL wants a power of two). ~23 ms at 44.1 kHz: snappy
/// without waking a handheld's audio thread every few milliseconds.
const DEVICE_BUFFER_FRAMES: u16 = 1024;

/// servo-media hardcodes stereo for real-time contexts, so the device matches it.
const CHANNELS: u8 = 2;

/// Queue depth ahead of the device: the render thread idles above this mark and is
/// woken below it. Four device buffers (~93 ms) rides out a slow render pass.
const QUEUE_TARGET_SAMPLES: usize = 4 * DEVICE_BUFFER_FRAMES as usize * CHANNELS as usize;

/// Shared between Servo's render thread and SDL's audio thread. One mutex covers
/// both fields: the callback needs them together and each section is a memcpy.
#[derive(Default)]
struct Queue {
    /// Interleaved stereo samples waiting to be played.
    samples: VecDeque<f32>,
    /// Wakes the render thread when `samples` runs low; set by [`AudioSink::init`].
    notify: Option<Sender<AudioRenderThreadMsg>>,
}

/// Poison-tolerant lock — the callback must not unwind across the FFI boundary.
fn lock(queue: &Mutex<Queue>) -> MutexGuard<'_, Queue> {
    queue
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// SDL's audio thread: drains the queue into the device buffer and wakes the render
/// thread once the queue falls below target.
///
/// # Safety
///
/// `userdata` is the `Arc<Mutex<Queue>>` handed to [`Device::open`]; the sink
/// outlives the device, so it is live for every call.
unsafe extern "C" fn audio_callback(userdata: *mut c_void, stream: *mut u8, len: c_int) {
    let queue = unsafe { &*(userdata as *const Mutex<Queue>) };
    let out = unsafe {
        std::slice::from_raw_parts_mut(stream as *mut f32, len as usize / size_of::<f32>())
    };

    let mut queue = lock(queue);
    let available = queue.samples.len().min(out.len());
    for (dst, sample) in out.iter_mut().zip(queue.samples.drain(..available)) {
        *dst = sample;
    }
    // Underrun: play silence rather than whatever the driver left in the buffer.
    out[available..].fill(0.0);

    if queue.samples.len() < QUEUE_TARGET_SAMPLES {
        if let Some(notify) = &queue.notify {
            let _ = notify.send(AudioRenderThreadMsg::SinkNeedData);
        }
    }
}

/// An open SDL playback device, identified by nothing but its id so it stays `Send`.
struct Device {
    id: sys::SDL_AudioDeviceID,
}

impl Device {
    /// Opens the default playback device. SDL keeps `queue`'s address as the
    /// callback userdata, so that `Arc` must outlive the `Device`.
    fn open(sample_rate: f32, queue: &Arc<Mutex<Queue>>) -> Result<Self, String> {
        let desired = sys::SDL_AudioSpec {
            freq: sample_rate as c_int,
            format: sys::AUDIO_F32SYS as sys::SDL_AudioFormat,
            channels: CHANNELS,
            silence: 0,
            samples: DEVICE_BUFFER_FRAMES,
            padding: 0,
            size: 0,
            callback: Some(audio_callback),
            userdata: Arc::as_ptr(queue) as *mut c_void,
        };
        let mut obtained = MaybeUninit::uninit();
        // allowed_changes = 0: SDL converts internally, so the callback always gets
        // f32 stereo at `sample_rate` whatever the hardware offers.
        let id =
            unsafe { sys::SDL_OpenAudioDevice(ptr::null(), 0, &desired, obtained.as_mut_ptr(), 0) };
        if id == 0 {
            return Err(sdl2::get_error());
        }
        Ok(Self { id })
    }

    fn set_paused(&self, paused: bool) {
        unsafe { sys::SDL_PauseAudioDevice(self.id, c_int::from(paused)) };
    }
}

impl Drop for Device {
    fn drop(&mut self) {
        // Joins SDL's audio thread, so no callback can be in flight afterwards.
        unsafe { sys::SDL_CloseAudioDevice(self.id) };
    }
}

/// The sink servo-media pushes rendered audio into. One per `AudioContext`.
#[derive(Default)]
pub struct SdlAudioSink {
    /// Opened on the first [`AudioSink::play`]: a page may build an `AudioContext`
    /// and never start it, and an idle open device keeps the hardware powered.
    device: RefCell<Option<Device>>,
    /// Shared with SDL's audio thread; its address is the callback's userdata.
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

impl Drop for SdlAudioSink {
    fn drop(&mut self) {
        // Close the device before `queue` is released: SDL holds that allocation's
        // address as the callback's userdata.
        *self.device.borrow_mut() = None;
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
        let mut device = self.device.borrow_mut();
        if device.is_none() {
            *device = Some(
                Device::open(self.sample_rate.get(), &self.queue).map_err(|e| {
                    log::warn!("audio: could not open playback device: {e}");
                    AudioSinkError::Backend(e)
                })?,
            );
        }
        device
            .as_ref()
            .expect("opened just above, or already present")
            .set_paused(false);
        Ok(())
    }

    fn stop(&self) -> Result<(), AudioSinkError> {
        if let Some(device) = self.device.borrow().as_ref() {
            device.set_paused(true);
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
