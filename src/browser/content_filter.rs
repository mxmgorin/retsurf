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
use std::collections::{HashMap, HashSet};
use url::Url;

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

/// Images allowed on one tab under the per-page cap, hashed and bucketed by the
/// load's referrer — the closest thing to a frame identity — so an iframe
/// cannot spend the page's budget.
#[derive(Default)]
pub struct PageImages(HashMap<u64, HashSet<u64>>);

impl PageImages {
    /// Forget everything (a top-level navigation).
    pub fn clear(&mut self) {
        self.0.clear();
    }

    /// Whether this image may load under `cap` distinct images per document.
    /// A repeat of an allowed one always may: Servo fetches once per element,
    /// and pages reuse one spacer gif dozens of times.
    pub fn allow(&mut self, url: &Url, referrer: Option<&Url>, cap: usize) -> bool {
        let bucket = self.0.entry(owner_key(referrer)).or_default();
        let key = image_key(url);
        if bucket.contains(&key) {
            return true;
        }
        if bucket.len() >= cap {
            return false;
        }
        bucket.insert(key);
        true
    }
}

/// Which document a subresource load belongs to. A load with no referrer shares
/// the one bucket, which is what the main document gets.
fn owner_key(referrer: Option<&Url>) -> u64 {
    referrer.map_or(0, image_key)
}

/// Image identity for the cap. Hashed, not stored verbatim: inline `data:`
/// images run to hundreds of kilobytes each. A collision costs one image's slot.
fn image_key(url: &Url) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::hash::DefaultHasher::new();
    url.as_str().hash(&mut hasher);
    hasher.finish()
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
        let cfg = DataSavingConfig {
            max_images_per_page: 48,
            ..DataSavingConfig::default()
        };
        assert_eq!(ContentFilter::from_config(&cfg).image_cap(), Some(48));
    }

    /// The cap counts distinct images per referrer bucket: repeats stay free,
    /// and an iframe's images cannot spend the page's budget.
    #[test]
    fn page_images_cap_is_per_bucket_and_repeats_are_free() {
        let mut images = PageImages::default();
        let page = Url::parse("https://a.example/").unwrap();
        let frame = Url::parse("https://b.example/frame").unwrap();
        let img = |n: u32| Url::parse(&format!("https://cdn.example/{n}.png")).unwrap();

        assert!(images.allow(&img(1), Some(&page), 2));
        assert!(images.allow(&img(2), Some(&page), 2));
        assert!(!images.allow(&img(3), Some(&page), 2));
        assert!(images.allow(&img(1), Some(&page), 2));
        assert!(images.allow(&img(3), Some(&frame), 2));

        images.clear();
        assert!(images.allow(&img(3), Some(&page), 2));
    }
}
