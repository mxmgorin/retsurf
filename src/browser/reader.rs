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
static READER_CSS: &str = r#"
:root { color-scheme: dark; }
body { margin: 0; background: #1b1b1f; color: #e8e6e3;
       font-family: sans-serif; font-size: 18px; line-height: 1.6; }
article { max-width: 40em; margin: 0 auto; padding: 14px 18px 48px; }
h1 { font-size: 1.45em; line-height: 1.25; margin: 0 0 0.25em; }
.retsurf-meta { color: #98968e; font-size: 0.85em; margin: 0 0 1.5em; }
img, video, figure, iframe { max-width: 100%; height: auto; }
figure { margin: 1em 0; }
a { color: #6cb6ff; }
pre { overflow-x: auto; background: #26262b; padding: 0.75em; }
code { background: #26262b; }
blockquote { border-left: 3px solid #44444c; margin: 1em 0;
             padding-left: 1em; color: #c8c6c0; }
"#;

/// The in-page toggle, run inside one IIFE together with Readability's source
/// (so its `function Readability` never leaks into the page's globals).
/// Returns a status string for the Rust callback below.
static TOGGLE_JS: &str = r#"
if (document.documentElement.dataset.retsurfReader) return "reader";
var article;
try {
    article = new Readability(document.cloneNode(true)).parse();
} catch (e) {
    return "error: " + e;
}
if (!article || !article.content) return "no-article";
var esc = function (s) {
    var d = document.createElement("div");
    d.textContent = s || "";
    return d.innerHTML;
};
var meta = [article.byline, article.siteName].filter(Boolean).join(" · ");
document.documentElement.dataset.retsurfReader = "1";
document.head.innerHTML = '<meta charset="utf-8"><title>' + esc(article.title) +
    '</title><style>__RETSURF_READER_CSS__</style>';
document.body.className = "";
document.body.removeAttribute("style");
document.body.innerHTML = "<article><h1>" + esc(article.title) + "</h1>" +
    (meta ? '<p class="retsurf-meta">' + esc(meta) + "</p>" : "") +
    article.content + "</article>";
window.scrollTo(0, 0);
return "ok";
"#;

impl AppBrowser {
    /// Toggle reader mode on the active page: extract the article and swap it
    /// in, or — when the page is already the reader view — reload to leave it.
    /// Pages without extractable content are left untouched (logged only).
    pub fn toggle_reader(&self) {
        let Some(webview) = self.inner.active_webview() else {
            return;
        };
        let toggle = TOGGLE_JS.replace(
            "__RETSURF_READER_CSS__",
            &READER_CSS.replace('\n', " "),
        );
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
