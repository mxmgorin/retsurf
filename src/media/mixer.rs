//! Shared playback: one SDL device per sample rate, every source at that rate summed
//! into it.
//!
//! SDL2 caps open devices at 16, fewer than the media a page can hold. Grouping by
//! rate leaves sources unresampled, so a source's queue counts frames at its own rate.
//!
//! A bus opens with its first attachment, closes with its last, and runs only while
//! some attachment is active.
//!
//! Lock order: buses, then a bus's control, sources, device, then a source's own
//! locks. SDL's callback takes only sources and below, so the SDL calls that wait
//! for it (pause, close) are never made under the sources lock.

use std::collections::VecDeque;
use std::ffi::{c_int, c_void};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use super::device::{lock, out_slice, Device};

/// Audio that can be summed into a shared device buffer.
pub(crate) trait MixSource: Send + Sync {
    /// Adds the next `out.len()` interleaved stereo samples into `out`, fewer if
    /// that is all it has. Runs on SDL's audio thread, so it must not block long.
    fn mix_into(&self, out: &mut [f32]);
}

/// Adds up to `out.len()` samples drained from `queue` into `out`, scaled by
/// `factor`; past what the queue held, `out` is left as it was.
pub(crate) fn mix_from(out: &mut [f32], queue: &mut VecDeque<f32>, factor: f32) {
    let available = queue.len().min(out.len());
    for (dst, sample) in out.iter_mut().zip(queue.drain(..available)) {
        *dst += sample * factor;
    }
}

struct Entry {
    source: Arc<dyn MixSource>,
    active: Arc<AtomicBool>,
}

/// One device and the sources playing through it.
struct Bus {
    rate: c_int,
    /// Serializes pause decisions with the SDL calls that apply them.
    control: Mutex<()>,
    sources: Mutex<Vec<Entry>>,
    /// `None` only while the bus opens and after it closed.
    device: Mutex<Option<Device>>,
}

impl Bus {
    fn update_pause(&self) {
        let _control = lock(&self.control);
        let any_active = lock(&self.sources)
            .iter()
            .any(|entry| entry.active.load(Ordering::SeqCst));
        if let Some(device) = lock(&self.device).as_ref() {
            device.set_paused(!any_active);
        }
    }
}

static BUSES: Mutex<Vec<Arc<Bus>>> = Mutex::new(Vec::new());

/// A source's place on its rate's bus; dropping it detaches the source.
pub(crate) struct Attachment {
    bus: Arc<Bus>,
    active: Arc<AtomicBool>,
}

impl Attachment {
    /// Whether the source is mixed; an inactive one is not drained either.
    pub(crate) fn set_active(&self, active: bool) {
        if self.active.swap(active, Ordering::SeqCst) != active {
            self.bus.update_pause();
        }
    }
}

impl Drop for Attachment {
    fn drop(&mut self) {
        let mut buses = lock(&BUSES);
        let empty = {
            let mut sources = lock(&self.bus.sources);
            sources.retain(|entry| !Arc::ptr_eq(&entry.active, &self.active));
            sources.is_empty()
        };
        if empty {
            buses.retain(|bus| !Arc::ptr_eq(bus, &self.bus));
            // Close (joins SDL's audio thread) before the bus can be freed: SDL
            // holds its address as the callback's userdata.
            let device = lock(&self.bus.device).take();
            drop(device);
        } else {
            drop(buses);
            self.bus.update_pause();
        }
    }
}

/// Attaches `source`, inactive, to the bus for `sample_rate`, opening that bus's
/// device if nothing plays at that rate yet.
pub(crate) fn attach(sample_rate: f32, source: Arc<dyn MixSource>) -> Result<Attachment, String> {
    let rate = sample_rate as c_int;
    let mut buses = lock(&BUSES);
    let bus = match buses.iter().find(|bus| bus.rate == rate) {
        Some(bus) => bus.clone(),
        None => {
            let bus = Arc::new(Bus {
                rate,
                control: Mutex::new(()),
                sources: Mutex::new(Vec::new()),
                device: Mutex::new(None),
            });
            let userdata = Arc::as_ptr(&bus) as *mut c_void;
            *lock(&bus.device) = Some(Device::open(sample_rate, bus_callback, userdata)?);
            buses.push(bus.clone());
            bus
        }
    };
    let active = Arc::new(AtomicBool::new(false));
    lock(&bus.sources).push(Entry {
        source,
        active: active.clone(),
    });
    Ok(Attachment { bus, active })
}

/// SDL's audio thread for one bus. Safety: `userdata` is the [`Bus`] that owns the
/// device, live until that device closed.
unsafe extern "C" fn bus_callback(userdata: *mut c_void, stream: *mut u8, len: c_int) {
    let bus = unsafe { &*(userdata as *const Bus) };
    let out = unsafe { out_slice(stream, len) };
    mix(&lock(&bus.sources), out);
}

/// Sums the active sources into `out`, clamped: overlapping sources can exceed full
/// scale, and a float device passes that on.
fn mix(sources: &[Entry], out: &mut [f32]) {
    out.fill(0.0);
    for entry in sources {
        if entry.active.load(Ordering::Relaxed) {
            entry.source.mix_into(out);
        }
    }
    for sample in out {
        *sample = sample.clamp(-1.0, 1.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Queue(Mutex<VecDeque<f32>>);

    impl MixSource for Queue {
        fn mix_into(&self, out: &mut [f32]) {
            mix_from(out, &mut lock(&self.0), 1.0);
        }
    }

    fn entry(samples: &[f32], active: bool) -> (Entry, Arc<Queue>) {
        let queue = Arc::new(Queue(Mutex::new(samples.iter().copied().collect())));
        let entry = Entry {
            source: queue.clone(),
            active: Arc::new(AtomicBool::new(active)),
        };
        (entry, queue)
    }

    /// Active sources add up, a short one leaves silence behind it, and an inactive
    /// one keeps its samples for when it plays.
    #[test]
    fn sums_active_sources_only() {
        let (a, _) = entry(&[0.25, 0.25, 0.25, 0.25], true);
        let (b, _) = entry(&[0.5, 0.5], true);
        let (paused, paused_queue) = entry(&[0.125; 4], false);
        let mut out = [9.0; 4];

        mix(&[a, b, paused], &mut out);

        assert_eq!(out, [0.75, 0.75, 0.25, 0.25]);
        assert_eq!(lock(&paused_queue.0).len(), 4);
    }

    #[test]
    fn clamps_the_sum_to_full_scale() {
        let (a, _) = entry(&[0.75, -0.75], true);
        let (b, _) = entry(&[0.75, -0.75], true);
        let mut out = [0.0; 2];

        mix(&[a, b], &mut out);

        assert_eq!(out, [1.0, -1.0]);
    }
}
