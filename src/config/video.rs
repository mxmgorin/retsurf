use serde::{Deserialize, Serialize};

/// Video playback (`[video]`). H.264-in-MP4 decoded in software (see
/// [`crate::media`]); off makes video files play audio-only, like before.
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct VideoConfig {
    /// Read once at startup, like `[audio] enabled`.
    pub enabled: bool,
}

impl Default for VideoConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}
