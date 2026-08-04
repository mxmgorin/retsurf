//! One background thread per file (ureq is blocking): stream to `<path>.part`,
//! rename into place. A watchdog fails stalled transfers (ureq has no idle timeout).

use crate::event::user::{UserEvent, UserEventSender};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::{Duration, Instant};

/// Deadline for each pre-body phase of the request (DNS, connect, headers).
const PHASE_TIMEOUT: Duration = Duration::from_secs(30);

/// No received bytes for this long fails the transfer as stalled.
const STALL_TIMEOUT: Duration = Duration::from_secs(60);

/// How often the watchdog checks progress and the cancel flag.
const WATCH_INTERVAL: Duration = Duration::from_secs(1);

/// Cancel grace before the watchdog resolves the entry for a blocked worker.
const CANCEL_GRACE: Duration = Duration::from_secs(3);

/// Worker → main-thread progress; `result` is write-once via `done`.
pub(super) struct Shared {
    pub received: AtomicU64,
    pub total: AtomicU64,
    pub cancel: AtomicBool,
    /// Claimed (exactly once) by whoever stores `result`.
    done: AtomicBool,
    pub result: Mutex<Option<Result<(), String>>>,
}

/// Shared agent with per-phase deadlines; mid-body stalls are the watchdog's job.
fn agent() -> &'static ureq::Agent {
    static AGENT: LazyLock<ureq::Agent> = LazyLock::new(|| {
        ureq::Agent::config_builder()
            .timeout_resolve(Some(PHASE_TIMEOUT))
            .timeout_connect(Some(PHASE_TIMEOUT))
            .timeout_recv_response(Some(PHASE_TIMEOUT))
            .build()
            .new_agent()
    });
    &AGENT
}

/// Spawn the worker and its watchdog; returns the destination path and progress handle.
pub(super) fn spawn(url: &str, dir: &str, sender: &UserEventSender) -> (String, Arc<Shared>) {
    let path = unique_path(dir, &filename_from_url(url));
    log::info!("downloading `{url}` -> `{path}`");
    let shared = Arc::new(Shared {
        received: AtomicU64::new(0),
        total: AtomicU64::new(0),
        cancel: AtomicBool::new(false),
        done: AtomicBool::new(false),
        result: Mutex::new(None),
    });
    {
        let url = url.to_string();
        let path = path.clone();
        let shared = shared.clone();
        let sender = sender.clone();
        std::thread::spawn(move || run(url, path, shared, sender));
    }
    {
        let shared = shared.clone();
        let sender = sender.clone();
        std::thread::spawn(move || watch(shared, sender));
    }
    (path, shared)
}

/// Worker-thread entry: stream to `<path>.part`, rename into place; partial removed on failure.
fn run(url: String, path: String, shared: Arc<Shared>, sender: UserEventSender) {
    let part = format!("{path}.part");
    let mut result = fetch(&url, &part, &shared, &sender);
    if result.is_ok() {
        result = std::fs::rename(&part, &path).map_err(|e| format!("rename: {e}"));
    }
    if let Err(e) = &result {
        let _ = std::fs::remove_file(&part);
        log::warn!("download `{url}` failed: {e}");
    }
    finish(&shared, result, &sender);
}

/// Publish `result` once (first of worker/watchdog wins) and wake the main loop.
fn finish(shared: &Shared, result: Result<(), String>, sender: &UserEventSender) {
    if shared.done.swap(true, Ordering::Relaxed) {
        return;
    }
    *shared.result.lock().unwrap() = Some(result);
    sender.send(UserEvent::DownloadUpdate);
}

/// Resolve what the worker can't: an unacknowledged cancel or a stalled socket.
fn watch(shared: Arc<Shared>, sender: UserEventSender) {
    let mut last_received = 0;
    let mut last_change = Instant::now();
    let mut cancelled_at: Option<Instant> = None;
    loop {
        std::thread::sleep(WATCH_INTERVAL);
        if shared.done.load(Ordering::Relaxed) {
            return;
        }
        let received = shared.received.load(Ordering::Relaxed);
        if received != last_received {
            last_received = received;
            last_change = Instant::now();
        }
        if shared.cancel.load(Ordering::Relaxed) {
            if cancelled_at.get_or_insert_with(Instant::now).elapsed() >= CANCEL_GRACE {
                finish(&shared, Err("cancelled".to_string()), &sender);
                return;
            }
        } else if last_change.elapsed() >= STALL_TIMEOUT {
            shared.cancel.store(true, Ordering::Relaxed);
            finish(
                &shared,
                Err(format!("stalled: no data for {}s", STALL_TIMEOUT.as_secs())),
                &sender,
            );
            return;
        }
    }
}

fn fetch(url: &str, part: &str, shared: &Shared, sender: &UserEventSender) -> Result<(), String> {
    let response = agent().get(url).call().map_err(|e| e.to_string())?;
    if let Some(total) = crate::net::content_length(response.headers()) {
        shared.total.store(total, Ordering::Relaxed);
    }
    let reader = response.into_body().into_reader();
    let file = std::fs::File::create(part).map_err(|e| e.to_string())?;
    crate::net::stream(reader, file, |_, received, due| {
        shared.received.store(received, Ordering::Relaxed);
        if due {
            sender.send(UserEvent::DownloadUpdate);
        }
        !shared.cancel.load(Ordering::Relaxed)
    })
}

/// Save name from the URL's last path segment, falling back to `download`.
pub(super) fn filename_from_url(url: &str) -> String {
    let name = url::Url::parse(url)
        .ok()
        .and_then(|u| {
            u.path_segments()
                .and_then(|s| s.rev().find(|s| !s.is_empty()).map(str::to_string))
        })
        .unwrap_or_default();
    let name = percent_encoding::percent_decode_str(&name)
        .decode_utf8_lossy()
        .to_string();
    let name: String = name
        .chars()
        .map(|c| {
            if c == '/' || c == '\\' || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect();
    if name.is_empty() || name == "." || name == ".." {
        "download".to_string()
    } else {
        name
    }
}

/// `dir/filename`, suffixed `-1`, `-2`, … until neither the file nor its `.part` exists.
pub(super) fn unique_path(dir: &str, filename: &str) -> String {
    let (stem, ext) = match filename.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s.to_string(), format!(".{e}")),
        _ => (filename.to_string(), String::new()),
    };
    let mut n = 0u32;
    loop {
        let name = if n == 0 {
            filename.to_string()
        } else {
            format!("{stem}-{n}{ext}")
        };
        let path = format!("{dir}{name}");
        let part = format!("{path}.part");
        if !std::path::Path::new(&path).exists() && !std::path::Path::new(&part).exists() {
            return path;
        }
        n += 1;
    }
}
