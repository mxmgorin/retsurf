//! Swap tuning for small boards, held by a guard process.
//!
//! Adds a zram device with the fastest codec the kernel offers, ahead of the
//! firmware's own swap, and tunes `page-cluster` (and `swappiness` above 1 GB) for
//! swap that lives in RAM. The changes are system-wide, and the browser can die by
//! SIGKILL (the OOM killer, a firmware's kill helper) with nothing run. So a child
//! process makes them and reverts them once the browser's end of a pipe closes,
//! however it closed: it leaves the browser's session, ignores the signals a
//! launcher sends, and asks the OOM killer to pass it over.
//!
//! What the guard changed is also written to a state file, which the next start
//! reverts when the guard did not get to.

use std::ffi::CString;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// The hidden flag that turns the binary into the guard; the state path follows it.
const GUARD_FLAG: &str = "--swap-guard";
/// Not the browser's name, so a kill by that name misses the guard; `comm` caps
/// it at 15 bytes.
const GUARD_NAME: &std::ffi::CStr = c"retsurf-swapgd";
/// Sent once the changes are in place.
const READY: &str = "ready";
const STATE_FILE: &str = "swap-guard.state";

/// Where the engine's own `tight` memory profile stops applying.
const TUNED_MAX_RAM_MB: u64 = 1536;
/// On 1 GB boards a higher swappiness only speeds the thrash.
const SWAPPINESS_MIN_RAM_MB: u64 = 1024;
/// Enough for leftovers, too little to turn an OOM kill into a thrash.
const ZRAM_RAM_DIVISOR: u64 = 4;
/// Fastest to decompress first: a fault waits on it.
const CODECS: [&str; 3] = ["lz4", "lzo-rle", "lzo"];
/// Outranks the firmware's swap so new pages land in ours.
const ZRAM_PRIORITY: i32 = 1100;
/// The default of 3 fetches eight pages a fault; zram decompresses each one.
const PAGE_CLUSTER: &str = "0";
/// Dropping a file page means re-reading the binary off the card.
const SWAPPINESS: &str = "100";
/// `linux/swap.h`; libc does not export them.
const SWAP_FLAG_PREFER: i32 = 0x8000;
const SWAP_FLAG_PRIO_MASK: i32 = 0x7fff;
/// Never chosen by the OOM killer.
const OOM_SCORE_ADJ_MIN: &str = "-1000";
/// How long a start waits for a previous guard still reverting.
const PREVIOUS_GUARD_WAIT: Duration = Duration::from_secs(10);

/// What the guard changed, so exactly that can be put back.
#[derive(Debug, Default, PartialEq)]
struct Tuning {
    guard_pid: Option<u32>,
    zram: Option<u32>,
    page_cluster: Option<String>,
    swappiness: Option<String>,
    loaded_module: bool,
}

impl Tuning {
    fn is_empty(&self) -> bool {
        self.zram.is_none()
            && self.page_cluster.is_none()
            && self.swappiness.is_none()
            && !self.loaded_module
    }

    fn to_state(&self) -> String {
        let mut out = String::new();
        let mut line = |key: &str, value: &dyn std::fmt::Display| {
            out.push_str(&format!("{key}={value}\n"));
        };
        if let Some(pid) = self.guard_pid {
            line("guard_pid", &pid);
        }
        if let Some(n) = self.zram {
            line("zram", &n);
        }
        if let Some(v) = &self.page_cluster {
            line("page_cluster", v);
        }
        if let Some(v) = &self.swappiness {
            line("swappiness", v);
        }
        if self.loaded_module {
            line("loaded_module", &1);
        }
        out
    }

    /// Unknown keys and unparsable values are skipped, not fatal.
    fn from_state(text: &str) -> Self {
        let mut t = Self::default();
        for (key, value) in text.lines().filter_map(|l| l.split_once('=')) {
            let value = value.trim();
            match key.trim() {
                "guard_pid" => t.guard_pid = value.parse().ok(),
                "zram" => t.zram = value.parse().ok(),
                "page_cluster" => t.page_cluster = Some(value.to_owned()),
                "swappiness" => t.swappiness = Some(value.to_owned()),
                "loaded_module" => t.loaded_module = value == "1",
                _ => {}
            }
        }
        t
    }
}

/// The kernel files the tuning touches, under a root tests can replace.
struct Sys {
    root: PathBuf,
}

impl Sys {
    fn live() -> Self {
        Self {
            root: PathBuf::from("/"),
        }
    }

    /// Only the real system gets kernel modules loaded into it.
    fn is_live(&self) -> bool {
        self.root == Path::new("/")
    }

    fn zram_control(&self, file: &str) -> PathBuf {
        self.root.join("sys/class/zram-control").join(file)
    }

    fn zram_block(&self, n: u32) -> PathBuf {
        self.root.join(format!("sys/block/zram{n}"))
    }

    fn zram_dev(&self, n: u32) -> PathBuf {
        self.root.join(format!("dev/zram{n}"))
    }

    fn vm(&self, knob: &str) -> PathBuf {
        self.root.join("proc/sys/vm").join(knob)
    }

    fn swaps(&self) -> PathBuf {
        self.root.join("proc/swaps")
    }
}

/// Run as the guard when the command line says so; never returns then. Must come
/// before anything else in the process starts.
pub fn run_if_guard() {
    let mut args = std::env::args_os().skip(1);
    if args.next().as_deref() != Some(GUARD_FLAG.as_ref()) {
        return;
    }
    let Some(state) = args.next() else {
        std::process::exit(1);
    };
    guard(Path::new(&state));
}

/// Revert what a dead guard left behind, then start a new one when `enabled`
/// and the board is small enough to need it.
pub fn start(enabled: bool, data_dir: &str) {
    let state = Path::new(data_dir).join(STATE_FILE);
    revert_leftovers(&state);
    if !enabled {
        return;
    }
    let ram_mb = crate::browser::memory::detect_ram_mb();
    if ram_mb > TUNED_MAX_RAM_MB {
        log::info!("swap tuning: {ram_mb} MB of RAM, nothing to tune");
        return;
    }
    if let Err(e) = spawn_guard(&state) {
        log::warn!("swap tuning: could not start the guard: {e}");
    }
}

fn spawn_guard(state: &Path) -> std::io::Result<()> {
    let mut child = Command::new(std::env::current_exe()?)
        .arg(GUARD_FLAG)
        .arg(state)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()?;
    let mut line = String::new();
    if let Some(stdout) = child.stdout.take() {
        BufReader::new(stdout).read_line(&mut line)?;
    }
    if line.trim() != READY {
        log::warn!("swap tuning: the guard exited before it was ready");
        return Ok(());
    }
    // Held until the process ends, whatever ends it: the guard reverts on EOF.
    std::mem::forget(child.stdin.take());
    Ok(())
}

fn guard(state: &Path) -> ! {
    detach();
    let sys = Sys::live();
    let mut tuning = apply(&sys, crate::browser::memory::detect_ram_mb());
    if !tuning.is_empty() {
        tuning.guard_pid = Some(std::process::id());
        let _ = std::fs::write(state, tuning.to_state());
    }
    {
        let mut stdout = std::io::stdout();
        let _ = writeln!(stdout, "{READY}");
        let _ = stdout.flush();
    }
    // Blocks until the browser's end closes: a clean exit, a crash or a SIGKILL.
    let _ = std::io::copy(&mut std::io::stdin(), &mut std::io::sink());
    revert(&sys, &tuning);
    let _ = std::fs::remove_file(state);
    std::process::exit(0);
}

/// Out of the browser's session, renamed, deaf to a launcher's signals, and the
/// OOM killer's last choice.
fn detach() {
    // SAFETY: plain syscalls on this process, no memory handed over.
    unsafe {
        libc::setsid();
        libc::prctl(libc::PR_SET_NAME, GUARD_NAME.as_ptr());
        for sig in [libc::SIGTERM, libc::SIGINT, libc::SIGHUP, libc::SIGQUIT] {
            libc::signal(sig, libc::SIG_IGN);
        }
    }
    let _ = std::fs::write("/proc/self/oom_score_adj", OOM_SCORE_ADJ_MIN);
}

/// Reverts what a dead guard left; waits for one still reverting.
fn revert_leftovers(state: &Path) {
    let Ok(text) = std::fs::read_to_string(state) else {
        return;
    };
    let tuning = Tuning::from_state(&text);
    if let Some(pid) = tuning.guard_pid {
        let deadline = Instant::now() + PREVIOUS_GUARD_WAIT;
        while guard_alive(pid) && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(100));
        }
        if !state.exists() {
            return;
        }
    }
    log::warn!("swap tuning: reverting what the last run left behind");
    revert(&Sys::live(), &tuning);
    let _ = std::fs::remove_file(state);
}

fn guard_alive(pid: u32) -> bool {
    read_trimmed(Path::new(&format!("/proc/{pid}/comm")))
        .is_some_and(|comm| comm.as_bytes() == GUARD_NAME.to_bytes())
}

fn apply(sys: &Sys, ram_mb: u64) -> Tuning {
    let mut tuning = Tuning::default();
    add_zram(sys, ram_mb, &mut tuning);
    // Both knobs only matter with swap present.
    if tuning.zram.is_none() && !has_swap(sys) {
        return tuning;
    }
    tuning.page_cluster = set(&sys.vm("page-cluster"), PAGE_CLUSTER);
    if ram_mb > SWAPPINESS_MIN_RAM_MB {
        tuning.swappiness = set(&sys.vm("swappiness"), SWAPPINESS);
    }
    tuning
}

fn add_zram(sys: &Sys, ram_mb: u64, tuning: &mut Tuning) {
    let hot_add = sys.zram_control("hot_add");
    // ROCKNIX ships zram as a module and never loads it.
    if !hot_add.exists() && sys.is_live() {
        tuning.loaded_module = run("modprobe", &["zram".as_ref()]);
    }
    let Some(n) = read_trimmed(&hot_add).and_then(|s| s.parse::<u32>().ok()) else {
        return;
    };
    let block = sys.zram_block(n);
    let algorithm = block.join("comp_algorithm");
    let offered = read_trimmed(&algorithm).unwrap_or_default();
    let codec = CODECS.into_iter().find(|codec| {
        offered
            .split_whitespace()
            .any(|o| o.trim_matches(['[', ']']) == *codec)
            && std::fs::write(&algorithm, codec).is_ok()
    });

    let zram_mb = ram_mb / ZRAM_RAM_DIVISOR;
    let dev = sys.zram_dev(n);
    if std::fs::write(block.join("disksize"), (zram_mb << 20).to_string()).is_ok()
        && run("mkswap", &[dev.as_os_str()])
        && swapon(&dev, ZRAM_PRIORITY)
    {
        tuning.zram = Some(n);
        say(&format!(
            "swap tuning on (zram{n}, {zram_mb} MiB, {})",
            codec.unwrap_or("default codec")
        ));
    } else {
        let _ = std::fs::write(sys.zram_control("hot_remove"), n.to_string());
    }
}

/// Puts back only values still ours, so a later change by the OS stands.
fn revert(sys: &Sys, tuning: &Tuning) {
    if let Some(before) = &tuning.page_cluster {
        restore(&sys.vm("page-cluster"), PAGE_CLUSTER, before);
    }
    if let Some(before) = &tuning.swappiness {
        restore(&sys.vm("swappiness"), SWAPPINESS, before);
    }
    if let Some(n) = tuning.zram {
        // Fails when RAM cannot take the pages back; better left than forced.
        if !swapoff(&sys.zram_dev(n)) {
            say(&format!("could not release zram{n}; it stays until reboot"));
            return;
        }
        let _ = std::fs::write(sys.zram_control("hot_remove"), n.to_string());
    }
    if tuning.loaded_module {
        run("modprobe", &["-r".as_ref(), "zram".as_ref()]);
    }
}

/// The value it replaced, or `None` when nothing was written.
fn set(path: &Path, value: &str) -> Option<String> {
    let before = read_trimmed(path)?;
    if before == value || std::fs::write(path, value).is_err() {
        return None;
    }
    Some(before)
}

fn restore(path: &Path, ours: &str, before: &str) {
    if read_trimmed(path).as_deref() == Some(ours) {
        let _ = std::fs::write(path, before);
    }
}

/// Any active swap device or file; `/proc/swaps` opens with a header line.
fn has_swap(sys: &Sys) -> bool {
    std::fs::read_to_string(sys.swaps())
        .is_ok_and(|text| text.lines().skip(1).any(|l| l.starts_with('/')))
}

fn swapon(dev: &Path, priority: i32) -> bool {
    let Ok(path) = CString::new(dev.as_os_str().as_bytes()) else {
        return false;
    };
    let flags = SWAP_FLAG_PREFER | (priority & SWAP_FLAG_PRIO_MASK);
    // SAFETY: `path` is a valid NUL-terminated string for the call's duration.
    unsafe { libc::swapon(path.as_ptr(), flags) == 0 }
}

fn swapoff(dev: &Path) -> bool {
    let Ok(path) = CString::new(dev.as_os_str().as_bytes()) else {
        return false;
    };
    // SAFETY: `path` is a valid NUL-terminated string for the call's duration.
    unsafe { libc::swapoff(path.as_ptr()) == 0 }
}

fn run(program: &str, args: &[&std::ffi::OsStr]) -> bool {
    Command::new(program)
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// Drops a failed write instead of panicking like `eprintln!`: stderr's reader
/// may have died with the launcher.
fn say(message: &str) {
    let _ = writeln!(std::io::stderr(), "retsurf: {message}");
}

fn read_trimmed(path: &Path) -> Option<String> {
    std::fs::read_to_string(path)
        .ok()
        .map(|s| s.trim().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> Sys {
        let root = std::env::temp_dir().join(format!("retsurf-swap-guard-{name}"));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("proc/sys/vm")).unwrap();
        Sys { root }
    }

    #[test]
    fn state_round_trips() {
        let tuning = Tuning {
            guard_pid: Some(42),
            zram: Some(1),
            page_cluster: Some("3".into()),
            swappiness: Some("60".into()),
            loaded_module: true,
        };
        assert_eq!(Tuning::from_state(&tuning.to_state()), tuning);
    }

    #[test]
    fn state_skips_what_it_cannot_read() {
        let tuning = Tuning::from_state("zram=x\nnoise\nfuture=1\nswappiness=60\n");
        assert_eq!(
            tuning,
            Tuning {
                swappiness: Some("60".into()),
                ..Tuning::default()
            }
        );
    }

    /// Without zram or any other swap the knobs stay untouched.
    #[test]
    fn no_swap_leaves_the_knobs_alone() {
        let sys = tmp("noswap");
        std::fs::write(sys.vm("page-cluster"), "3\n").unwrap();
        std::fs::write(sys.swaps(), "Filename Type Size Used Priority\n").unwrap();

        assert!(apply(&sys, 1024).is_empty());
        assert_eq!(read_trimmed(&sys.vm("page-cluster")).unwrap(), "3");
    }

    /// With the firmware's swap present, 1 GB gets page-cluster but keeps swappiness.
    #[test]
    fn one_gigabyte_keeps_its_swappiness() {
        let sys = tmp("onegig");
        std::fs::write(sys.vm("page-cluster"), "3\n").unwrap();
        std::fs::write(sys.vm("swappiness"), "60\n").unwrap();
        std::fs::write(
            sys.swaps(),
            "Filename Type Size Used Priority\n/dev/zram0 partition 1 0 -2\n",
        )
        .unwrap();

        let tuning = apply(&sys, 1024);
        assert_eq!(tuning.page_cluster.as_deref(), Some("3"));
        assert_eq!(tuning.swappiness, None);
        assert_eq!(read_trimmed(&sys.vm("page-cluster")).unwrap(), PAGE_CLUSTER);
        assert_eq!(read_trimmed(&sys.vm("swappiness")).unwrap(), "60");

        revert(&sys, &tuning);
        assert_eq!(read_trimmed(&sys.vm("page-cluster")).unwrap(), "3");
    }

    /// A value the OS changed after us is not overruled.
    #[test]
    fn revert_keeps_a_value_that_is_no_longer_ours() {
        let sys = tmp("taken");
        std::fs::write(sys.vm("swappiness"), "10\n").unwrap();
        let tuning = Tuning {
            swappiness: Some("60".into()),
            ..Tuning::default()
        };

        revert(&sys, &tuning);
        assert_eq!(read_trimmed(&sys.vm("swappiness")).unwrap(), "10");
    }
}
