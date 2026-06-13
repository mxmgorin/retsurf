//! The built-in start page served at `retsurf:home`. A navigation to this URL
//! is intercepted by the `load_web_resource` delegate hook (see
//! [`super::delegate`]) and answered with the HTML produced here — never fetched
//! over the network.
//!
//! The page itself is `resources/home.html` (a search box over a speed-dial
//! grid). It reads its data from a `window.__NEWTAB__` object the embedder sets
//! before the page script runs; [`render`] generates that object from the
//! configured search page and the user's saved bookmarks (read fresh from disk
//! each time, so the grid always reflects the current list) and splices it in at
//! the `__CONFIG__` marker.

use crate::data::bookmarks::Bookmarks;

/// The internal URL the start page lives at. Used as the default `home_page`
/// (see [`crate::config`]); navigations here are intercepted, never fetched.
pub const HOME_URL: &str = "retsurf:home";

/// The page shell; `__CONFIG__` is replaced with the injected data script.
const TEMPLATE: &str = include_str!("../../resources/home.html");

/// Whether `url` is the start-page sentinel.
pub fn is_home(url: &url::Url) -> bool {
    url.as_str() == HOME_URL
}

/// Render the start-page HTML: inject `window.__NEWTAB__` with the search-URL
/// template (`%s` is the query) and the saved bookmarks as speed-dial sites.
pub fn render(search_page: &str) -> String {
    let bookmarks = Bookmarks::load();
    let sites: String = bookmarks
        .urls()
        .iter()
        .map(|url| {
            format!(
                "{{name:{},url:{}}}",
                js_string(&host_label(url)),
                js_string(url),
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let config = format!(
        "<script>window.__NEWTAB__={{search:{},sites:[{}]}};</script>",
        js_string(search_page),
        sites,
    );
    TEMPLATE.replace("__CONFIG__", &config)
}

/// A short label for a speed-dial tile: the registrable domain name (the label
/// left of the public suffix) — e.g. `duckduckgo.com` → `duckduckgo`,
/// `en.wikipedia.org` → `wikipedia`, `bbc.co.uk` → `bbc`. Falls back to the full
/// host, then the raw string, when it can't be parsed.
fn host_label(url: &str) -> String {
    let Some(host) = url::Url::parse(url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_string))
    else {
        return url.to_string();
    };
    let host = host.trim_start_matches("www.");
    let parts: Vec<&str> = host.split('.').filter(|s| !s.is_empty()).collect();
    let n = parts.len();
    if n <= 1 {
        return host.to_string();
    }
    // Drop the public suffix to reach the registrable name. Most suffixes are
    // one label (`.com`); treat a trailing two-letter ccTLD preceded by a short
    // label (`.co.uk`, `.com.au`) as a two-label suffix.
    let suffix_len = if n >= 3 && parts[n - 2].len() <= 3 && parts[n - 1].len() == 2 {
        2
    } else {
        1
    };
    parts[n - suffix_len - 1].to_string()
}

/// Encode a string as a JavaScript double-quoted string literal. `<` is escaped
/// too so the value can't terminate the surrounding `</script>`.
fn js_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '<' => out.push_str("\\x3c"),
            _ => out.push(c),
        }
    }
    out.push('"');
    out
}
