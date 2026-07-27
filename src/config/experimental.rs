use serde::{Deserialize, Serialize};

/// Servo experimental web-platform features (`[experimental]`). retsurf turns
/// these on after startup — Servo ships them off but the modern web needs them.
/// The 12 bools are the source of truth; the settings "Web features" preset
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
    /// web fonts, Intersection/ResizeObserver); graphics and niche DOM APIs off.
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
        let all = ExperimentalConfig {
            webgl2: true,
            webgpu: true,
            offscreen_canvas: true,
            grid: true,
            columns: true,
            container_queries: true,
            fontface: true,
            intersection_observer: true,
            resize_observer: true,
            notification: true,
            async_clipboard: true,
            permissions: true,
        };
        let none = ExperimentalConfig {
            webgl2: false,
            webgpu: false,
            offscreen_canvas: false,
            grid: false,
            columns: false,
            container_queries: false,
            fontface: false,
            intersection_observer: false,
            resize_observer: false,
            notification: false,
            async_clipboard: false,
            permissions: false,
        };
        match self {
            ExperimentalPreset::Full => all,
            ExperimentalPreset::Off => none,
            // Layout + compat essentials; graphics + niche DOM APIs off.
            ExperimentalPreset::Minimal => ExperimentalConfig {
                grid: true,
                columns: true,
                container_queries: true,
                fontface: true,
                intersection_observer: true,
                resize_observer: true,
                ..none
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
        assert_eq!(ExperimentalPreset::from_value(" OFF "), ExperimentalPreset::Off);
        assert_eq!(ExperimentalPreset::from_value("Full"), ExperimentalPreset::Full);
        assert_eq!(
            ExperimentalPreset::from_value("nonsense"),
            ExperimentalPreset::Balanced
        );
    }
}
