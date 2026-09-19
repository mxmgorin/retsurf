//! Shared HTTP for everything the app fetches on its own behalf — the download
//! worker ([`crate::data::downloads`]), the self-updater ([`crate::update`]) and
//! the adblock filter lists: one agent with pre-body deadlines, request
//! construction, and chunked streaming with a throttled progress callback.
//! Per-chunk work (hashing, cancellation, notifying the main loop) stays with
//! the caller.

use std::io::{Read, Write};
use std::sync::LazyLock;
use std::time::{Duration, Instant};

/// Deadline for each pre-body phase of a request (DNS, connect, headers).
const PHASE_TIMEOUT: Duration = Duration::from_secs(30);

/// How the app identifies itself when fetching for itself rather than for a
/// page (the GitHub API 403s a request without one). Fetches on a page's
/// behalf send the browser's own User-Agent instead.
pub const USER_AGENT: &str = concat!("retsurf/", env!("CARGO_PKG_VERSION"));

/// The shared agent, with per-phase deadlines so no fetch can hang before its
/// body; mid-body stalls are each caller's own watchdog's job.
pub fn agent() -> &'static ureq::Agent {
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

/// A GET on the shared agent, identified as [`USER_AGENT`].
pub fn get(url: &str) -> ureq::RequestBuilder<ureq::typestate::WithoutBody> {
    agent().get(url).header("User-Agent", USER_AGENT)
}

/// Throttle for [`stream`]'s progress notifications.
const NOTIFY_EVERY: Duration = Duration::from_millis(250);

/// Read buffer size for response streaming.
const CHUNK: usize = 64 * 1024;

/// A response's `Content-Length`, or `None` when absent/unparsable.
pub fn content_length(headers: &ureq::http::HeaderMap) -> Option<u64> {
    headers
        .get("Content-Length")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok())
}

/// Stream `reader` into `writer` in [`CHUNK`]-sized reads. `progress` gets each
/// chunk, the running total, and whether the [`NOTIFY_EVERY`] throttle elapsed;
/// returning `false` aborts with `Err("cancelled")`, leaving the partial write.
pub fn stream(
    mut reader: impl Read,
    mut writer: impl Write,
    mut progress: impl FnMut(&[u8], u64, bool) -> bool,
) -> Result<(), String> {
    let mut buf = [0u8; CHUNK];
    let mut received = 0u64;
    let mut last_notify = Instant::now();
    loop {
        let n = reader.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            return Ok(());
        }
        writer.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        received += n as u64;
        let due = last_notify.elapsed() >= NOTIFY_EVERY;
        if due {
            last_notify = Instant::now();
        }
        if !progress(&buf[..n], received, due) {
            return Err("cancelled".to_string());
        }
    }
}
