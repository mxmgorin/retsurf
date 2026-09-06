//! Reader mode: strip the page down to its article content with Mozilla's
//! Readability (vendored under `vendor/readability/`, Apache 2.0) and restyle
//! it for a small screen. Everything happens inside the page via injected JS —
//! the article replaces the DOM in place, so logged-in and dynamic pages work
//! without a refetch. The original DOM is gone afterwards, so toggling off is
//! a reload.

use super::AppBrowser;

static READABILITY_JS: &str = include_str!("../../vendor/readability/Readability.js");

/// Dark, narrow-column article styling sized for small handheld screens.
/// Inlined into a JS string literal, so: no newlines preserved (they're
/// stripped at splice time) and no quote characters.
static READER_CSS: &str = include_str!("assets/reader.css");

/// The in-page toggle, run inside one IIFE together with Readability's source
/// (so its `function Readability` never leaks into the page's globals).
/// Returns a status string for the Rust callback below.
static TOGGLE_JS: &str = include_str!("assets/reader_toggle.js");

impl AppBrowser {
    /// Toggle reader mode on the active page: extract the article and swap it
    /// in, or — when the page is already the reader view — reload to leave it.
    /// Pages without extractable content are left untouched (logged only).
    pub fn toggle_reader(&self) {
        let Some(webview) = self.inner.active_webview() else {
            return;
        };
        let toggle = TOGGLE_JS.replace("__RETSURF_READER_CSS__", &READER_CSS.replace('\n', " "));
        let script = format!("(function() {{\n{READABILITY_JS}\n{toggle}\n}})()");
        webview.clone().evaluate_javascript(script, move |result| {
            match result {
                Ok(servo::JSValue::String(status)) => match status.as_str() {
                    "ok" => log::debug!("reader mode: article extracted"),
                    // Already in reader view — the original DOM is gone, so
                    // leaving is a reload.
                    "reader" => webview.reload(),
                    "no-article" => log::info!("reader mode: no article found on this page"),
                    other => log::warn!("reader mode: {other}"),
                },
                Ok(other) => log::warn!("reader mode returned unexpected value: {other:?}"),
                Err(e) => log::warn!("reader mode failed: {e:?}"),
            }
        });
    }
}
