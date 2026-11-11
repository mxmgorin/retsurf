//! Short names for what the lists and the start page show: a site's brand name
//! from its address, and a bookmark's name or, unnamed, its site.

use crate::data::bookmarks::Bookmark;
use std::cell::RefCell;
use std::collections::HashMap;

thread_local! {
    /// Memoizes [`brand_label`] per URL: it parses a `Url`, and runs for every
    /// visible tile on each home repaint. Single-threaded — egui is main-thread.
    static BRAND_LABELS: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
    /// Memoizes [`site_label`] per URL, for the same reason.
    static SITE_LABELS: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
}

/// A URL's site as a list row names it: its host without `www.`, else the
/// URL itself.
pub fn site_label(url: &str) -> String {
    SITE_LABELS.with(|cache| {
        if let Some(label) = cache.borrow().get(url) {
            return label.clone();
        }
        let label = url::Url::parse(url)
            .ok()
            .and_then(|u| {
                u.host_str()
                    .map(|h| h.trim_start_matches("www.").to_string())
            })
            .unwrap_or_else(|| url.to_string());
        cache.borrow_mut().insert(url.to_owned(), label.clone());
        label
    })
}

/// Weekday names, Monday first.
const WEEKDAYS: [&str; 7] = [
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
    "Sunday",
];
/// Month names, short, January first.
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

/// A list's head over the entries of local `day` (days since the epoch), read
/// on `today`: Today, Yesterday, the weekday and date this year, else the date
/// with its year; `None` heads the entries with no time.
pub(super) fn day_label(day: Option<i64>, today: i64) -> String {
    let Some(day) = day else {
        return "Undated".to_string();
    };
    let (year, month, date) = crate::clock::civil_from_days(day);
    let month = MONTHS[month as usize - 1];
    match today - day {
        0 => "Today".to_string(),
        1 => "Yesterday".to_string(),
        _ if year == crate::clock::civil_from_days(today).0 => {
            let weekday = WEEKDAYS[crate::clock::weekday(day)];
            format!("{weekday}, {month} {date}")
        }
        _ => format!("{month} {date}, {year}"),
    }
}

/// The local time of day at unix time `secs` as `HH:MM`; a dash for no time.
pub(super) fn time_label(secs: u64) -> String {
    if secs == 0 {
        return "\u{2014}".to_string();
    }
    let (hour, minute) = crate::clock::local_time(secs);
    format!("{hour:02}:{minute:02}")
}

/// What a bookmark is shown as: its name, else its site.
pub fn bookmark_label(bookmark: &Bookmark) -> String {
    match bookmark.name.is_empty() {
        true => brand_label(&bookmark.url),
        false => bookmark.name.clone(),
    }
}

/// A short label for a tile or a menu URL row (memoized wrapper over
/// [`compute_brand_label`]).
pub(super) fn brand_label(url: &str) -> String {
    BRAND_LABELS.with(|cache| {
        if let Some(label) = cache.borrow().get(url) {
            return label.clone();
        }
        let label = compute_brand_label(url);
        cache.borrow_mut().insert(url.to_owned(), label.clone());
        label
    })
}

/// A short label for a tile: the registrable domain name (`en.wikipedia.org` ->
/// `wikipedia`, `bbc.co.uk` -> `bbc`), falling back to the host, then the raw string.
fn compute_brand_label(url: &str) -> String {
    let Ok(parsed) = url::Url::parse(url) else {
        return url.to_string();
    };
    let Some(host) = parsed.host() else {
        return url.to_string();
    };
    // An address has no registrable domain to shorten to — splitting on dots
    // would label `127.0.0.1` as "0".
    if !matches!(host, url::Host::Domain(_)) {
        return host.to_string();
    }
    let host = host.to_string();
    let host = host.trim_start_matches("www.");
    let parts: Vec<&str> = host.split('.').filter(|s| !s.is_empty()).collect();
    let n = parts.len();
    if n <= 1 {
        return host.to_string();
    }
    let suffix_len = if n >= 3 && parts[n - 2].len() <= 3 && parts[n - 1].len() == 2 {
        2
    } else {
        1
    };
    parts[n - suffix_len - 1].to_string()
}

#[cfg(test)]
mod tests {
    use super::{compute_brand_label, day_label};
    use crate::clock::days_from_civil;

    #[test]
    fn day_labels_name_recent_days_and_date_the_rest() {
        let today = days_from_civil(2026, 11, 10);
        assert_eq!(day_label(Some(today), today), "Today");
        assert_eq!(day_label(Some(today - 1), today), "Yesterday");
        assert_eq!(day_label(Some(today - 2), today), "Sunday, Nov 8");
        let last_year = days_from_civil(2025, 12, 31);
        assert_eq!(day_label(Some(last_year), today), "Dec 31, 2025");
        assert_eq!(day_label(None, today), "Undated");
    }

    #[test]
    fn brand_label_takes_the_registrable_domain() {
        assert_eq!(
            compute_brand_label("https://en.wikipedia.org/wiki/X"),
            "wikipedia"
        );
        assert_eq!(compute_brand_label("https://www.google.com/"), "google");
        assert_eq!(compute_brand_label("https://bbc.co.uk"), "bbc");
        assert_eq!(compute_brand_label("http://localhost:8099/x"), "localhost");
    }

    /// An address has no registrable domain; splitting on dots used to yield "0".
    #[test]
    fn brand_label_keeps_addresses_whole() {
        assert_eq!(compute_brand_label("http://127.0.0.1:8099/x"), "127.0.0.1");
        assert_eq!(compute_brand_label("http://[::1]:8099/x"), "[::1]");
    }

    /// The internal pages (and anything unparseable) fall back to the raw string.
    #[test]
    fn brand_label_falls_back_to_the_input() {
        assert_eq!(compute_brand_label("retsurf:home"), "retsurf:home");
        assert_eq!(compute_brand_label("not a url"), "not a url");
    }
}
