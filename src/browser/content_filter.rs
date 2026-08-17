//! Lightweight mode: block whole categories of subresource by their fetch
//! destination — images, media, web fonts — to save bandwidth and memory on a
//! handheld. Servo has no pref to disable image loading, but its
//! `load_web_resource` delegate hook (see [`crate::browser::delegate`]) reports
//! each load's [`Destination`], so the unwanted ones get the same empty-200
//! interception the ad blocker uses and fail soft. Driven by the `block_*`
//! fields of [`DataSavingConfig`]; the running app re-reads them on every
//! settings save, so it toggles live.

use crate::config::DataSavingConfig;
use content_security_policy::Destination;

/// Which content categories to block. A `Copy` snapshot of the config's
/// `block_*` flags, cheap enough to live behind a `Cell` and be replaced
/// wholesale when settings change.
#[derive(Clone, Copy, Default)]
pub struct ContentFilter {
    images: bool,
    media: bool,
    fonts: bool,
    max_images: usize,
}

impl ContentFilter {
    pub fn from_config(cfg: &DataSavingConfig) -> Self {
        Self {
            images: cfg.block_images,
            media: cfg.block_media,
            fonts: cfg.block_fonts,
            max_images: cfg.max_images_per_page,
        }
    }

    /// Per-page cap on distinct images, `None` when unlimited. Counted per image,
    /// not per load: a spacer gif reused thirty times must not eat thirty slots.
    /// The allowed set lives on the tab, cleared on each of its navigations.
    pub fn image_cap(&self) -> Option<usize> {
        (self.max_images != 0).then_some(self.max_images)
    }

    /// Whether a load to this destination should be blocked under the current
    /// flags. Unknown/other destinations (documents, scripts, styles, XHR) are
    /// never touched — only the bandwidth-heavy media categories.
    pub fn blocks(&self, destination: Destination) -> bool {
        match destination {
            Destination::Image => self.images,
            Destination::Audio | Destination::Video | Destination::Track => self.media,
            Destination::Font => self.fonts,
            _ => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::DataSavingConfig;

    /// The default config caps nothing: no image limit, no category blocked.
    #[test]
    fn default_lets_images_through() {
        let f = ContentFilter::from_config(&DataSavingConfig::default());
        assert_eq!(f.image_cap(), None);
        assert!(!f.blocks(Destination::Image));
    }

    /// A configured cap is reported as-is; 0 stays unlimited.
    #[test]
    fn configured_image_cap() {
        let cfg = DataSavingConfig { max_images_per_page: 48, ..DataSavingConfig::default() };
        assert_eq!(ContentFilter::from_config(&cfg).image_cap(), Some(48));
    }
}
