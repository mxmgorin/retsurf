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
    /// Cap on distinct images per page (0 = unlimited); beyond it, loads soft-block.
    /// Servo has no lazy-loading, so image-heavy grids can freeze a handheld. Off by
    /// default: ordinary pages run to 80-odd images, and capping them cost under
    /// 3% of resident memory while dropping half the content.
    pub max_images_per_page: usize,
}
