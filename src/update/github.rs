//! The GitHub "latest release" query: one blocking `ureq` GET, parsed with
//! `serde_json`, compared against `CARGO_PKG_VERSION` with `semver`.

use super::{Offer, UpdateState, REPO, USER_AGENT};
use serde::Deserialize;

/// The tag `.github/workflows/nightly.yml` force-moves to the commit it builds.
const NIGHTLY_TAG: &str = "nightly";

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    /// The release's web page — shown as a "View on GitHub" link beside the notes.
    #[serde(default)]
    html_url: String,
    /// The release notes (markdown), shown read-only on the About tab.
    #[serde(default)]
    body: String,
    /// Unpublished draft (only ever visible to authenticated maintainers); skipped
    /// by the beta channel's newest-by-semver scan.
    #[serde(default)]
    draft: bool,
    #[serde(default)]
    assets: Vec<Asset>,
}

#[derive(Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
    #[serde(default)]
    size: u64,
}

/// Query `.../releases/latest` and classify the result relative to this build. A
/// 404 (no release published yet) is [`UpdateState::UpToDate`], not an error. When a
/// newer release exists, offer an in-place install if `asset` is set and present in
/// the release, otherwise offer to open the release page. `Err` only on real
/// network/parse failures.
pub(super) fn latest_release(asset: Option<&str>) -> Result<UpdateState, String> {
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
    let mut response = match ureq::get(&url)
        .header("User-Agent", USER_AGENT)
        .header("Accept", "application/vnd.github+json")
        .call()
    {
        Ok(r) => r,
        // No release yet -> nothing to update to (not a failure).
        Err(ureq::Error::StatusCode(404)) => {
            return Ok(UpdateState::UpToDate {
                current: current().to_string(),
            });
        }
        Err(e) => return Err(e.to_string()),
    };

    let body = response
        .body_mut()
        .read_to_vec()
        .map_err(|e| e.to_string())?;
    let release: Release =
        serde_json::from_slice(&body).map_err(|e| format!("parse release: {e}"))?;
    classify(&release, asset)
}

/// Query `.../releases` (which includes pre-releases, unlike `/releases/latest`) and
/// classify the newest by semver — pre-releases sort below their final version but
/// above the previous patch, so a beta user gets whichever is highest. Empty (no
/// releases) is [`UpdateState::UpToDate`]. This is the `beta` channel.
pub(super) fn latest_beta(asset: Option<&str>) -> Result<UpdateState, String> {
    let url = format!("https://api.github.com/repos/{REPO}/releases?per_page=30");
    let mut response = ureq::get(&url)
        .header("User-Agent", USER_AGENT)
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| e.to_string())?;
    let body = response
        .body_mut()
        .read_to_vec()
        .map_err(|e| e.to_string())?;
    let releases: Vec<Release> =
        serde_json::from_slice(&body).map_err(|e| format!("parse releases: {e}"))?;

    // Highest semver among non-draft releases (pre-releases included); tags that
    // aren't valid semver are skipped rather than failing the whole check.
    let newest = releases
        .iter()
        .filter(|r| !r.draft)
        .filter_map(|r| {
            let v = semver::Version::parse(r.tag_name.trim_start_matches('v')).ok()?;
            Some((v, r))
        })
        .max_by(|a, b| a.0.cmp(&b.0));
    match newest {
        Some((_, release)) => classify(release, asset),
        None => Ok(UpdateState::UpToDate {
            current: current().to_string(),
        }),
    }
}

/// Compare one release against this build: [`UpdateState::UpToDate`] if its tag isn't
/// strictly newer than [`CARGO_PKG_VERSION`], otherwise an [`Offer::Install`] (when
/// `asset` is present in the release) or [`Offer::Open`] (fall back to the page).
fn classify(release: &Release, asset: Option<&str>) -> Result<UpdateState, String> {
    let tag = release.tag_name.trim_start_matches('v').to_string();
    let latest = semver::Version::parse(&tag).map_err(|e| format!("bad tag `{tag}`: {e}"))?;
    let current_ver = semver::Version::parse(current()).map_err(|e| e.to_string())?;
    if latest <= current_ver {
        return Ok(UpdateState::UpToDate {
            current: current().to_string(),
        });
    }
    // Prefer an in-place asset (with its optional checksum sidecar); fall back to
    // opening the release page for manual download.
    let offer = match asset.and_then(|want| find_asset(release, want)) {
        Some(a) => Offer::Install {
            url: a.browser_download_url.clone(),
            size: a.size,
            sha256: find_asset(release, &format!("{}.sha256", a.name))
                .and_then(|s| fetch_sha256(&s.browser_download_url)),
        },
        None => Offer::Open,
    };
    // The notes (release body) and page back the About tab's read-only preview and
    // "View on GitHub" link; both are shown regardless of the install path.
    let notes = {
        let body = release.body.trim();
        (!body.is_empty()).then(|| body.to_string())
    };
    let page = (!release.html_url.is_empty()).then(|| release.html_url.clone());
    Ok(UpdateState::Available {
        version: tag,
        notes,
        page,
        offer,
    })
}

fn find_asset<'a>(release: &'a Release, name: &str) -> Option<&'a Asset> {
    release.assets.iter().find(|a| a.name == name)
}

/// The rolling `nightly` pre-release. Its tag carries no version, so the commit
/// recorded in the body is what [`RETSURF_GIT_HASH`] is compared against. A 404
/// (none published yet) is not an error.
pub(super) fn latest_nightly(asset: Option<&str>) -> Result<UpdateState, String> {
    let url = format!("https://api.github.com/repos/{REPO}/releases/tags/{NIGHTLY_TAG}");
    let mut response = match ureq::get(&url)
        .header("User-Agent", USER_AGENT)
        .header("Accept", "application/vnd.github+json")
        .call()
    {
        Ok(r) => r,
        Err(ureq::Error::StatusCode(404)) => {
            return Ok(UpdateState::UpToDate {
                current: current_sha().to_string(),
            });
        }
        Err(e) => return Err(e.to_string()),
    };
    let body = response
        .body_mut()
        .read_to_vec()
        .map_err(|e| e.to_string())?;
    let release: Release =
        serde_json::from_slice(&body).map_err(|e| format!("parse release: {e}"))?;

    let Some(sha) = nightly_commit(&release.body) else {
        return Err("nightly release records no commit".to_string());
    };
    // A local build has no git hash and so never matches — the channel then always
    // offers the published nightly, which is what a developer wants.
    if current_sha() != "unknown" && sha.starts_with(current_sha()) {
        return Ok(UpdateState::UpToDate {
            current: current_sha().to_string(),
        });
    }

    let offer = match asset.and_then(|want| find_asset(&release, want)) {
        Some(a) => Offer::Install {
            url: a.browser_download_url.clone(),
            size: a.size,
            sha256: find_asset(&release, &format!("{}.sha256", a.name))
                .and_then(|s| fetch_sha256(&s.browser_download_url)),
        },
        None => Offer::Open,
    };
    Ok(UpdateState::Available {
        version: format!("nightly {}", sha.get(..7).unwrap_or(sha)),
        notes: {
            let body = release.body.trim();
            (!body.is_empty()).then(|| body.to_string())
        },
        page: (!release.html_url.is_empty()).then(|| release.html_url.clone()),
        offer,
    })
}

/// The commit a nightly was built from — the workflow writes it as a `commit: <sha>`
/// line in the release body, there being no other field that survives a moved tag.
fn nightly_commit(body: &str) -> Option<&str> {
    body.lines()
        .find_map(|l| l.trim().strip_prefix("commit:"))
        .map(str::trim)
        .filter(|s| !s.is_empty())
}

fn current_sha() -> &'static str {
    env!("RETSURF_GIT_HASH")
}

/// Fetch the small `.sha256` sidecar and take its first field — `sha256sum` writes
/// `<hex>  <filename>`. Best-effort: any failure or malformed body yields `None`,
/// so the install falls back to HTTPS trust alone.
fn fetch_sha256(url: &str) -> Option<String> {
    let body = ureq::get(url)
        .header("User-Agent", USER_AGENT)
        .call()
        .ok()?
        .body_mut()
        .read_to_string()
        .ok()?;
    let hex = body.split_whitespace().next()?.to_ascii_lowercase();
    (hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit())).then_some(hex)
}

fn current() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Deserialize a release the way the live query does, so the tests cover the
    /// `body` field too. `99.0.0` is always newer than this crate; leaving out any
    /// `.sha256` sidecar asset keeps [`classify`] off the network.
    fn release(v: serde_json::Value) -> Release {
        serde_json::from_value(v).expect("valid release json")
    }

    /// A newer release with notes and a matching asset -> an in-place install offer
    /// carrying the notes and page (sha256 stays `None`: no sidecar asset).
    #[test]
    fn classify_install_carries_notes_and_page() {
        let r = release(json!({
            "tag_name": "v99.0.0",
            "html_url": "https://example.com/tag/v99.0.0",
            "body": "New in this release\n- one\n- two",
            "assets": [
                {"name": "retsurf-linux-x86_64.zip",
                 "browser_download_url": "https://example.com/a.zip", "size": 123}
            ]
        }));
        match classify(&r, Some("retsurf-linux-x86_64.zip")).unwrap() {
            UpdateState::Available {
                version,
                notes,
                page,
                offer,
            } => {
                assert_eq!(version, "99.0.0");
                assert_eq!(notes.as_deref(), Some("New in this release\n- one\n- two"));
                assert_eq!(page.as_deref(), Some("https://example.com/tag/v99.0.0"));
                match offer {
                    Offer::Install { url, size, sha256 } => {
                        assert_eq!(url, "https://example.com/a.zip");
                        assert_eq!(size, 123);
                        assert_eq!(sha256, None);
                    }
                    Offer::Open => panic!("expected an in-place install offer"),
                }
            }
            _ => panic!("expected Available"),
        }
    }

    /// A newer release whose asset we can't install in place still surfaces its
    /// notes + page, just behind an "open the page" offer.
    #[test]
    fn classify_open_still_carries_notes_and_page() {
        let r = release(json!({
            "tag_name": "v99.0.0",
            "html_url": "https://example.com/tag/v99.0.0",
            "body": "notes",
            "assets": []
        }));
        match classify(&r, None).unwrap() {
            UpdateState::Available {
                notes, page, offer, ..
            } => {
                assert_eq!(notes.as_deref(), Some("notes"));
                assert_eq!(page.as_deref(), Some("https://example.com/tag/v99.0.0"));
                assert!(matches!(offer, Offer::Open));
            }
            _ => panic!("expected Available"),
        }
    }

    /// A blank release body (whitespace only) becomes `None`, not an empty preview.
    #[test]
    fn classify_blank_body_is_none() {
        let r = release(json!({"tag_name": "v99.0.0", "html_url": "https://x", "body": "  \n\t "}));
        match classify(&r, None).unwrap() {
            UpdateState::Available { notes, .. } => assert_eq!(notes, None),
            _ => panic!("expected Available"),
        }
    }

    /// A release no newer than this build is `UpToDate`, never `Available`.
    #[test]
    fn classify_older_is_up_to_date() {
        let r = release(json!({"tag_name": "v0.0.1", "html_url": "https://x", "body": "old"}));
        assert!(matches!(
            classify(&r, None).unwrap(),
            UpdateState::UpToDate { .. }
        ));
    }

    /// The body the nightly workflow writes, prose and all.
    #[test]
    fn nightly_commit_reads_the_recorded_sha() {
        let body = "Automated build from the latest commit on main. Expect bugs.\n\n\
                    commit: 0123456789abcdef0123456789abcdef01234567\n";
        assert_eq!(
            nightly_commit(body),
            Some("0123456789abcdef0123456789abcdef01234567")
        );
    }

    /// A body without the line is a workflow change, not something to guess around.
    #[test]
    fn nightly_commit_absent_or_empty_is_none() {
        assert_eq!(nightly_commit("no marker here"), None);
        assert_eq!(nightly_commit("commit:   \n"), None);
    }
}
