//! In-app self-update. A manual "Check for updates" on the Settings -> About tab
//! queries GitHub for a newer build on the configured channel (stable releases,
//! `beta` pre-releases, or `ci` per-commit artifacts); what it can then do depends
//! on the detected install [`Kind`]:
//! - **PortMaster** (handheld port): download the release zip, verify, atomically
//!   swap the launcher + three per-core binaries in place, quit-to-relaunch.
//! - **Single-binary desktop** (Linux x86_64 / aarch64): download the release zip,
//!   verify, rename the one binary over the running exe, quit-to-relaunch.
//! - **Manual** (Windows, macOS, Android, or a read-only install): no in-place
//!   swap — offer to open the release page so the user can download it.
//!
//! Mirrors the [`crate::data::downloads`] manager: state lives behind an
//! `Arc<Mutex<_>>`, worker threads mutate it and wake the loop with
//! [`UserEvent::UpdateProgress`], and the UI reads [`Updater::snapshot`] each frame.

mod github;
mod install;

use crate::config::{Channel, UpdateConfig};
use crate::event::user::{UserEvent, UserEventSender};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

/// The repo the updater queries and pulls releases from.
const REPO: &str = "mxmgorin/retsurf";
/// The PortMaster release asset (see the ARM CI package job).
const PORTMASTER_ASSET: &str = "retsurf-portmaster.zip";
/// User-Agent for the GitHub API (it 403s requests without one).
const USER_AGENT: &str = concat!("retsurf/", env!("CARGO_PKG_VERSION"));

/// The self-update lifecycle. Cloned out through [`Updater::snapshot`] for the UI.
#[derive(Clone)]
pub enum UpdateState {
    Idle,
    Checking,
    UpToDate { current: String },
    Available { version: String, offer: Offer },
    Downloading { received: u64, total: u64 },
    Installing,
    Installed { version: String },
    Error(String),
}

/// How the About tab offers an available update.
#[derive(Clone)]
pub enum Offer {
    /// In-place install (PortMaster / single-binary desktop): the release asset.
    Install {
        url: String,
        size: u64,
        sha256: Option<String>,
    },
    /// No in-place path on this install — open the release page to download.
    Open { page: String },
}

/// The detected install kind, resolved once from the live process.
#[derive(Clone)]
pub(crate) enum Kind {
    /// PortMaster port: the launcher + three per-core binaries live in the gamedir.
    PortMaster { gamedir: PathBuf, launcher: PathBuf },
    /// A single replaceable binary (Linux x86_64 / aarch64 desktop). `asset` is the
    /// release zip and `bin` the binary's name inside it.
    Single {
        exe: PathBuf,
        asset: &'static str,
        bin: &'static str,
    },
    /// No in-place update path (Windows, macOS, Android, read-only installs): the
    /// About tab can still check and open the release page.
    Manual,
}

impl Kind {
    /// The release asset to download for an in-place install, if this kind supports one.
    fn asset(&self) -> Option<&str> {
        match self {
            Kind::PortMaster { .. } => Some(PORTMASTER_ASSET),
            Kind::Single { asset, .. } => Some(asset),
            Kind::Manual => None,
        }
    }

    /// The CI artifact name for an in-place install — the release asset without its
    /// `.zip` suffix (upload-artifact stores the same tree under that bare name).
    fn artifact(&self) -> Option<&str> {
        self.asset().and_then(|a| a.strip_suffix(".zip"))
    }
}

pub struct Updater {
    state: Arc<Mutex<UpdateState>>,
    kind: Kind,
    /// Which builds to check (stable releases or CI artifacts).
    channel: Channel,
    /// GitHub token for the CI channel (resolved from env/config once at startup).
    /// Held here, never in [`UpdateState`], so it stays out of the UI snapshot.
    token: Option<String>,
}

impl Updater {
    pub fn new(cfg: &UpdateConfig) -> Self {
        Self {
            state: Arc::new(Mutex::new(UpdateState::Idle)),
            kind: resolve_kind(),
            channel: cfg.channel,
            token: cfg.resolve_token(),
        }
    }

    /// Snapshot the current state for the UI (lock, clone the small enum, release).
    pub fn snapshot(&self) -> UpdateState {
        self.state.lock().unwrap().clone()
    }

    /// Query GitHub for the latest release on a background thread. Works on every
    /// kind (Manual just gets an "open the page" offer); a no-op while mid-flight.
    pub fn check(&self, sender: &UserEventSender) {
        {
            let mut state = self.state.lock().unwrap();
            if matches!(
                *state,
                UpdateState::Checking | UpdateState::Downloading { .. } | UpdateState::Installing
            ) {
                return;
            }
            *state = UpdateState::Checking;
        }
        sender.send(UserEvent::UpdateProgress);
        let state = self.state.clone();
        let sender = sender.clone();
        let channel = self.channel;
        let token = self.token.clone();
        let asset = self.kind.asset().map(str::to_string);
        let artifact = self.kind.artifact().map(str::to_string);
        std::thread::spawn(move || {
            let result = match channel {
                Channel::Release => github::latest_release(asset.as_deref()),
                Channel::Beta => github::latest_beta(asset.as_deref()),
                Channel::Ci => match (artifact, token) {
                    (Some(artifact), Some(token)) => github::latest_ci(&artifact, &token),
                    (None, _) => Err("CI updates aren't available for this install type".to_string()),
                    (_, None) => {
                        Err("Set RETSURF_GITHUB_TOKEN (or [update] token) for the CI channel".to_string())
                    }
                },
            };
            publish(&state, result.unwrap_or_else(UpdateState::Error), &sender);
        });
    }

    /// Download + verify + swap the available release on a background thread. Only
    /// valid from an [`Offer::Install`]; the manual "Download" path uses `OpenLink`.
    pub fn install(&self, sender: &UserEventSender) {
        let (version, url, sha256) = {
            let state = self.state.lock().unwrap();
            match &*state {
                UpdateState::Available {
                    version,
                    offer: Offer::Install { url, sha256, .. },
                } => (version.clone(), url.clone(), sha256.clone()),
                _ => return,
            }
        };
        // CI artifact downloads hit the GitHub API and need the token; release asset
        // URLs are public (and must NOT carry it). ureq drops the Authorization header
        // when it follows the 302 to blob storage (redirect_auth_headers = Never).
        let auth = (self.channel == Channel::Ci).then(|| self.token.clone()).flatten();
        let kind = self.kind.clone();
        let state = self.state.clone();
        let sender = sender.clone();
        std::thread::spawn(move || {
            install::run(&kind, &version, &url, sha256.as_deref(), auth.as_deref(), &state, &sender);
        });
    }
}

/// Store a new state and wake the main loop so the About tab repaints.
fn publish(state: &Mutex<UpdateState>, next: UpdateState, sender: &UserEventSender) {
    *state.lock().unwrap() = next;
    sender.send(UserEvent::UpdateProgress);
}

/// Detect which install kind we are, from the live process.
fn resolve_kind() -> Kind {
    if let Some((gamedir, launcher)) = portmaster_paths() {
        return Kind::PortMaster { gamedir, launcher };
    }
    // A single-binary desktop target whose directory we can actually write to.
    if let Some((asset, bin)) = single_binary_asset() {
        if let Some(exe) = replaceable_exe() {
            return Kind::Single { exe, asset, bin };
        }
    }
    Kind::Manual
}

/// PortMaster gate: Linux (Android's target_os is "android", so a plain linux check
/// excludes it), the launcher set `RETSURF_DATA_DIR`, and a sibling `Retsurf.sh`
/// exists — the last part is what tells a real port from a desktop user who set
/// `RETSURF_DATA_DIR` for a portable profile (see `src/config/paths.rs`). Returns
/// `(gamedir, launcher)`.
fn portmaster_paths() -> Option<(PathBuf, PathBuf)> {
    if !cfg!(target_os = "linux") {
        return None;
    }
    std::env::var_os("RETSURF_DATA_DIR")?;
    // The launcher execs an absolute `$GAMEDIR/retsurf.a53`, so the exe's parent is
    // the gamedir and its parent holds Retsurf.sh.
    let exe = std::env::current_exe().ok()?.canonicalize().ok()?;
    let gamedir = exe.parent()?.to_path_buf();
    let launcher = gamedir.parent()?.join("Retsurf.sh");
    launcher.is_file().then_some((gamedir, launcher))
}

/// The release asset + in-zip binary name for a single-binary desktop install, or
/// `None` on targets we don't ship a standalone binary for (Windows/macOS/Android
/// go through the Manual "open the page" path).
fn single_binary_asset() -> Option<(&'static str, &'static str)> {
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    {
        Some(("retsurf-linux-x86_64.zip", "retsurf"))
    }
    #[cfg(all(target_os = "linux", target_arch = "aarch64"))]
    {
        Some(("retsurf-linux-aarch64.zip", "retsurf-linux-aarch64"))
    }
    #[cfg(not(any(
        all(target_os = "linux", target_arch = "x86_64"),
        all(target_os = "linux", target_arch = "aarch64"),
    )))]
    {
        None
    }
}

/// The canonical current-exe path if its directory is writable (so an in-place swap
/// can work), else `None` — a read-only/system install falls back to Manual. Probes
/// with a temp file, the robust cross-filesystem writability check.
fn replaceable_exe() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?.canonicalize().ok()?;
    let dir = exe.parent()?;
    let probe = dir.join(format!(".retsurf-update-probe-{}", std::process::id()));
    std::fs::File::create(&probe).ok()?;
    let _ = std::fs::remove_file(&probe);
    Some(exe)
}
