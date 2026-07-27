//! Shared HTTP download streaming for the download worker
//! ([`crate::data::downloads`]) and the self-updater ([`crate::update`]):
//! chunked writes with a throttled progress callback. Per-chunk work (hashing,
//! cancellation, notifying the main loop) stays with the caller.

use std::io::{Read, Write};
use std::time::{Duration, Instant};

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
