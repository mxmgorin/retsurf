//! Capturing downloads that a page builds in JavaScript (`fetch` +
//! `URL.createObjectURL`). Servo has no `download` attribute support, so such
//! a click navigates to the `blob:` URL and saves nothing. An injected script
//! intercepts the click, encodes the blob, and pings a sentinel URL;
//! [`super::delegate`] answers the ping locally and queues the tab, the bytes
//! come back over `evaluate_javascript`, [`crate::data::downloads`] saves them.

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

/// The script injected once per document load: `blob_download.js` with the
/// constants above spliced in. Its capture-phase listener grabs the `Blob`
/// before the page can `revokeObjectURL` it.
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
    Failed { error: String },
}

/// One file captured from a page, ready for [`crate::data::downloads`].
pub struct BlobDownload {
    pub filename: String,
    /// `Err` carries a page-side failure (over the size limit, unreadable blob).
    pub bytes: Result<Vec<u8>, String>,
}

/// Parse what [`TAKE_JS`] returned. `None` means the queue was empty.
pub(super) fn parse_taken(value: &str) -> Option<BlobDownload> {
    if value.is_empty() {
        return None;
    }
    Some(match serde_json::from_str(value) {
        Ok(Taken::File { name, data }) => BlobDownload {
            filename: sanitize(&name),
            bytes: base64::engine::general_purpose::STANDARD
                .decode(data)
                .map_err(|e| format!("decode: {e}")),
        },
        Ok(Taken::Failed { error }) => BlobDownload {
            filename: FALLBACK_NAME.to_string(),
            bytes: Err(error),
        },
        Err(e) => BlobDownload {
            filename: FALLBACK_NAME.to_string(),
            bytes: Err(format!("unexpected capture payload: {e}")),
        },
    })
}

/// The name is page-controlled: strip separators, traversal, and control
/// characters; never return empty.
fn sanitize(name: &str) -> String {
    let name: String = name
        .chars()
        .filter(|c| !matches!(c, '/' | '\\') && !c.is_control())
        .collect();
    let name = name.trim().trim_start_matches('.').to_string();
    if name.is_empty() {
        FALLBACK_NAME.to_string()
    } else {
        name
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The happy path: name and payload survive the round trip.
    #[test]
    fn parses_a_captured_file() {
        let taken =
            parse_taken(r#"{"name":"report.pdf","data":"aGVsbG8="}"#).expect("a queued item");
        assert_eq!(taken.filename, "report.pdf");
        assert_eq!(taken.bytes.expect("valid base64"), b"hello");
    }

    /// A page-side failure arrives as a message, not as a corrupt file.
    #[test]
    fn parses_an_error() {
        let taken = parse_taken(r#"{"error":"video.mkv: too big"}"#).expect("a queued item");
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
        let taken = parse_taken("{not json").expect("an entry");
        assert!(taken.bytes.is_err());
    }

    /// The name comes from the page, so it can't be trusted as a path.
    #[test]
    fn strips_path_components_from_the_name() {
        let taken = parse_taken(r#"{"name":"../../etc/passwd","data":""}"#).expect("a queued item");
        assert_eq!(taken.filename, "etcpasswd");
    }

    /// JSON carries any name safely; sanitize still drops control characters.
    #[test]
    fn strips_control_characters_from_the_name() {
        let taken = parse_taken(r#"{"name":"re\nport.pdf","data":""}"#).expect("a queued item");
        assert_eq!(taken.filename, "report.pdf");
    }

    /// The substituted script must not leave placeholder tokens behind.
    #[test]
    fn capture_js_is_fully_substituted() {
        let js = capture_js();
        assert!(!js.contains("__PING_URL__") && !js.contains("__MAX_BYTES__"));
        assert!(js.contains(PING_URL));
    }
}
