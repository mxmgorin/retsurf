//! Raising the CPU governor around page loads. Measured on a Miyoo Flip:
//! `performance` is worth -16% page time, but the win is in the load bursts
//! where `ondemand` ramps too late — holding it all session just burns battery.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// The governor to hold while loading.
const BOOSTED: &str = "performance";

/// How long the boost outlives the load, so the paint that follows it is covered.
const LINGER: Duration = Duration::from_millis(500);

/// What each governor file held before the boost. Global so the panic hook can
/// put it back: this is a machine-wide setting, and leaving it raised costs
/// battery until the next reboot.
static SAVED: Mutex<Vec<(PathBuf, String)>> = Mutex::new(Vec::new());

/// Put every boosted governor back, but only where ours is still in force: muOS
/// switches to `powersave` on idle, and a value saved before that decision must
/// not overrule it. Idempotent, and safe when nothing was raised.
pub fn restore() {
    let Ok(mut saved) = SAVED.lock() else {
        return;
    };
    for (path, governor) in saved.drain(..) {
        if read_trimmed(&path).as_deref() == Some(BOOSTED) {
            let _ = std::fs::write(&path, &governor);
        }
    }
}

/// Holds `performance` while a page loads. Inert unless `[performance]
/// cpu_boost_on_load` is on and a writable governor offers it — root in
/// practice, which is every handheld launcher and no desktop.
pub struct LoadBoost {
    enabled: bool,
    /// One `scaling_governor` per policy; empty when the knob is off or there is
    /// no cpufreq we can drive.
    governors: Vec<PathBuf>,
    /// When the boost may be dropped. `Some` while loading and through the linger.
    hold_until: Option<Instant>,
}

impl LoadBoost {
    pub fn new(enabled: bool) -> Self {
        let mut boost = Self {
            enabled: false,
            governors: Vec::new(),
            hold_until: None,
        };
        boost.set_enabled(enabled);
        boost
    }

    /// Adopt an edited `cpu_boost_on_load`, dropping a live boost when it goes off.
    pub fn set_enabled(&mut self, enabled: bool) {
        if enabled == self.enabled {
            return;
        }
        self.enabled = enabled;
        if !enabled {
            self.hold_until = None;
            self.governors.clear();
            restore();
            return;
        }
        self.governors = governor_files();
        match self.governors.first() {
            Some(path) => log::info!(
                "cpu boost on load: {} cpufreq policies, now `{}`",
                self.governors.len(),
                read_trimmed(path).unwrap_or_default(),
            ),
            None => log::info!("cpu boost on load: no cpufreq governor offers `{BOOSTED}`"),
        }
    }

    /// Raise while `loading`, drop once it has been clear for [`LINGER`]. Returns
    /// whether the boost is still held, so the caller can keep the loop awake —
    /// otherwise an idle wait would hold the clock up until the next input.
    pub fn follow(&mut self, loading: bool) -> bool {
        if self.governors.is_empty() {
            return false;
        }
        if loading {
            if self.hold_until.is_none() && !self.raise() {
                // Refused (no root) or already there: go inert rather than
                // retry a write and hold a repaint on every load.
                log::info!("cpu boost on load: nothing to raise, leaving the governor alone");
                self.governors.clear();
                return false;
            }
            self.hold_until = Some(Instant::now() + LINGER);
        } else if self.hold_until.is_some_and(|at| at <= Instant::now()) {
            self.hold_until = None;
            restore();
        }
        self.hold_until.is_some()
    }

    /// Whether the boost is now in force, counting policies that were already
    /// there. `false` means the write was refused.
    fn raise(&self) -> bool {
        let Ok(mut saved) = SAVED.lock() else {
            return false;
        };
        if !saved.is_empty() {
            return true;
        }
        let mut held = 0;
        for path in &self.governors {
            let Some(before) = read_trimmed(path) else {
                continue;
            };
            if before == BOOSTED {
                held += 1;
            } else if std::fs::write(path, BOOSTED).is_ok() {
                saved.push((path.clone(), before));
                held += 1;
            }
        }
        held > 0
    }
}

impl Drop for LoadBoost {
    fn drop(&mut self) {
        restore();
    }
}

const CPU_ROOT: &str = "/sys/devices/system/cpu";

/// Every `scaling_governor` that offers `performance`. Prefers the `policy*`
/// layout — one file per policy, and cores sharing a policy share the file — and
/// falls back to the per-cpu one for drivers that expose only that.
fn governor_files() -> Vec<PathBuf> {
    governor_files_in(Path::new(CPU_ROOT))
}

fn governor_files_in(cpu_root: &Path) -> Vec<PathBuf> {
    let policies = offering_performance(dirs(&cpu_root.join("cpufreq")));
    if !policies.is_empty() {
        return policies;
    }
    offering_performance(
        dirs(cpu_root)
            .into_iter()
            .map(|cpu| cpu.join("cpufreq"))
            .collect(),
    )
}

/// Skips the governor tunable dirs (`cpufreq/ondemand`), which carry no
/// `scaling_available_governors`.
fn offering_performance(candidates: Vec<PathBuf>) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = candidates
        .into_iter()
        .filter(|dir| {
            read_trimmed(&dir.join("scaling_available_governors"))
                .is_some_and(|list| list.split_whitespace().any(|g| g == BOOSTED))
        })
        .map(|dir| dir.join("scaling_governor"))
        .collect();
    // `read_dir` order is arbitrary; a stable list keeps the log readable.
    found.sort();
    found
}

fn dirs(path: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(path)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect()
}

fn read_trimmed(path: &Path) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy(dir: &Path, governors: &str) {
        std::fs::create_dir_all(dir).unwrap();
        std::fs::write(dir.join("scaling_available_governors"), governors).unwrap();
        std::fs::write(dir.join("scaling_governor"), "ondemand\n").unwrap();
    }

    fn tmp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("retsurf-cpufreq-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    /// The `policy*` layout wins, and `cpufreq/ondemand` is not a policy.
    #[test]
    fn prefers_policies_and_skips_tunable_dirs() {
        let root = tmp("policies");
        policy(&root.join("cpufreq/policy0"), "ondemand performance\n");
        policy(&root.join("cpufreq/policy1"), "ondemand performance\n");
        std::fs::create_dir_all(root.join("cpufreq/ondemand")).unwrap();
        policy(&root.join("cpu0/cpufreq"), "ondemand performance\n");

        assert_eq!(
            governor_files_in(&root),
            vec![
                root.join("cpufreq/policy0/scaling_governor"),
                root.join("cpufreq/policy1/scaling_governor"),
            ]
        );
    }

    /// Drivers that expose only the per-cpu layout still get driven.
    #[test]
    fn falls_back_to_the_per_cpu_layout() {
        let root = tmp("percpu");
        policy(&root.join("cpu0/cpufreq"), "ondemand performance\n");
        std::fs::create_dir_all(root.join("cpufreq/ondemand")).unwrap();

        assert_eq!(
            governor_files_in(&root),
            vec![root.join("cpu0/cpufreq/scaling_governor")]
        );
    }

    /// Restoring must not overrule an OS that moved the governor itself while
    /// the boost was up. Both cases in one test: `SAVED` is global.
    #[test]
    fn restores_only_what_is_still_ours() {
        let dir = tmp("restore");
        std::fs::create_dir_all(&dir).unwrap();
        let ours = dir.join("ours");
        let taken = dir.join("taken");
        std::fs::write(&ours, "performance\n").unwrap();
        std::fs::write(&taken, "powersave\n").unwrap();

        SAVED.lock().unwrap().extend([
            (ours.clone(), "ondemand".to_owned()),
            (taken.clone(), "ondemand".to_owned()),
        ]);
        restore();

        assert_eq!(read_trimmed(&ours).unwrap(), "ondemand");
        assert_eq!(read_trimmed(&taken).unwrap(), "powersave");
        assert!(SAVED.lock().unwrap().is_empty());
    }

    /// A kernel without `performance` is left alone rather than written to.
    #[test]
    fn a_kernel_without_performance_offers_nothing() {
        let root = tmp("nogov");
        policy(&root.join("cpufreq/policy0"), "powersave schedutil\n");

        assert!(governor_files_in(&root).is_empty());
    }
}
