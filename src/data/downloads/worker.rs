//! The downloading itself: one background thread per file (ureq is blocking),
//! streaming to `<path>.part` and renaming into place on success. Progress flows
//! back through [`Shared`] (read by [`super::Downloads::poll`]); the worker wakes
//! the main loop with [`UserEvent::DownloadUpdate`] so the UI repaints while idle.

use crate::event::user::{UserEvent, UserEventSender};
use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Worker → main-thread progress. The worker stores the counters as it streams
/// and the final result exactly once; the main loop sets `cancel` to stop it.
pub(super) struct Shared {
    pub received: AtomicU64,
    pub total: AtomicU64,
    pub cancel: AtomicBool,
    pub result: Mutex<Option<Result<(), String>>>,
}

/// Throttle between progress wakeups sent to the main loop.
const NOTIFY_EVERY: Duration = Duration::from_millis(250);

/// Pick a free destination path for `url` inside `dir` and spawn the worker
/// thread fetching it. Returns the path and the progress handle.
pub(super) fn spawn(url: &str, dir: &str, sender: &UserEventSender) -> (String, Arc<Shared>) {
    let path = unique_path(dir, &filename_from_url(url));
    log::info!("downloading `{url}` -> `{path}`");
    let shared = Arc::new(Shared {
        received: AtomicU64::new(0),
        total: AtomicU64::new(0),
        cancel: AtomicBool::new(false),
        result: Mutex::new(None),
    });
    {
        let url = url.to_string();
        let path = path.clone();
        let shared = shared.clone();
        let sender = sender.clone();
        std::thread::spawn(move || run(url, path, shared, sender));
    }
    (path, shared)
}

/// Worker-thread entry: stream the URL to `<path>.part`, then rename into place.
/// The partial file is removed on failure/cancel. Always stores a result and
/// wakes the main loop, so the poll sees the transition exactly once.
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
    *shared.result.lock().unwrap() = Some(result);
    sender.send(UserEvent::DownloadUpdate);
}

fn fetch(url: &str, part: &str, shared: &Shared, sender: &UserEventSender) -> Result<(), String> {
    let response = ureq::get(url).call().map_err(|e| e.to_string())?;
    if let Some(total) = response
        .headers()
        .get("Content-Length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok())
    {
        shared.total.store(total, Ordering::Relaxed);
    }
    let mut reader = response.into_body().into_reader();
    let mut file = std::fs::File::create(part).map_err(|e| e.to_string())?;
    let mut buf = [0u8; 64 * 1024];
    let mut received = 0u64;
    let mut last_notify = Instant::now();
    loop {
        if shared.cancel.load(Ordering::Relaxed) {
            return Err("cancelled".to_string());
        }
        let n = reader.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            return Ok(());
        }
        file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        received += n as u64;
        shared.received.store(received, Ordering::Relaxed);
        if last_notify.elapsed() >= NOTIFY_EVERY {
            last_notify = Instant::now();
            sender.send(UserEvent::DownloadUpdate);
        }
    }
}

/// Derive a save name from the URL's last path segment (percent-decoded, path
/// separators stripped), falling back to `download`.
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

/// `dir/filename`, suffixed `-1`, `-2`, … before the extension until neither the
/// file nor its `.part` exists.
fn unique_path(dir: &str, filename: &str) -> String {
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
