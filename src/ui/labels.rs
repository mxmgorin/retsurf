//! Short names for what the lists and the start page show: a site's brand name
//! from its address, and a bookmark's name or, unnamed, its site.

use crate::data::bookmarks::Bookmark;
use std::cell::RefCell;
use std::collections::HashMap;

thread_local! {
    /// Memoizes [`brand_label`] per URL: it parses a `Url`, and runs for every
    /// visible tile on each home repaint. Single-threaded — egui is main-thread.
    static BRAND_LABELS: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
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
    use super::compute_brand_label;

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
