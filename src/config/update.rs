//! Self-update settings (`[update]` in the config). See [`crate::update`].

use crate::config::token_enum::token_enum;
use serde::{Deserialize, Serialize};

token_enum! {
    /// Which builds the updater checks for.
    pub enum Channel {
        default Release;
        /// Tagged GitHub releases, stable only (the default).
        Release => "release", "Stable releases",
        /// Tagged GitHub releases including pre-releases (highest semver wins) — a
        /// beta channel. Same public assets as `release`, no token needed.
        Beta => "beta", "Beta (pre-releases)",
        /// Per-commit `main` CI artifacts (a dev/nightly channel). Needs a GitHub
        /// token — GitHub requires auth to download Actions artifacts, even on
        /// public repos. See [`UpdateConfig::token`] / `RETSURF_GITHUB_TOKEN`.
        Ci => "ci", "CI builds (dev)",
    }
}

/// `[update]` config. Off the beaten path on purpose: the CI channel installs
/// unsigned per-commit builds, so it stays opt-in via a hand-edited config.
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct UpdateConfig {
    /// `release` (default) or `ci`. See [`Channel`].
    pub channel: Channel,
    /// Check for a newer build in the background at startup (throttled to at most
    /// once a day; see [`crate::update::Updater::auto_check`]). On by default; set
    /// `false` to only ever check from the Settings -> About tab.
    pub auto_check: bool,
    /// GitHub token (a fine-grained PAT with `actions:read`) for the `ci` channel —
    /// GitHub requires auth to list/download Actions artifacts. Prefer the
    /// `RETSURF_GITHUB_TOKEN` env var over writing a secret to disk; this field is a
    /// fallback. Empty by default and unused by the `release` channel.
    pub token: String,
}

impl Default for UpdateConfig {
    fn default() -> Self {
        // `auto_check` defaults on — the whole point is discovery without a manual
        // trip to the About tab; a derived `Default` would wrongly start it `false`.
        Self {
            channel: Channel::default(),
            auto_check: true,
            token: String::new(),
        }
    }
}

impl UpdateConfig {
    /// The effective GitHub token for the CI channel: `RETSURF_GITHUB_TOKEN` if set
    /// and non-empty, else the config `token`. `None` when neither is set (the CI
    /// channel then reports that it needs one).
    pub fn resolve_token(&self) -> Option<String> {
        let from_env = std::env::var("RETSURF_GITHUB_TOKEN").ok();
        let raw = from_env
            .filter(|t| !t.trim().is_empty())
            .unwrap_or_else(|| self.token.clone());
        let raw = raw.trim().to_string();
        (!raw.is_empty()).then_some(raw)
    }
}
