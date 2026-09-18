//! Raw-SDL playback device, shared by the WebAudio sink and the `<audio>` player.
//!
//! Runs through `sdl2::sys` because rust-sdl2's safe `AudioDevice` owns an `!Send`
//! `AudioSubsystem`, while both users live on threads other than main. Keeping
//! `SDL_INIT_AUDIO` up for the process is [`crate::media::init`]'s job.

use std::ffi::{c_int, c_void};
use std::mem::MaybeUninit;
use std::ptr;

use sdl2::sys;

/// Device buffer in frames (SDL wants a power of two). ~23 ms at 44.1 kHz: snappy
/// without waking a handheld's audio thread every few milliseconds.
pub const BUFFER_FRAMES: u16 = 1024;

/// Everything is mixed to stereo before it reaches a device.
pub const CHANNELS: u8 = 2;

/// The SDL audio-thread callback: fill `stream` (`len` bytes) from `userdata`.
pub type Callback = unsafe extern "C" fn(userdata: *mut c_void, stream: *mut u8, len: c_int);

/// An open SDL playback device, identified by nothing but its id so it stays `Send`.
pub struct Device {
    id: sys::SDL_AudioDeviceID,
}

impl Device {
    /// Opens the default playback device for f32 stereo at `sample_rate`. SDL keeps
    /// `userdata` for the callback, so whatever it points at must outlive the device.
    pub fn open(
        sample_rate: f32,
        callback: Callback,
        userdata: *mut c_void,
    ) -> Result<Self, String> {
        let desired = sys::SDL_AudioSpec {
            freq: sample_rate as c_int,
            format: sys::AUDIO_F32SYS as sys::SDL_AudioFormat,
            channels: CHANNELS,
            silence: 0,
            samples: BUFFER_FRAMES,
            padding: 0,
            size: 0,
            callback: Some(callback),
            userdata,
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

    pub fn set_paused(&self, paused: bool) {
        unsafe { sys::SDL_PauseAudioDevice(self.id, c_int::from(paused)) };
    }
}

impl Drop for Device {
    fn drop(&mut self) {
        // Joins SDL's audio thread, so no callback can be in flight afterwards.
        unsafe { sys::SDL_CloseAudioDevice(self.id) };
    }
}

/// Poison-tolerant lock: an SDL callback must not unwind across FFI, and a
/// panicked producer thread must not take its peer down with it.
pub(crate) fn lock<T>(mutex: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// The callback's `stream` as the f32 slice SDL sized it as. Safety:
/// `stream`/`len` must be an SDL audio callback's arguments for an f32 device.
pub(crate) unsafe fn out_slice<'a>(stream: *mut u8, len: c_int) -> &'a mut [f32] {
    unsafe { std::slice::from_raw_parts_mut(stream as *mut f32, len as usize / size_of::<f32>()) }
}

/// Fill a callback's `out` from `queue`, scaled by `factor`. The tail past what
/// the queue held plays silence, not whatever the driver left in the buffer.
pub(crate) fn drain_into(
    out: &mut [f32],
    queue: &mut std::collections::VecDeque<f32>,
    factor: f32,
) {
    let available = queue.len().min(out.len());
    for (dst, sample) in out.iter_mut().zip(queue.drain(..available)) {
        *dst = sample * factor;
    }
    out[available..].fill(0.0);
}
