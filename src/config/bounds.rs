//! Numeric ranges for the clampable config fields, owned here in the config
//! layer and read by both `AppConfig::sanitize` (which clamps a hand-edited
//! `retsurf.toml`) and the settings GUI's sliders ([`crate::overlay::settings`]).
//! Defining each range once keeps the two from drifting — before, every min/max
//! was spelled in both places by hand.
//!
//! Step size and decimal places stay in the GUI (presentation-only). The GUI's
//! `Kind::Int`/`Kind::Float` already speak `i64`/`f64`, so the ranges are stored
//! at those widths; `sanitize` casts each to the field's own type at the call
//! site (see the `fix_*` helpers in [`crate::config`]).

/// Inclusive `[min, max]` range for an integer config field. `sanitize` casts
/// these to the field's own integer width (all values are small and
/// non-negative, so the cast is exact).
pub struct IntBounds {
    pub min: i64,
    pub max: i64,
}

/// Inclusive `[min, max]` range plus the out-of-range `default` for a float
/// config field. `sanitize` substitutes `default` for a non-finite value
/// (NaN/inf) and clamps everything else.
pub struct FloatBounds {
    pub min: f64,
    pub max: f64,
    pub default: f64,
}

// Browser
pub const PAGE_ZOOM: FloatBounds = FloatBounds { min: 0.3, max: 3.0, default: 1.0 };

// Display
pub const WIDTH: IntBounds = IntBounds { min: 160, max: 3840 };
pub const HEIGHT: IntBounds = IntBounds { min: 144, max: 2160 };
pub const CURSOR_LINGER_MS: IntBounds = IntBounds { min: 0, max: 10_000 };

// Input
pub const DEADZONE: FloatBounds = FloatBounds { min: 0.0, max: 0.9, default: 0.25 };
pub const CURSOR_SPEED: FloatBounds = FloatBounds { min: 100.0, max: 3000.0, default: 600.0 };
pub const SCROLL_SPEED: FloatBounds = FloatBounds { min: 100.0, max: 5000.0, default: 1600.0 };
pub const TRIGGER_THRESHOLD: FloatBounds = FloatBounds { min: 0.1, max: 0.9, default: 0.5 };
pub const OSK_NAV_THRESHOLD: FloatBounds = FloatBounds { min: 0.1, max: 0.9, default: 0.5 };
pub const OSK_NAV_INITIAL_DELAY_MS: IntBounds = IntBounds { min: 50, max: 1000 };
pub const OSK_NAV_REPEAT_MS: IntBounds = IntBounds { min: 20, max: 500 };
pub const HOLD_MS: IntBounds = IntBounds { min: 100, max: 2000 };

// Content
pub const HISTORY_MAX: IntBounds = IntBounds { min: 0, max: 1000 };
pub const ADBLOCK_UPDATE_DAYS: IntBounds = IntBounds { min: 0, max: 90 };

// Performance
pub const LAYOUT_THREADS: IntBounds = IntBounds { min: 0, max: 8 };
pub const WORKER_POOL_MAX: IntBounds = IntBounds { min: 0, max: 16 };
