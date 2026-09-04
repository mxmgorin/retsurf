use std::collections::VecDeque;
use std::ffi::{c_int, c_void};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Condvar, Mutex, MutexGuard};
use std::time::Instant;

use servo_base::generic_channel::GenericCallback;
use servo_media::player::PlayerEvent;

use crate::media::device::CHANNELS;

/// `push_data` refuses bytes past this much buffered input; the element cancels
/// the fetch and the decoder requests a refetch when it runs dry. Bounds what a
/// parked page can pin no matter how large the file is.
pub(super) const BYTE_HIGH_WATER: usize = 16 * 1024 * 1024;

/// Already-read bytes kept behind the cursor so small backward seeks stay local
/// instead of costing a refetch round-trip.
const KEEP_BACK_BYTES: usize = 4 * 1024 * 1024;

/// `pending_seek` sentinel for "none"; a real seek target is never NaN because
/// the element clamps it into the seekable ranges first.
const NO_SEEK: u64 = u64::MAX;

/// Poison-tolerant lock: the SDL callback must not unwind across FFI, and a
/// panicked decoder thread must not take the script thread down with it.
pub(super) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub(super) fn wait<'a, T>(cv: &Condvar, guard: MutexGuard<'a, T>) -> MutexGuard<'a, T> {
    cv.wait(guard)
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Where `PlayerEvent`s go; a trait so tests can collect them.
pub(super) trait EventSink: Send + Sync {
    fn send(&self, event: PlayerEvent);
}

/// The element-side callback queues a task per event; failures mean the element
/// is gone, which the quit flag handles.
pub(super) struct CallbackSink(pub(super) Mutex<GenericCallback<PlayerEvent>>);

impl EventSink for CallbackSink {
    fn send(&self, event: PlayerEvent) {
        let _ = lock(&self.0).send(event);
    }
}

/// Raw response bytes, one contiguous region `[base, base + data.len())` of the
/// resource: fetches are linear and every refetch resets the region.
pub(super) struct StreamBuf {
    pub(super) base: u64,
    pub(super) data: Vec<u8>,
    /// Absolute read cursor; may point outside the region until `read` resolves it.
    pub(super) read_pos: u64,
    pub(super) total_len: Option<u64>,
    /// The response answered 206, i.e. the server honors `Range`.
    pub(super) response_seekable: bool,
    pub(super) eos: bool,
    /// The fetch died from our `EnoughData` error; reads at the head must refetch.
    pub(super) stalled: bool,
}

impl StreamBuf {
    pub(super) fn head(&self) -> u64 {
        self.base + self.data.len() as u64
    }

    /// Drops read bytes beyond the keep-back window.
    pub(super) fn evict(&mut self) {
        let consumed = (self.read_pos - self.base) as usize;
        if consumed > KEEP_BACK_BYTES {
            let n = consumed - KEEP_BACK_BYTES;
            self.data.drain(..n);
            self.base += n as u64;
        }
    }
}

/// Decoded interleaved-stereo PCM waiting for the device.
pub(super) struct Pcm {
    pub(super) queue: VecDeque<f32>,
    /// Frames pushed since `base_secs`; played = pushed - queued.
    pub(super) pushed_frames: u64,
    /// Media time of the first frame pushed after the last seek.
    pub(super) base_secs: f64,
    /// Media time of the newest decoded sample; `buffered()`'s fallback.
    pub(super) decoded_secs: f64,
    pub(super) last_emitted_pos: f64,
}

pub(super) struct MetaState {
    pub(super) duration_secs: Option<f64>,
    pub(super) sample_rate: Option<u32>,
}

/// Media clock: audio position when an audio track exists, a pause-aware
/// wallclock otherwise (muted video-only files).
pub(super) struct Clock {
    pub(super) audio_rate: Option<u32>,
    /// Set while playing without audio.
    pub(super) anchor: Option<Instant>,
    pub(super) at: f64,
}

pub(crate) struct Shared {
    pub(super) stream: Mutex<StreamBuf>,
    /// Wakes a read blocked on more bytes (`push_data`, `end_of_stream`, quit, seek).
    pub(super) bytes_cv: Condvar,
    pub(super) pcm: Mutex<Pcm>,
    /// Wakes the decoder: SDL drained the queue, or play/seek/stop changed state.
    pub(super) work_cv: Condvar,
    pub(super) meta: Mutex<MetaState>,
    /// f64 bits of the seek target; [`NO_SEEK`] when none. Latest seek wins.
    pub(super) pending_seek: AtomicU64,
    /// Honor `pending_seek` in reads only once the probe is done — aborting a
    /// probe-time read would kill the player before it ever produced metadata.
    pub(super) probed: AtomicBool,
    pub(super) quit: AtomicBool,
    pub(super) paused: AtomicBool,
    pub(super) muted: AtomicBool,
    /// `set_audio_track(0, enabled)`; we always report exactly one track.
    pub(super) track_enabled: AtomicBool,
    /// `set_video_track(0, enabled)`: off skips the paint, decode continues.
    pub(super) video_track_enabled: AtomicBool,
    /// f64 bits; applied per-sample in the SDL callback.
    pub(super) volume: AtomicU64,
    pub(super) clock: Mutex<Clock>,
    pub(super) events: Box<dyn EventSink>,
}

impl Shared {
    pub(super) fn new(events: Box<dyn EventSink>) -> Self {
        Self {
            stream: Mutex::new(StreamBuf {
                base: 0,
                data: Vec::new(),
                read_pos: 0,
                total_len: None,
                response_seekable: false,
                eos: false,
                stalled: false,
            }),
            bytes_cv: Condvar::new(),
            pcm: Mutex::new(Pcm {
                queue: VecDeque::new(),
                pushed_frames: 0,
                base_secs: 0.0,
                decoded_secs: 0.0,
                last_emitted_pos: 0.0,
            }),
            work_cv: Condvar::new(),
            meta: Mutex::new(MetaState {
                duration_secs: None,
                sample_rate: None,
            }),
            pending_seek: AtomicU64::new(NO_SEEK),
            probed: AtomicBool::new(false),
            quit: AtomicBool::new(false),
            paused: AtomicBool::new(true),
            muted: AtomicBool::new(false),
            track_enabled: AtomicBool::new(true),
            video_track_enabled: AtomicBool::new(true),
            volume: AtomicU64::new(1f64.to_bits()),
            clock: Mutex::new(Clock {
                audio_rate: None,
                anchor: None,
                at: 0.0,
            }),
            events,
        }
    }

    pub(crate) fn seek_pending(&self) -> bool {
        self.probed.load(Ordering::SeqCst) && self.pending_seek.load(Ordering::SeqCst) != NO_SEEK
    }

    pub(super) fn take_pending_seek(&self) -> Option<f64> {
        let bits = self.pending_seek.swap(NO_SEEK, Ordering::SeqCst);
        (bits != NO_SEEK).then(|| f64::from_bits(bits))
    }

    pub(super) fn wake_all(&self) {
        self.bytes_cv.notify_all();
        self.work_cv.notify_all();
    }

    pub(crate) fn is_quit(&self) -> bool {
        self.quit.load(Ordering::SeqCst)
    }

    pub(crate) fn is_paused(&self) -> bool {
        self.paused.load(Ordering::SeqCst)
    }

    pub(crate) fn has_audio(&self) -> bool {
        lock(&self.clock).audio_rate.is_some()
    }

    pub(crate) fn video_track_enabled(&self) -> bool {
        self.video_track_enabled.load(Ordering::Relaxed)
    }

    pub(crate) fn send_event(&self, event: PlayerEvent) {
        self.events.send(event);
    }

    pub(crate) fn notify_work(&self) {
        self.work_cv.notify_all();
    }

    /// Current media time. Lock order: clock before pcm, never the reverse.
    pub(crate) fn clock_secs(&self) -> f64 {
        let clock = lock(&self.clock);
        if let Some(rate) = clock.audio_rate {
            let pcm = lock(&self.pcm);
            let played = pcm
                .pushed_frames
                .saturating_sub(pcm.queue.len() as u64 / u64::from(CHANNELS));
            return pcm.base_secs + played as f64 / f64::from(rate);
        }
        match clock.anchor {
            Some(anchor) => clock.at + anchor.elapsed().as_secs_f64(),
            None => clock.at,
        }
    }

    /// Freeze the wallclock (pause/stop); no-op under an audio clock.
    pub(super) fn clock_freeze(&self) {
        let mut clock = lock(&self.clock);
        if let Some(anchor) = clock.anchor.take() {
            clock.at += anchor.elapsed().as_secs_f64();
        }
    }

    /// Resume the wallclock; the audio clock resumes by itself.
    pub(super) fn clock_run(&self) {
        let mut clock = lock(&self.clock);
        if clock.audio_rate.is_none() && clock.anchor.is_none() {
            clock.anchor = Some(Instant::now());
        }
    }

    /// Jump the wallclock to a seek target, keeping its run state.
    pub(super) fn clock_jump(&self, at: f64) {
        let mut clock = lock(&self.clock);
        clock.at = at;
        if clock.anchor.is_some() {
            clock.anchor = Some(Instant::now());
        }
    }
}

/// SDL's audio thread: drain the PCM queue scaled by volume, zero-fill the rest,
/// and wake the decoder to refill.
///
/// # Safety
///
/// `userdata` is the `Arc<Shared>` the decoder thread holds until after it closed
/// the device, so it is live for every call.
pub(super) unsafe extern "C" fn player_callback(
    userdata: *mut c_void,
    stream: *mut u8,
    len: c_int,
) {
    let shared = unsafe { &*(userdata as *const Shared) };
    let out = unsafe {
        std::slice::from_raw_parts_mut(stream as *mut f32, len as usize / size_of::<f32>())
    };

    let silent =
        shared.muted.load(Ordering::Relaxed) || !shared.track_enabled.load(Ordering::Relaxed);
    let factor = if silent {
        0.0
    } else {
        f64::from_bits(shared.volume.load(Ordering::Relaxed)) as f32
    };

    let mut pcm = lock(&shared.pcm);
    let available = pcm.queue.len().min(out.len());
    for (dst, sample) in out.iter_mut().zip(pcm.queue.drain(..available)) {
        *dst = sample * factor;
    }
    out[available..].fill(0.0);
    drop(pcm);

    // Every tick, not just below a low-water mark: the decoder also paces
    // PositionChanged events off these wakes.
    shared.work_cv.notify_all();
}
