//! Save names for downloads: header/attribute/URL precedence, sanitizing,
//! and the on-disk uniqueness walk both reservation strategies share.

/// Save-name precedence (per the HTML spec): Content-Disposition, the page's
/// `download` attribute, a file-naming redirect target, the link URL.
pub(super) fn pick_filename(
    headers: &ureq::http::HeaderMap,
    final_path: &str,
    suggested: Option<&str>,
    url: &str,
) -> String {
    let disposition = headers
        .get("Content-Disposition")
        .map(|v| String::from_utf8_lossy(v.as_bytes()).into_owned());
    if let Some(name) = disposition.as_deref().and_then(filename_from_disposition) {
        return name;
    }
    if let Some(name) = suggested.and_then(sanitize) {
        return name;
    }
    if let Some(name) = filename_from_path(final_path) {
        if has_extension(&name) {
            return name;
        }
    }
    filename_from_url(url)
}

/// Filename from a Content-Disposition value (RFC 6266): `filename*` (RFC 5987)
/// wins over plain `filename=`, quoted or bare.
fn filename_from_disposition(value: &str) -> Option<String> {
    let mut plain = None;
    let mut extended = None;
    for param in value.split(';') {
        let Some((key, val)) = param.split_once('=') else {
            continue;
        };
        let val = val.trim();
        match key.trim().to_ascii_lowercase().as_str() {
            // charset'language'percent-encoded; everything past the last quote.
            "filename*" => {
                let encoded = val.rsplit_once('\'').map_or(val, |(_, e)| e);
                extended = Some(
                    percent_encoding::percent_decode_str(encoded)
                        .decode_utf8_lossy()
                        .into_owned(),
                );
            }
            "filename" => plain = Some(val.trim_matches('"').to_string()),
            _ => {}
        }
    }
    extended.or(plain).and_then(|name| sanitize(&name))
}

/// Save name from the URL's last path segment, falling back to `download`.
pub(super) fn filename_from_url(url: &str) -> String {
    url::Url::parse(url)
        .ok()
        .and_then(|u| filename_from_path(u.path()))
        .unwrap_or_else(|| "download".to_string())
}

/// Last non-empty segment of a URL path, percent-decoded and sanitized.
fn filename_from_path(path: &str) -> Option<String> {
    let segment = path.rsplit('/').find(|s| !s.is_empty())?;
    let name = percent_encoding::percent_decode_str(segment).decode_utf8_lossy();
    sanitize(&name)
}

/// Make an untrusted name safe as a bare file name: no separators, control
/// characters, or leading dots. `None` when nothing usable remains.
fn sanitize(name: &str) -> Option<String> {
    let name: String = name
        .chars()
        .filter(|c| !matches!(c, '/' | '\\') && !c.is_control())
        .collect();
    let name = name.trim().trim_start_matches('.').trim();
    (!name.is_empty()).then(|| name.to_string())
}

fn has_extension(name: &str) -> bool {
    matches!(name.rsplit_once('.'), Some((stem, ext)) if !stem.is_empty() && !ext.is_empty())
}

/// Candidate destinations for `filename` in `dir` — the name itself, then
/// `stem-1.ext`, `stem-2.ext`, … Both reservation strategies walk this list.
fn candidates<'a>(dir: &'a str, filename: &'a str) -> impl Iterator<Item = String> + 'a {
    let (stem, ext) = match filename.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => (s.to_string(), format!(".{e}")),
        _ => (filename.to_string(), String::new()),
    };
    (0u32..).map(move |n| match n {
        0 => format!("{dir}{filename}"),
        n => format!("{dir}{stem}-{n}{ext}"),
    })
}

/// Reserve a free destination: creating the `.part` exclusively is the
/// reservation, so parallel workers can't collide.
pub(super) fn create_unique(
    dir: &str,
    filename: &str,
) -> Result<(String, String, std::fs::File), String> {
    for path in candidates(dir, filename) {
        if std::path::Path::new(&path).exists() {
            continue;
        }
        let part = format!("{path}.part");
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&part)
        {
            Ok(file) => return Ok((path, part, file)),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(format!("create: {e}")),
        }
    }
    unreachable!("candidates never ends")
}

/// The first candidate where neither the file nor its `.part` exists. For
/// whole-file writes on the main thread; workers use [`create_unique`].
pub(super) fn unique_path(dir: &str, filename: &str) -> String {
    candidates(dir, filename)
        .find(|path| {
            !std::path::Path::new(path).exists()
                && !std::path::Path::new(&format!("{path}.part")).exists()
        })
        .expect("candidates never ends")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(disposition: Option<&'static str>) -> ureq::http::HeaderMap {
        let mut h = ureq::http::HeaderMap::new();
        if let Some(v) = disposition {
            h.insert(
                "Content-Disposition",
                ureq::http::HeaderValue::from_static(v),
            );
        }
        h
    }

    /// A quoted filename parameter names the file.
    #[test]
    fn disposition_quoted_filename() {
        let name = filename_from_disposition(r#"attachment; filename="release notes.pdf""#);
        assert_eq!(name.as_deref(), Some("release notes.pdf"));
    }

    /// A bare (unquoted) filename parameter also works.
    #[test]
    fn disposition_bare_filename() {
        let name = filename_from_disposition("attachment;filename=game.zip");
        assert_eq!(name.as_deref(), Some("game.zip"));
    }

    /// The RFC 5987 form is percent-decoded and preferred over plain filename.
    #[test]
    fn disposition_extended_filename_wins() {
        let name = filename_from_disposition(
            "attachment; filename=fallback.bin; filename*=UTF-8''na%C3%AFve%20rom.gba",
        );
        assert_eq!(name.as_deref(), Some("naïve rom.gba"));
    }

    /// A disposition without any filename yields nothing.
    #[test]
    fn disposition_without_filename() {
        assert_eq!(filename_from_disposition("inline"), None);
        assert_eq!(filename_from_disposition("attachment; size=42"), None);
    }

    /// Server names are untrusted: no traversal, no hidden files.
    #[test]
    fn disposition_name_is_sanitized() {
        let name = filename_from_disposition(r#"attachment; filename="../../.hidden""#);
        assert_eq!(name.as_deref(), Some("hidden"));
        assert_eq!(
            filename_from_disposition(r#"attachment; filename="...""#),
            None
        );
    }

    /// Content-Disposition beats the download attribute and both URLs.
    #[test]
    fn pick_prefers_disposition() {
        let h = headers(Some(r#"attachment; filename="real.zip""#));
        let name = pick_filename(
            &h,
            "/mirror/obj123",
            Some("asked.zip"),
            "https://x.test/a.zip",
        );
        assert_eq!(name, "real.zip");
    }

    /// Without a disposition, the page's download attribute names the file.
    #[test]
    fn pick_uses_the_suggested_name() {
        let h = headers(None);
        let name = pick_filename(
            &h,
            "/files/real.chd",
            Some("asked.chd"),
            "https://x.test/gen",
        );
        assert_eq!(name, "asked.chd");
    }

    /// Without either, a redirect target that names a file wins over the link.
    #[test]
    fn pick_uses_final_path_when_it_names_a_file() {
        let h = headers(None);
        let name = pick_filename(&h, "/files/game-1.2.chd", None, "https://x.test/latest.chd");
        assert_eq!(name, "game-1.2.chd");
    }

    /// An extension-less redirect target (a CDN token path) loses to the link URL.
    #[test]
    fn pick_falls_back_to_the_link_url() {
        let h = headers(None);
        let name = pick_filename(
            &h,
            "/obj/ab12f3",
            None,
            "https://x.test/roms/game.sfc?sig=1",
        );
        assert_eq!(name, "game.sfc");
    }

    /// URL names are percent-decoded and never empty.
    #[test]
    fn filename_from_url_decodes_and_falls_back() {
        assert_eq!(
            filename_from_url("https://x.test/a%20b.zip"),
            "a b.zip".to_string()
        );
        assert_eq!(filename_from_url("https://x.test/"), "download".to_string());
        assert_eq!(filename_from_url("not a url"), "download".to_string());
    }

    /// Path separators and control characters never reach the file system.
    #[test]
    fn sanitize_strips_separators_and_controls() {
        assert_eq!(sanitize("a/b\\c"), Some("abc".to_string()));
        assert_eq!(sanitize("re\nport.pdf"), Some("report.pdf".to_string()));
        assert_eq!(sanitize("  .. "), None);
    }
}
