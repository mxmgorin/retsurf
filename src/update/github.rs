//! The GitHub "latest release" query: one blocking `ureq` GET, parsed with
//! `serde_json`, compared against `CARGO_PKG_VERSION` with `semver`.

use super::{Offer, UpdateState, REPO, USER_AGENT};
use serde::Deserialize;

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    /// The release's web page — offered when we can't install in place.
    #[serde(default)]
    html_url: String,
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

    let body = response.body_mut().read_to_vec().map_err(|e| e.to_string())?;
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
    let body = response.body_mut().read_to_vec().map_err(|e| e.to_string())?;
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
        None => Offer::Open {
            page: release.html_url.clone(),
        },
    };
    Ok(UpdateState::Available {
        version: tag,
        offer,
    })
}

fn find_asset<'a>(release: &'a Release, name: &str) -> Option<&'a Asset> {
    release.assets.iter().find(|a| a.name == name)
}

#[derive(Deserialize)]
struct ArtifactList {
    #[serde(default)]
    artifacts: Vec<Artifact>,
}

#[derive(Deserialize)]
struct Artifact {
    #[serde(default)]
    name: String,
    #[serde(default)]
    size_in_bytes: u64,
    #[serde(default)]
    expired: bool,
    /// The GitHub API endpoint that 302s to a short-lived signed download URL.
    #[serde(default)]
    archive_download_url: String,
    #[serde(default)]
    created_at: String,
    #[serde(default)]
    workflow_run: WorkflowRun,
}

#[derive(Deserialize, Default)]
struct WorkflowRun {
    #[serde(default)]
    head_branch: String,
    #[serde(default)]
    head_sha: String,
}

/// Query the repo's Actions artifacts by `name` (authenticated — GitHub requires a
/// token to list/download artifacts even on public repos) and pick the newest
/// non-expired one built from `main`. Compares its commit against this build's
/// [`RETSURF_GIT_HASH`]; a match is [`UpdateState::UpToDate`], otherwise an
/// in-place [`Offer::Install`] whose download URL needs the same token.
pub(super) fn latest_ci(artifact: &str, token: &str) -> Result<UpdateState, String> {
    let url =
        format!("https://api.github.com/repos/{REPO}/actions/artifacts?per_page=100&name={artifact}");
    let mut response = ureq::get(&url)
        .header("User-Agent", USER_AGENT)
        .header("Accept", "application/vnd.github+json")
        .header("Authorization", format!("Bearer {token}"))
        .call()
        .map_err(ci_error)?;
    let body = response.body_mut().read_to_vec().map_err(|e| e.to_string())?;
    let list: ArtifactList =
        serde_json::from_slice(&body).map_err(|e| format!("parse artifacts: {e}"))?;

    let latest = list
        .artifacts
        .into_iter()
        .filter(|a| !a.expired && a.name == artifact && a.workflow_run.head_branch == "main")
        .max_by(|a, b| a.created_at.cmp(&b.created_at));
    let Some(art) = latest else {
        // No usable artifact (all expired, or none from main yet).
        return Ok(UpdateState::UpToDate {
            current: current_sha().to_string(),
        });
    };

    // Already running this commit? (A local build with no git info never matches, so
    // the dev channel always offers the newest CI build there — intended.)
    if current_sha() != "unknown" && art.workflow_run.head_sha.starts_with(current_sha()) {
        return Ok(UpdateState::UpToDate {
            current: current_sha().to_string(),
        });
    }

    let short = art
        .workflow_run
        .head_sha
        .get(..7)
        .unwrap_or(&art.workflow_run.head_sha)
        .to_string();
    Ok(UpdateState::Available {
        version: format!("main {short}"),
        offer: Offer::Install {
            url: art.archive_download_url,
            size: art.size_in_bytes,
            // Artifacts have no checksum sidecar; the token + TLS are the trust.
            sha256: None,
        },
    })
}

/// Friendlier message for the common CI auth failures.
fn ci_error(e: ureq::Error) -> String {
    match e {
        ureq::Error::StatusCode(401 | 403) => {
            "GitHub rejected the token (needs a valid token with actions:read)".to_string()
        }
        other => other.to_string(),
    }
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
