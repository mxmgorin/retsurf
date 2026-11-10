//! The one wall-clock read, shared by history, downloads, bookmarks and the
//! updater, and the civil-date arithmetic their files are written in.

use std::time::{SystemTime, UNIX_EPOCH};

/// Current unix time in seconds, or `0` if the clock is before the epoch
pub fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Seconds east of UTC in the system's time zone at unix time `secs`; `0` where
/// no zone is set.
#[cfg(any(target_os = "linux", target_os = "android"))]
pub fn utc_offset(secs: u64) -> i64 {
    let Ok(at) = libc::time_t::try_from(secs) else {
        return 0;
    };
    let mut tm = std::mem::MaybeUninit::<libc::tm>::uninit();
    // SAFETY: both pointers are valid for the call, which fills `tm` when it
    // returns non-null.
    match unsafe { libc::localtime_r(&at, tm.as_mut_ptr()) }.is_null() {
        true => 0,
        // SAFETY: localtime_r filled `tm`.
        false => long_to_i64(unsafe { tm.assume_init() }.tm_gmtoff),
    }
}

/// A C `long` as `i64`: the same type on 64-bit targets, a narrower one on 32-bit.
#[cfg(any(target_os = "linux", target_os = "android"))]
fn long_to_i64(value: impl Into<i64>) -> i64 {
    value.into()
}

/// Seconds east of UTC; this platform's time zone is not read.
#[cfg(not(any(target_os = "linux", target_os = "android")))]
pub fn utc_offset(_secs: u64) -> i64 {
    0
}

/// Seconds in a minute.
pub const SECS_PER_MINUTE: u64 = 60;
const SECS_PER_HOUR: i64 = 60 * SECS_PER_MINUTE as i64;
const SECS_PER_DAY: i64 = 24 * SECS_PER_HOUR;

/// The local wall-clock time at unix time `secs`, as hour and minute.
pub fn local_time(secs: u64) -> (u32, u32) {
    let local = i64::try_from(secs).unwrap_or(i64::MAX) + utc_offset(secs);
    hour_minute(local)
}

/// The hour and minute of the day `secs` into it, counted from any midnight.
fn hour_minute(secs: i64) -> (u32, u32) {
    let of_day = secs.rem_euclid(SECS_PER_DAY);
    let hour = of_day / SECS_PER_HOUR;
    let minute = of_day % SECS_PER_HOUR / SECS_PER_MINUTE as i64;
    (hour as u32, minute as u32)
}

/// Days since the unix epoch as a `(year, month, day)` civil date (UTC), by
/// Howard Hinnant's civil-from-days.
pub fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = doy - (153 * mp + 2) / 5 + 1; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // [1, 12]
    let y = if m <= 2 { y + 1 } else { y };
    (y, m as u32, d as u32)
}

/// A civil date (UTC) as days since the unix epoch; the inverse of
/// [`civil_from_days`].
pub fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400; // [0, 399]
    let m = month as i64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + day as i64 - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::{civil_from_days, days_from_civil, hour_minute, now_unix, utc_offset};

    /// Real zones sit within a day of UTC, on quarter hours.
    #[test]
    fn utc_offset_is_a_zone() {
        const QUARTER_HOUR: i64 = 15 * 60;
        const MAX_OFFSET: i64 = 14 * 60 * 60;
        let offset = utc_offset(now_unix());
        println!("utc offset here: {offset}s");
        assert!(offset.abs() <= MAX_OFFSET);
        assert_eq!(offset % QUARTER_HOUR, 0);
    }

    #[test]
    fn the_time_of_day_wraps_at_midnight() {
        assert_eq!(hour_minute(0), (0, 0));
        assert_eq!(hour_minute(14 * 3600 + 32 * 60 + 59), (14, 32));
        assert_eq!(hour_minute(-60), (23, 59));
        assert_eq!(hour_minute(20_744 * 86_400 + 3_600), (1, 0));
    }

    #[test]
    fn civil_dates_round_trip() {
        for days in [-1, 0, 59, 60, 11_016, 20_744, 60_000] {
            let (y, m, d) = civil_from_days(days);
            assert_eq!(days_from_civil(y, m, d), days);
        }
        assert_eq!(civil_from_days(20_744), (2026, 10, 18));
    }
}
