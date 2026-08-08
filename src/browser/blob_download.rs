//! Capturing downloads a page triggers from JavaScript (Servo has no `download`
//! attribute support, so those clicks navigate and save nothing). A user script
//! intercepts them and pings a sentinel URL; [`super::delegate`] queues the tab
//! and the entries come back over `evaluate_javascript`: in-page bytes
//! (`blob:`/`data:`) arrive whole, `a[download]` http(s) links as URLs to fetch.

use base64::Engine;
use serde::Deserialize;
use std::sync::LazyLock;

/// Sentinel the page loads to announce a waiting capture; the resource hook
/// answers it, so the request never leaves the device.
pub(super) const PING_URL: &str = "https://retsurf.invalid/blob-download";

/// Capture cap: `evaluate_javascript` moves the file as one base64 string, so
/// a video must fail fast instead of exhausting a handheld's memory.
const MAX_BYTES: usize = 32 * 1024 * 1024;

/// File name used when the page suggests none.
const FALLBACK_NAME: &str = "download";

/// The user script: `blob_download.js` with the constants above spliced in.
pub(super) fn capture_js() -> &'static str {
    static JS: LazyLock<String> = LazyLock::new(|| {
        include_str!("blob_download.js")
            .replace("__PING_URL__", PING_URL)
            .replace("__MAX_BYTES__", &MAX_BYTES.to_string())
    });
    &JS
}

/// Pops one queue entry as JSON (a [`Taken`]); empty string once drained.
pub(super) const TAKE_JS: &str = r#"(function () {
  var d = window.__retsurfDl;
  if (!d || !d.pending.length) return "";
  return JSON.stringify(d.pending.shift());
})()"#;

/// One queue entry as the injected script's `queue()` builds it.
#[derive(Deserialize)]
#[serde(untagged)]
enum Taken {
    File { name: String, data: String },
    Link { url: String, name: String },
    Failed { error: String },
}

/// One file captured from a page, ready for [`crate::data::downloads`].
pub struct BlobDownload {
    pub filename: String,
    /// `Err` carries a page-side failure (over the size limit, unreadable blob).
    pub bytes: Result<Vec<u8>, String>,
}

/// One drained queue entry.
pub(super) enum Captured {
    /// Bytes built in-page, born finished.
    File(BlobDownload),
    /// An `a[download]` link; fetched like an intercepted navigation.
    Link { url: String, name: Option<String> },
}

/// Parse what [`TAKE_JS`] returned. `None` means the queue was empty.
pub(super) fn parse_taken(value: &str) -> Option<Captured> {
    if value.is_empty() {
        return None;
    }
    Some(match serde_json::from_str(value) {
        Ok(Taken::File { name, data }) => Captured::File(BlobDownload {
            filename: sanitize(&name).unwrap_or_else(|| FALLBACK_NAME.to_string()),
            bytes: base64::engine::general_purpose::STANDARD
                .decode(data)
                .map_err(|e| format!("decode: {e}")),
        }),
        // The page controls the URL; only http(s) may reach the fetch worker.
        Ok(Taken::Link { url, name }) => {
            match url::Url::parse(&url).map(|u| u.scheme().to_string()) {
                Ok(scheme) if scheme == "http" || scheme == "https" => Captured::Link {
                    url,
                    name: sanitize(&name),
                },
                _ => Captured::File(BlobDownload {
                    filename: sanitize(&name).unwrap_or_else(|| FALLBACK_NAME.to_string()),
                    bytes: Err(format!("blocked download url: {url}")),
                }),
            }
        }
        Ok(Taken::Failed { error }) => Captured::File(BlobDownload {
            filename: FALLBACK_NAME.to_string(),
            bytes: Err(error),
        }),
        Err(e) => Captured::File(BlobDownload {
            filename: FALLBACK_NAME.to_string(),
            bytes: Err(format!("unexpected capture payload: {e}")),
        }),
    })
}

/// The name is page-controlled: strip separators, traversal, and control
/// characters. `None` when nothing usable remains.
fn sanitize(name: &str) -> Option<String> {
    let name: String = name
        .chars()
        .filter(|c| !matches!(c, '/' | '\\') && !c.is_control())
        .collect();
    let name = name.trim().trim_start_matches('.').trim();
    (!name.is_empty()).then(|| name.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(value: &str) -> BlobDownload {
        match parse_taken(value).expect("a queued item") {
            Captured::File(item) => item,
            Captured::Link { .. } => panic!("expected a file entry"),
        }
    }

    /// The happy path: name and payload survive the round trip.
    #[test]
    fn parses_a_captured_file() {
        let taken = file(r#"{"name":"report.pdf","data":"aGVsbG8="}"#);
        assert_eq!(taken.filename, "report.pdf");
        assert_eq!(taken.bytes.expect("valid base64"), b"hello");
    }

    /// A page-side failure arrives as a message, not as a corrupt file.
    #[test]
    fn parses_an_error() {
        let taken = file(r#"{"error":"video.mkv: too big"}"#);
        assert_eq!(taken.bytes.unwrap_err(), "video.mkv: too big");
    }

    /// An empty queue must not produce a download entry.
    #[test]
    fn empty_queue_yields_nothing() {
        assert!(parse_taken("").is_none());
    }

    /// A malformed payload surfaces as a failed entry, not a silent drop.
    #[test]
    fn malformed_payload_becomes_an_error() {
        assert!(file("{not json").bytes.is_err());
    }

    /// The name comes from the page, so it can't be trusted as a path.
    #[test]
    fn strips_path_components_from_the_name() {
        let taken = file(r#"{"name":"../../etc/passwd","data":""}"#);
        assert_eq!(taken.filename, "etcpasswd");
    }

    /// JSON carries any name safely; sanitize still drops control characters.
    #[test]
    fn strips_control_characters_from_the_name() {
        let taken = file(r#"{"name":"re\nport.pdf","data":""}"#);
        assert_eq!(taken.filename, "report.pdf");
    }

    /// An `a[download]` link arrives as a URL with its suggested name.
    #[test]
    fn parses_a_link() {
        match parse_taken(r#"{"url":"https://x.test/gen?id=5","name":"report.pdf"}"#) {
            Some(Captured::Link { url, name }) => {
                assert_eq!(url, "https://x.test/gen?id=5");
                assert_eq!(name.as_deref(), Some("report.pdf"));
            }
            _ => panic!("expected a link entry"),
        }
    }

    /// A bare `download` attribute yields no suggested name.
    #[test]
    fn link_without_a_name() {
        match parse_taken(r#"{"url":"http://x.test/a.zip","name":""}"#) {
            Some(Captured::Link { name, .. }) => assert_eq!(name, None),
            _ => panic!("expected a link entry"),
        }
    }

    /// Only http(s) URLs may reach the fetch worker.
    #[test]
    fn blocks_non_http_link_schemes() {
        let taken = file(r#"{"url":"file:///etc/passwd","name":"x"}"#);
        assert!(taken.bytes.unwrap_err().starts_with("blocked download url"));
    }

    /// The substituted script must not leave placeholder tokens behind.
    #[test]
    fn capture_js_is_fully_substituted() {
        let js = capture_js();
        assert!(!js.contains("__PING_URL__") && !js.contains("__MAX_BYTES__"));
        assert!(js.contains(PING_URL));
    }
}
