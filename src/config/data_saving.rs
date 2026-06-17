use serde::{Deserialize, Serialize};

/// Lightweight "data saving" mode (`[data_saving]` in the config): skip whole
/// subresource categories to cut bandwidth and memory. Each is blocked at the
/// network level like the ad blocker, so pages fail soft, and all apply live.
/// See [`crate::browser::content_filter`].
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct DataSavingConfig {
    /// Skip image subresource loads (`<img>`, CSS backgrounds, favicons).
    pub block_images: bool,
    /// Skip audio/video/track media loads.
    pub block_media: bool,
    /// Skip web-font downloads — pages fall back to the bundled system fonts.
    pub block_fonts: bool,
}
