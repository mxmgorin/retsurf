//! The one wall-clock read, shared by history, downloads and the updater.

use std::time::{SystemTime, UNIX_EPOCH};

/// Current unix time in seconds, or `0` if the clock is before the epoch
pub fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
