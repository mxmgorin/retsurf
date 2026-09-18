use serde::{Deserialize, Serialize};

/// Servo experimental web-platform features (`[experimental]`). retsurf turns
/// these on after startup — Servo ships them off but the modern web needs them.
/// The 14 bools are the source of truth; the settings "Web features" preset
/// ([`ExperimentalPreset`]) is derived from them. Default is `Balanced`
/// (essentials + WebGL2/OffscreenCanvas). Future per-site overrides hang off here.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct ExperimentalConfig {
    /// WebGL 2.0 (`dom_webgl2_enabled`) — GLES 3.0-class 3D.
    pub webgl2: bool,
    /// WebGPU (`dom_webgpu_enabled`) — next-gen GPU API; off in Balanced.
    pub webgpu: bool,
    /// OffscreenCanvas (`dom_offscreen_canvas_enabled`) — canvas off the main thread.
    pub offscreen_canvas: bool,
    /// CSS Grid (`layout_grid_enabled`) — `display: grid`.
    pub grid: bool,
    /// CSS multi-column (`layout_columns_enabled`).
    pub columns: bool,
    /// CSS container queries (`layout_container_queries_enabled`).
    pub container_queries: bool,
    /// Web fonts (`dom_fontface_enabled`) — `@font-face` / FontFace API.
    pub fontface: bool,
    /// IntersectionObserver (`dom_intersection_observer_enabled`) — lazy-load / infinite scroll.
    pub intersection_observer: bool,
    /// ResizeObserver (`dom_resize_observer_enabled`).
    pub resize_observer: bool,
    /// IndexedDB (`dom_indexeddb_enabled`) — the store web apps and games keep
    /// their assets and saves in. Off in Servo, and absent rather than failing,
    /// so a page that needs it dies at startup.
    pub indexeddb: bool,
    /// `navigator.storage` (`dom_storage_manager_api_enabled`) — quota and
    /// persistence. Libraries probe `estimate()` to tell private mode from
    /// normal, so its absence sends them down the wrong branch.
    pub storage_manager: bool,
    /// Web Notifications (`dom_notification_enabled`).
    pub notification: bool,
    /// Async Clipboard API (`dom_async_clipboard_enabled`).
    pub async_clipboard: bool,
    /// Permissions API (`dom_permissions_enabled`).
    pub permissions: bool,
}

impl Default for ExperimentalConfig {
    fn default() -> Self {
        ExperimentalPreset::Balanced.features()
    }
}

/// A named bundle of experimental features (settings "Web features" row). Derived
/// from [`ExperimentalConfig`]'s bools, not stored; `Custom` = matches no preset.
/// Hand-rolled rather than `token_enum!`: `Custom` is derived-only.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExperimentalPreset {
    /// All features off — most stable, but breaks most modern sites.
    Off,
    /// Layout + compatibility essentials only (Grid, columns, container queries,
    /// web fonts, Intersection/ResizeObserver, IndexedDB, navigator.storage);
    /// graphics and niche DOM APIs off.
    Minimal,
    /// Handheld default: the essentials plus WebGL2 + OffscreenCanvas.
    Balanced,
    /// Every feature on — maximum site compatibility, heaviest.
    Full,
    /// Derived: the bools match no named preset.
    Custom,
}

impl ExperimentalPreset {
    /// `(label, token)` for the settings Choice; `Custom` is excluded (derived only).
    pub const CHOICES: &'static [(&'static str, &'static str)] = &[
        ("Off", "off"),
        ("Minimal", "minimal"),
        ("Balanced", "balanced"),
        ("Full", "full"),
    ];

    /// The real presets `detect` can return, in the order it tries them.
    const NAMED: [ExperimentalPreset; 4] = [
        ExperimentalPreset::Off,
        ExperimentalPreset::Minimal,
        ExperimentalPreset::Balanced,
        ExperimentalPreset::Full,
    ];

    /// UI token; `Custom` is display-only (capitalized), the rest match `from_value`.
    pub fn as_str(self) -> &'static str {
        match self {
            ExperimentalPreset::Off => "off",
            ExperimentalPreset::Minimal => "minimal",
            ExperimentalPreset::Balanced => "balanced",
            ExperimentalPreset::Full => "full",
            ExperimentalPreset::Custom => "Custom",
        }
    }

    /// Parse a preset token; unknown -> `Balanced`. Never yields `Custom`.
    pub fn from_value(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "off" => ExperimentalPreset::Off,
            "minimal" => ExperimentalPreset::Minimal,
            "full" => ExperimentalPreset::Full,
            _ => ExperimentalPreset::Balanced,
        }
    }

    /// The feature set this preset enables (`Custom` maps to `Balanced`).
    pub fn features(self) -> ExperimentalConfig {
        // Every feature at `on` — the one full spelling of the field list, so
        // a new field cannot be missed in one of two copies.
        let uniform = |on: bool| ExperimentalConfig {
            webgl2: on,
            webgpu: on,
            offscreen_canvas: on,
            grid: on,
            columns: on,
            container_queries: on,
            fontface: on,
            intersection_observer: on,
            resize_observer: on,
            indexeddb: on,
            storage_manager: on,
            notification: on,
            async_clipboard: on,
            permissions: on,
        };
        match self {
            ExperimentalPreset::Full => uniform(true),
            ExperimentalPreset::Off => uniform(false),
            // Layout + compat essentials; graphics + niche DOM APIs off.
            ExperimentalPreset::Minimal => ExperimentalConfig {
                grid: true,
                columns: true,
                container_queries: true,
                fontface: true,
                intersection_observer: true,
                resize_observer: true,
                indexeddb: true,
                storage_manager: true,
                ..uniform(false)
            },
            // Handheld default: essentials + the graphics the hardware supports.
            // WebGPU (immature), notifications/permissions/clipboard (low value) off.
            ExperimentalPreset::Balanced | ExperimentalPreset::Custom => ExperimentalConfig {
                webgl2: true,
                offscreen_canvas: true,
                ..ExperimentalPreset::Minimal.features()
            },
        }
    }

    /// Which named preset `exp` matches, or `Custom` if none.
    pub fn detect(exp: &ExperimentalConfig) -> Self {
        Self::NAMED
            .into_iter()
            .find(|p| p.features() == *exp)
            .unwrap_or(ExperimentalPreset::Custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The default config is the Balanced preset: essentials + WebGL2/OffscreenCanvas,
    /// with WebGPU and the niche DOM APIs off.
    #[test]
    fn default_is_balanced() {
        let c = ExperimentalConfig::default();
        assert!(c.grid && c.fontface && c.intersection_observer);
        assert!(c.indexeddb && c.storage_manager);
        assert!(c.webgl2 && c.offscreen_canvas);
        assert!(!c.webgpu && !c.notification && !c.permissions && !c.async_clipboard);
        assert_eq!(ExperimentalPreset::detect(&c), ExperimentalPreset::Balanced);
    }

    /// Every named preset round-trips: applying it then detecting yields itself.
    #[test]
    fn named_presets_round_trip() {
        for p in ExperimentalPreset::NAMED {
            assert_eq!(ExperimentalPreset::detect(&p.features()), p, "{p:?}");
        }
    }

    /// Hand-toggling one feature off a named preset reads back as `Custom`.
    #[test]
    fn hand_toggle_is_custom() {
        let mut c = ExperimentalPreset::Full.features();
        c.grid = false;
        assert_eq!(ExperimentalPreset::detect(&c), ExperimentalPreset::Custom);
    }

    /// Token parse is lenient and unknown tokens fall back to Balanced.
    #[test]
    fn from_value_is_lenient() {
        assert_eq!(
            ExperimentalPreset::from_value(" OFF "),
            ExperimentalPreset::Off
        );
        assert_eq!(
            ExperimentalPreset::from_value("Full"),
            ExperimentalPreset::Full
        );
        assert_eq!(
            ExperimentalPreset::from_value("nonsense"),
            ExperimentalPreset::Balanced
        );
    }
}
