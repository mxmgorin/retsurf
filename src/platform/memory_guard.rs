//! Watching system memory, so the browser can drop a page before the kernel
//! freezes or kills the device.
//!
//! A thread samples `/proc/meminfo` and PSI four times a second. It trips when RAM
//! plus free swap falls under a floor, or on memory pressure while swapping:
//! on a large zram a board freezes from thrashing before the OOM killer runs.
//! The main loop decides what to close; the thread then stays quiet for [`COOLDOWN`].

use crate::event::user::{UserEvent, UserEventSender};
use std::fmt;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// A page was measured eating 120 MB/s on its way to an OOM kill.
const SAMPLE_INTERVAL: Duration = Duration::from_millis(250);

/// Long enough for PSI's ten-second average to decay after a tab closes.
const COOLDOWN: Duration = Duration::from_secs(15);

/// A fifth of RAM: a 1 GB board crossed a tenth one second before its OOM kill.
const FLOOR_DIVISOR: u64 = 5;

/// On big machines a fifth of RAM is far more headroom than needed.
const FLOOR_MAX_KB: u64 = 512 * 1024;

/// Samples in a row under the floor; one may be an allocation spike.
const LOW_SAMPLES: u32 = 2;

/// PSI `some avg10` percent that counts as thrashing while swapping.
const PRESSURE_THRESHOLD: f32 = 20.0;

/// Why the guard tripped.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Trip {
    /// RAM plus free swap fell under the floor.
    LowMemory { headroom_kb: u64, floor_kb: u64 },
    /// Memory stall share over the last ten seconds, while swapping.
    Pressure { some_avg10: f32 },
}

impl fmt::Display for Trip {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Trip::LowMemory {
                headroom_kb,
                floor_kb,
            } => write!(
                f,
                "{} MB of RAM and swap left, floor {} MB",
                headroom_kb / 1024,
                floor_kb / 1024
            ),
            Trip::Pressure { some_avg10 } => {
                write!(f, "memory pressure {some_avg10:.0}% while swapping")
            }
        }
    }
}

/// One reading of the kernel's memory figures, in KB.
#[derive(Clone, Copy, Debug, Default)]
struct Sample {
    available_kb: u64,
    swap_total_kb: u64,
    swap_free_kb: u64,
    /// `None` without PSI.
    some_avg10: Option<f32>,
}

/// The trip rule, testable without the thread.
struct Judge {
    floor_kb: u64,
    low_samples: u32,
    quiet_until: Option<Instant>,
}

impl Judge {
    /// `floor_mb` of `0` derives the floor from total RAM.
    fn new(total_kb: u64, floor_mb: u32) -> Self {
        let floor_kb = match floor_mb {
            0 => (total_kb / FLOOR_DIVISOR).min(FLOOR_MAX_KB),
            mb => u64::from(mb) * 1024,
        };
        Self {
            floor_kb,
            low_samples: 0,
            quiet_until: None,
        }
    }

    fn assess(&mut self, sample: Sample, now: Instant) -> Option<Trip> {
        if self.quiet_until.is_some_and(|until| now < until) {
            return None;
        }
        self.quiet_until = None;

        let headroom_kb = sample.available_kb + sample.swap_free_kb;
        self.low_samples = if headroom_kb < self.floor_kb {
            self.low_samples + 1
        } else {
            0
        };
        let swapping = sample.swap_total_kb > sample.swap_free_kb;
        let trip = if self.low_samples >= LOW_SAMPLES {
            Some(Trip::LowMemory {
                headroom_kb,
                floor_kb: self.floor_kb,
            })
        } else {
            sample
                .some_avg10
                .filter(|&avg| swapping && avg >= PRESSURE_THRESHOLD)
                .map(|some_avg10| Trip::Pressure { some_avg10 })
        };
        if trip.is_some() {
            self.low_samples = 0;
            self.quiet_until = Some(now + COOLDOWN);
        }
        trip
    }

    fn quiet(&self, now: Instant) -> bool {
        self.quiet_until.is_some_and(|until| now < until)
    }
}

/// The main loop's end of the guard thread.
pub struct MemoryGuard {
    trip: Arc<Mutex<Option<Trip>>>,
}

impl MemoryGuard {
    /// Start watching, or `None` where the kernel's figures cannot be read.
    /// Not on Android, whose low-memory killer owns this decision. `floor_mb` of
    /// `0` derives the floor from total RAM.
    pub fn start(waker: UserEventSender, floor_mb: u32) -> Option<Self> {
        if cfg!(target_os = "android") {
            return None;
        }
        let total_kb = meminfo_kb(&std::fs::read_to_string(MEMINFO).ok()?, "MemTotal:")?;
        let mut judge = Judge::new(total_kb, floor_mb);
        log::info!(
            "memory guard: floor {} MB, pressure {}",
            judge.floor_kb / 1024,
            if read_pressure().is_some() {
                "on"
            } else {
                "unavailable"
            },
        );
        let trip = Arc::new(Mutex::new(None));
        let slot = trip.clone();
        std::thread::Builder::new()
            .name("memory-guard".into())
            .spawn(move || loop {
                std::thread::sleep(SAMPLE_INTERVAL);
                let now = Instant::now();
                // Lets the loop's own deadline fire without another event.
                if judge.quiet(now) {
                    waker.send(UserEvent::BrowserWakeup);
                    continue;
                }
                let Some(sample) = read_sample() else {
                    continue;
                };
                if let Some(found) = judge.assess(sample, now) {
                    // Here too: a busy main loop may never reach its own log line.
                    log::warn!("memory guard: tripped, {found}");
                    if let Ok(mut guard) = slot.lock() {
                        *guard = Some(found);
                    }
                    waker.send(UserEvent::BrowserWakeup);
                }
            })
            .ok()?;
        Some(Self { trip })
    }

    /// The trip raised since the last call, if any.
    pub fn take_trip(&self) -> Option<Trip> {
        self.trip.lock().ok().and_then(|mut guard| guard.take())
    }
}

const MEMINFO: &str = "/proc/meminfo";
const PRESSURE: &str = "/proc/pressure/memory";

fn read_sample() -> Option<Sample> {
    let text = std::fs::read_to_string(MEMINFO).ok()?;
    Some(Sample {
        available_kb: meminfo_kb(&text, "MemAvailable:")?,
        swap_total_kb: meminfo_kb(&text, "SwapTotal:").unwrap_or(0),
        swap_free_kb: meminfo_kb(&text, "SwapFree:").unwrap_or(0),
        some_avg10: read_pressure(),
    })
}

fn read_pressure() -> Option<f32> {
    some_avg10(&std::fs::read_to_string(PRESSURE).ok()?)
}

fn meminfo_kb(text: &str, key: &str) -> Option<u64> {
    text.lines()
        .find_map(|line| line.strip_prefix(key))?
        .split_whitespace()
        .next()?
        .parse()
        .ok()
}

/// `avg10` of the `some` line: `some avg10=1.23 avg60=... total=...`.
fn some_avg10(text: &str) -> Option<f32> {
    text.lines()
        .find_map(|line| line.strip_prefix("some "))?
        .split_whitespace()
        .find_map(|field| field.strip_prefix("avg10="))?
        .parse()
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    const GB_KB: u64 = 1024 * 1024;

    fn sample(
        available_kb: u64,
        swap_total_kb: u64,
        swap_free_kb: u64,
        psi: Option<f32>,
    ) -> Sample {
        Sample {
            available_kb,
            swap_total_kb,
            swap_free_kb,
            some_avg10: psi,
        }
    }

    #[test]
    fn parses_proc_files() {
        let meminfo = "MemTotal:         998244 kB\nMemAvailable:     123456 kB\nSwapFree:  0 kB\n";
        assert_eq!(meminfo_kb(meminfo, "MemTotal:"), Some(998244));
        assert_eq!(meminfo_kb(meminfo, "MemAvailable:"), Some(123456));
        assert_eq!(meminfo_kb(meminfo, "SwapTotal:"), None);
        let psi = "some avg10=27.50 avg60=3.10 avg300=0.80 total=123\nfull avg10=9.00 avg60=1.00 avg300=0.20 total=45\n";
        assert_eq!(some_avg10(psi), Some(27.5));
    }

    #[test]
    fn floor_is_a_fifth_of_ram_capped() {
        assert_eq!(Judge::new(GB_KB, 0).floor_kb, GB_KB / FLOOR_DIVISOR);
        assert_eq!(Judge::new(32 * GB_KB, 0).floor_kb, FLOOR_MAX_KB);
    }

    #[test]
    fn configured_floor_overrides_the_derived_one() {
        assert_eq!(Judge::new(GB_KB, 300).floor_kb, 300 * 1024);
    }

    /// One sample under the floor is a spike; a second in a row trips.
    #[test]
    fn low_memory_needs_consecutive_samples() {
        let mut judge = Judge::new(GB_KB, 0);
        let now = Instant::now();
        let low = sample(50 * 1024, 0, 0, None);
        let fine = sample(500 * 1024, 0, 0, None);
        assert_eq!(judge.assess(low, now), None);
        assert_eq!(judge.assess(fine, now), None);
        assert_eq!(judge.assess(low, now), None);
        assert!(matches!(
            judge.assess(low, now),
            Some(Trip::LowMemory { .. })
        ));
    }

    /// Free swap is headroom: a board browsing on swap by design is not short.
    #[test]
    fn free_swap_counts_as_headroom() {
        let mut judge = Judge::new(GB_KB, 0);
        let now = Instant::now();
        let on_swap = sample(20 * 1024, 512 * 1024, 400 * 1024, None);
        assert_eq!(judge.assess(on_swap, now), None);
        assert_eq!(judge.assess(on_swap, now), None);
    }

    /// Pressure alone trips only once the device is swapping.
    #[test]
    fn pressure_trips_only_while_swapping() {
        let mut judge = Judge::new(GB_KB, 0);
        let now = Instant::now();
        let no_swap_used = sample(500 * 1024, 256 * 1024, 256 * 1024, Some(40.0));
        assert_eq!(judge.assess(no_swap_used, now), None);
        let swapping = sample(500 * 1024, 256 * 1024, 100 * 1024, Some(40.0));
        assert_eq!(
            judge.assess(swapping, now),
            Some(Trip::Pressure { some_avg10: 40.0 })
        );
    }

    #[test]
    fn stays_quiet_through_the_cooldown() {
        let mut judge = Judge::new(GB_KB, 0);
        let now = Instant::now();
        let swapping = sample(500 * 1024, 256 * 1024, 100 * 1024, Some(40.0));
        assert!(judge.assess(swapping, now).is_some());
        assert_eq!(judge.assess(swapping, now + COOLDOWN / 2), None);
        assert!(judge.assess(swapping, now + COOLDOWN).is_some());
    }
}
