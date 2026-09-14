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
        /// The rolling `nightly` pre-release, rebuilt from `main` once a day. Its
        /// assets are public, so no token — unlike the per-commit Actions artifacts
        /// this replaced, which GitHub serves only to authenticated callers.
        Nightly => "nightly" | "ci", "Nightly builds (dev)",
    }
}

/// `[update]` config. Off the beaten path on purpose: the nightly channel installs
/// unsigned builds straight off `main`, so it stays opt-in via the config.
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct UpdateConfig {
    /// `release` (default), `beta` or `nightly`. See [`Channel`].
    pub channel: Channel,
    /// Check for a newer build in the background at startup (throttled to at most
    /// once a day; see [`crate::update::Updater::auto_check`]). On by default; set
    /// `false` to only ever check from the Settings -> About tab.
    pub auto_check: bool,
}

impl Default for UpdateConfig {
    fn default() -> Self {
        // `auto_check` defaults on — the whole point is discovery without a manual
        // trip to the About tab; a derived `Default` would wrongly start it `false`.
        Self {
            channel: Channel::default(),
            auto_check: true,
        }
    }
}
