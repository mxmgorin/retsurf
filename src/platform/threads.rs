//! Per-thread cost from `/proc/self/task` for `[debug] thread_cpu` — wall clock
//! alone cannot tell work a frame did from time it spent waiting.

use std::collections::HashMap;
use std::iter::Sum;
use std::ops::AddAssign;
use std::time::{Duration, Instant};

/// How often the per-thread deltas reach the log.
const REPORT_INTERVAL: Duration = Duration::from_secs(5);

/// Families under this much CPU in an interval are folded into one `other` row.
const NOISE_FLOOR: Duration = Duration::from_millis(2);

/// What a thread family cost over an interval or a run.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
struct Cost {
    cpu: Duration,
    /// Waiting on the card is the other way wall clock goes without CPU.
    major_faults: u64,
}

impl AddAssign for Cost {
    fn add_assign(&mut self, other: Self) {
        self.cpu += other.cpu;
        self.major_faults += other.major_faults;
    }
}

impl Sum for Cost {
    fn sum<I: Iterator<Item = Self>>(iter: I) -> Self {
        iter.fold(Self::default(), |mut acc, c| {
            acc += c;
            acc
        })
    }
}

impl Cost {
    /// Saturating: a reused tid reads as a counter going backwards.
    fn since(self, before: Self) -> Self {
        Self {
            cpu: self.cpu.saturating_sub(before.cpu),
            major_faults: self.major_faults.saturating_sub(before.major_faults),
        }
    }
}

/// Rolling per-thread cost; inert unless `[debug] thread_cpu` is on.
pub struct ThreadCpu {
    enabled: bool,
    since: Instant,
    run_started: Instant,
    /// Keyed by tid: threads come and go, and a delta needs the last absolute.
    last: HashMap<u32, Cost>,
    /// Cumulative cost per family over the whole run.
    total: HashMap<String, Cost>,
}

impl ThreadCpu {
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            since: Instant::now(),
            run_started: Instant::now(),
            last: HashMap::new(),
            total: HashMap::new(),
        }
    }

    /// Report the interval just ended. Driven from the main loop, so an idle
    /// browser reports late rather than often — hence measuring `wall`.
    pub fn tick(&mut self) {
        if !self.enabled || self.since.elapsed() < REPORT_INTERVAL {
            return;
        }
        let wall = self.since.elapsed();
        let deltas = self.sample();
        let (rows, sum) = format_families(&deltas, NOISE_FLOOR);
        log::info!(
            "thread cpu: {rows} | sum {:.1} ms over {:.1} s wall, {:.2} cores busy{}",
            ms(sum.cpu),
            wall.as_secs_f32(),
            sum.cpu.as_secs_f32() / wall.as_secs_f32(),
            format_faults(&deltas, sum),
        );
    }

    /// Run totals, folding nothing away: the line an A/B reads.
    pub fn report_run(&mut self) {
        if !self.enabled {
            return;
        }
        self.sample();
        let (rows, sum) = format_families(&self.total, Duration::ZERO);
        log::info!(
            "thread cpu total: {rows} | sum {:.1} ms over {:.1} s wall{}",
            ms(sum.cpu),
            self.run_started.elapsed().as_secs_f32(),
            format_faults(&self.total, sum),
        );
    }

    /// Read every live thread and fold the deltas into the run totals.
    fn sample(&mut self) -> HashMap<String, Cost> {
        self.since = Instant::now();
        let mut deltas: HashMap<String, Cost> = HashMap::new();
        let mut sampled = HashMap::with_capacity(self.last.len());
        for thread in proc_fs::threads() {
            let before = self.last.get(&thread.tid).copied().unwrap_or_default();
            sampled.insert(thread.tid, thread.cost);
            *deltas.entry(family(&thread.name)).or_default() += thread.cost.since(before);
        }
        self.last = sampled;
        for (name, delta) in &deltas {
            *self.total.entry(name.clone()).or_default() += *delta;
        }
        deltas
    }
}

/// Fold pools (`StyleThread#3`, `WRWorker#0`) into one row: cut at the `#`/`:`
/// that introduces the member, keeping it so the row reads as a family.
fn family(name: &str) -> String {
    let cut = name.find(['#', ':']).map_or(name.len(), |i| i + 1);
    name[..cut].to_owned()
}

/// `name ms` pairs, largest first, sub-`floor` ones folded into `other`.
/// The returned sum covers every family, folded included.
fn format_families(families: &HashMap<String, Cost>, floor: Duration) -> (String, Cost) {
    let mut rows: Vec<_> = families
        .iter()
        .filter(|(_, cost)| cost.cpu >= floor)
        .map(|(name, cost)| (name.as_str(), *cost))
        .collect();
    rows.sort_unstable_by(|a, b| b.1.cpu.cmp(&a.1.cpu).then(a.0.cmp(b.0)));

    let sum: Cost = families.values().copied().sum();
    let shown: Cost = rows.iter().map(|(_, cost)| *cost).sum();
    let mut out: Vec<String> = rows
        .iter()
        .map(|(name, cost)| format!("{name} {:.1}", ms(cost.cpu)))
        .collect();
    let other = sum.since(shown);
    if other != Cost::default() {
        out.push(format!("other {:.1}", ms(other.cpu)));
    }
    (out.join(", "), sum)
}

/// Only emitted when pages were read from disk; the line is long enough.
fn format_faults(families: &HashMap<String, Cost>, sum: Cost) -> String {
    if sum.major_faults == 0 {
        return String::new();
    }
    let mut rows: Vec<_> = families
        .iter()
        .filter(|(_, cost)| cost.major_faults > 0)
        .collect();
    rows.sort_unstable_by(|a, b| b.1.major_faults.cmp(&a.1.major_faults).then(a.0.cmp(b.0)));
    let listed: Vec<String> = rows
        .iter()
        .map(|(name, cost)| format!("{name} {}", cost.major_faults))
        .collect();
    format!(
        " | major faults {}: {}",
        sum.major_faults,
        listed.join(", ")
    )
}

fn ms(duration: Duration) -> f32 {
    duration.as_secs_f32() * 1000.0
}

#[cfg(target_os = "linux")]
mod priority {
    use std::io;
    use std::os::raw::c_int;

    /// `PRIO_PROCESS` from `sys/resource.h`. Linux keeps nice per task, so
    /// `who = 0` moves the calling thread alone.
    const PRIO_PROCESS: c_int = 0;
    const CALLING_THREAD: u32 = 0;

    pub fn set_nice(nice: i32) -> io::Result<()> {
        // Unlike `getpriority`, -1 here is only ever the error return.
        match unsafe { setpriority(PRIO_PROCESS, CALLING_THREAD, nice) } {
            0 => Ok(()),
            _ => Err(io::Error::last_os_error()),
        }
    }

    extern "C" {
        fn setpriority(which: c_int, who: u32, prio: c_int) -> c_int;
    }
}

#[cfg(not(target_os = "linux"))]
mod priority {
    use std::io;

    pub fn set_nice(_: i32) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "no thread priority on this platform",
        ))
    }
}

#[cfg(target_os = "linux")]
mod proc_fs {
    use super::Cost;
    use std::os::raw::{c_int, c_long};
    use std::sync::OnceLock;
    use std::time::Duration;

    /// A live thread's `comm` (kernel-capped at 15 chars) and its cost so far.
    pub struct Thread {
        pub tid: u32,
        pub name: String,
        pub cost: Cost,
    }

    pub fn threads() -> Vec<Thread> {
        let Ok(dir) = std::fs::read_dir("/proc/self/task") else {
            return Vec::new();
        };
        dir.flatten()
            .filter_map(|entry| {
                let tid = entry.file_name().to_str()?.parse().ok()?;
                let stat = std::fs::read_to_string(entry.path().join("stat")).ok()?;
                parse(tid, &stat)
            })
            .collect()
    }

    /// Offsets from `state`, the first field after `comm`; stime follows utime.
    const MAJFLT_AFTER_COMM: usize = 9;
    const UTIME_AFTER_MAJFLT: usize = 1;

    /// `comm` may hold spaces and parens, so cut at the last `)`.
    fn parse(tid: u32, stat: &str) -> Option<Thread> {
        let open = stat.find('(')?;
        let close = stat.rfind(')')?;
        let name = stat.get(open + 1..close)?.to_owned();
        let mut fields = stat.get(close + 2..)?.split(' ');
        let major_faults: u64 = fields.nth(MAJFLT_AFTER_COMM)?.parse().ok()?;
        let utime: u64 = fields.nth(UTIME_AFTER_MAJFLT)?.parse().ok()?;
        let stime: u64 = fields.next()?.parse().ok()?;
        Some(Thread {
            tid,
            name,
            cost: Cost {
                cpu: from_ticks(utime + stime),
                major_faults,
            },
        })
    }

    /// `_SC_CLK_TCK` from `unistd.h`.
    const SC_CLK_TCK: c_int = 2;

    /// USER_HZ everywhere we ship, for a `sysconf` that failed.
    const DEFAULT_HZ: u64 = 100;

    const NANOS_PER_SEC: u64 = 1_000_000_000;

    /// `/proc` reports CPU time in clock ticks, not seconds.
    fn from_ticks(ticks: u64) -> Duration {
        Duration::from_nanos(ticks.saturating_mul(NANOS_PER_SEC) / hz())
    }

    fn hz() -> u64 {
        static HZ: OnceLock<u64> = OnceLock::new();
        *HZ.get_or_init(|| match unsafe { sysconf(SC_CLK_TCK) } {
            value if value > 0 => value as u64,
            _ => DEFAULT_HZ,
        })
    }

    extern "C" {
        fn sysconf(name: c_int) -> c_long;
    }
}

#[cfg(not(target_os = "linux"))]
mod proc_fs {
    use super::Cost;

    pub struct Thread {
        pub tid: u32,
        pub name: String,
        pub cost: Cost,
    }

    pub fn threads() -> Vec<Thread> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cpu(millis: u64) -> Cost {
        Cost {
            cpu: Duration::from_millis(millis),
            major_faults: 0,
        }
    }

    #[test]
    fn pool_members_fold_into_one_family() {
        assert_eq!(family("StyleThread#3"), "StyleThread#");
        assert_eq!(family("Parse:example.com"), "Parse:");
        assert_eq!(family("Constellation"), "Constellation");
    }

    /// The sum covers folded families too, or an A/B reads low.
    #[test]
    fn sub_floor_families_fold_into_other() {
        let families = HashMap::from([
            ("Script#".to_owned(), cpu(100)),
            ("Devtools".to_owned(), cpu(1)),
        ]);
        let (rows, sum) = format_families(&families, NOISE_FLOOR);

        assert_eq!(rows, "Script# 100.0, other 1.0");
        assert_eq!(sum.cpu, Duration::from_millis(101));
    }

    /// Silent with RAM to spare; names the threads that waited otherwise.
    #[test]
    fn faults_are_reported_only_when_there_are_any() {
        let quiet = HashMap::from([("Script#".to_owned(), cpu(100))]);
        assert_eq!(format_faults(&quiet, cpu(100)), "");

        let faulting = HashMap::from([
            (
                "Script#".to_owned(),
                Cost {
                    cpu: Duration::from_millis(100),
                    major_faults: 40,
                },
            ),
            ("Devtools".to_owned(), cpu(1)),
        ]);
        let sum = faulting.values().copied().sum();
        assert_eq!(
            format_faults(&faulting, sum),
            " | major faults 40: Script# 40"
        );
    }
}
