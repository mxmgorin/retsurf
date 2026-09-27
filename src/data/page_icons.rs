//! Page icons on disk: one PNG per host under `page_icons/` in the user data
//! dir, so bookmarks, history and the speed dial show a site's icon without the
//! page being open. Keyed by host, not URL, because a site's pages share one
//! icon. The set of files mirrors visited hosts, so it is pruned to the hosts
//! still referenced and wiped along with the browsing data.

use crate::browser::Favicon;
use std::collections::HashSet;
use std::path::PathBuf;

const DIR: &str = "page_icons";
const EXT: &str = "png";

/// The cache key for `url`: its lowercase host without a leading `www.`, for
/// `http(s)` only (built-in pages and `file:` have no site icon).
pub fn host_key(url: &str) -> Option<String> {
    let url = url::Url::parse(url).ok()?;
    if !matches!(url.scheme(), "http" | "https") {
        return None;
    }
    let host = url.host_str()?.to_ascii_lowercase();
    let host = host.strip_prefix("www.").unwrap_or(&host);
    (!host.is_empty()).then(|| host.to_string())
}

/// Persist `icon` as `host`'s; a file with the same bytes is not rewritten.
pub fn save(host: &str, icon: &Favicon) {
    let Some(bytes) = encode(icon) else {
        return;
    };
    let path = file(host);
    if std::fs::read(&path).is_ok_and(|old| old == bytes) {
        return;
    }
    let written = std::fs::create_dir_all(dir()).and_then(|()| std::fs::write(&path, bytes));
    if let Err(e) = written {
        log::warn!("could not write page icon for {host}: {e}");
    }
}

/// `host`'s stored icon, if one was saved and still decodes.
pub fn load(host: &str) -> Option<Favicon> {
    decode(&std::fs::read(file(host)).ok()?)
}

pub fn prune(keep: &HashSet<String>) {
    let Ok(entries) = std::fs::read_dir(dir()) else {
        return;
    };
    let kept: HashSet<String> = keep.iter().map(|h| file_stem(h)).collect();
    for entry in entries.flatten() {
        let path = entry.path();
        let stale = path
            .file_stem()
            .and_then(|s| s.to_str())
            .is_none_or(|stem| !kept.contains(stem));
        if stale {
            if let Err(e) = std::fs::remove_file(&path) {
                log::warn!("could not remove page icon {}: {e}", path.display());
            }
        }
    }
}

fn dir() -> PathBuf {
    PathBuf::from(super::data_path(DIR))
}

fn file(host: &str) -> PathBuf {
    dir().join(format!("{}.{EXT}", file_stem(host)))
}

/// `host` as a portable file name: an IPv6 literal's `[` and `:` are not.
fn file_stem(host: &str) -> String {
    host.chars()
        .map(|c| match c {
            'a'..='z' | '0'..='9' | '.' | '-' => c,
            _ => '_',
        })
        .collect()
}

fn encode(icon: &Favicon) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    let mut encoder = png::Encoder::new(&mut out, icon.width as u32, icon.height as u32);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let written = encoder
        .write_header()
        .and_then(|mut w| w.write_image_data(&icon.rgba));
    match written {
        Ok(()) => Some(out),
        Err(e) => {
            log::warn!("could not encode page icon: {e}");
            None
        }
    }
}

/// Only what [`encode`] writes is accepted; a hand-placed file of another
/// format reads as absent.
fn decode(bytes: &[u8]) -> Option<Favicon> {
    let mut reader = png::Decoder::new(std::io::Cursor::new(bytes))
        .read_info()
        .ok()?;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    if info.color_type != png::ColorType::Rgba || info.bit_depth != png::BitDepth::Eight {
        return None;
    }
    buf.truncate(info.buffer_size());
    Favicon::new(info.width as usize, info.height as usize, buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn host_key_folds_www_and_case_and_skips_non_web() {
        assert_eq!(
            host_key("https://WWW.Example.com/a?b").as_deref(),
            Some("example.com")
        );
        assert_eq!(
            host_key("http://sub.example.com:8080/").as_deref(),
            Some("sub.example.com")
        );
        assert_eq!(host_key("retsurf:home"), None);
        assert_eq!(host_key("file:///tmp/a.html"), None);
        assert_eq!(host_key("not a url"), None);
    }

    #[test]
    fn file_stem_keeps_hostnames_and_escapes_ipv6() {
        assert_eq!(file_stem("xn--e1afmkfd.xn--p1ai"), "xn--e1afmkfd.xn--p1ai");
        assert_eq!(file_stem("[::1]"), "___1_");
    }

    #[test]
    fn png_round_trip_keeps_pixels() {
        let rgba: Vec<u8> = (0..4 * 3 * 4).map(|i| i as u8).collect();
        let icon = Favicon::new(4, 3, rgba.clone()).unwrap();
        let back = decode(&encode(&icon).unwrap()).unwrap();

        assert_eq!((back.width, back.height), (4, 3));
        assert_eq!(&back.rgba[..], &rgba[..]);
    }
}
