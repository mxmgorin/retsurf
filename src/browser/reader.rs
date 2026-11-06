//! Reader view: strip the page down to its article content with Mozilla's
//! Readability (vendored under `vendor/readability/`, Apache 2.0) and restyle
//! it for a small screen. Everything happens inside the page via injected JS —
//! the article replaces the DOM in place, so logged-in and dynamic pages work
//! without a refetch. The original DOM is gone afterwards, so toggling off is
//! a reload.

use super::{AppBrowser, TabMode};

static READABILITY_JS: &str = include_str!("../../vendor/readability/Readability.js");

/// Dark, narrow-column article styling sized for small handheld screens.
/// Inlined into a JS string literal, so: no newlines preserved (they're
/// stripped at splice time) and no quote characters.
static READER_CSS: &str = include_str!("assets/reader.css");

/// The in-page toggle, run inside one IIFE together with Readability's source
/// (so its `function Readability` never leaks into the page's globals).
/// Returns a status string for the Rust callback below.
static TOGGLE_JS: &str = include_str!("assets/reader_toggle.js");

/// Mozilla's check for whether a page reads as an article.
static READERABLE_JS: &str = include_str!("../../vendor/readability/Readability-readerable.js");

/// Put ahead of the toggle: returns early on a page that is not an article or
/// is already the reader view.
static ARTICLE_ONLY_JS: &str = include_str!("assets/reader_article_only.js");

impl AppBrowser {
    /// Toggle reader mode on the active page: extract the article and swap it
    /// in, or — when the page is already the reader view — reload to leave it.
    /// Pages without extractable content are left untouched (logged only).
    pub fn toggle_reader(&self) {
        self.run_reader(false);
    }

    /// Show the active page in reader view when `on`, else as the page; a page
    /// already shown that way, or a game, is left as it is.
    pub fn set_reader(&self, on: bool) {
        let mode = self.mode();
        if mode != TabMode::Game && (mode == TabMode::Reader) != on {
            self.toggle_reader();
        }
    }

    /// Enter reader view on the active page when it reads as an article.
    pub fn enter_reader_if_article(&self) {
        self.run_reader(true);
    }

    /// Run the toggle on the active page; `article_only` keeps it from a page
    /// that does not read as an article and from leaving reader view.
    fn run_reader(&self, article_only: bool) {
        let Some(webview) = self.inner.active_webview() else {
            return;
        };
        // Looked at: a page left by hand stays left.
        if let Some(tab) = self
            .inner
            .tabs
            .borrow_mut()
            .get_mut(self.inner.active.get())
        {
            let state = &mut tab.state;
            state.reader_checked.clone_from(&state.page_url);
        }
        let toggle = TOGGLE_JS.replace("__RETSURF_READER_CSS__", &READER_CSS.replace('\n', " "));
        let gate = match article_only {
            true => format!("{READERABLE_JS}\n{ARTICLE_ONLY_JS}"),
            false => String::new(),
        };
        let script = format!("(function() {{\n{READABILITY_JS}\n{gate}\n{toggle}\n}})()");
        let inner = self.inner.clone();
        webview.clone().evaluate_javascript(script, move |result| {
            match result {
                Ok(servo::JSValue::String(status)) => match status.as_str() {
                    "ok" | "kept" => {
                        log::debug!("reader mode: {status}");
                        if let Some(i) = inner.tab_index(webview.id()) {
                            let state = &mut inner.tabs.borrow_mut()[i].state;
                            if state.mode == TabMode::Page {
                                state.mode = TabMode::Reader;
                            }
                        }
                    }
                    // Already in reader view — the original DOM is gone, so
                    // leaving is a reload.
                    "reader" => {
                        if let Some(i) = inner.tab_index(webview.id()) {
                            inner.tabs.borrow_mut()[i].state.loading = true;
                        }
                        webview.reload();
                    }
                    "no-article" => log::info!("reader mode: no article found on this page"),
                    other => log::warn!("reader mode: {other}"),
                },
                Ok(other) => log::warn!("reader mode returned unexpected value: {other:?}"),
                Err(e) => log::warn!("reader mode failed: {e:?}"),
            }
        });
    }
}
