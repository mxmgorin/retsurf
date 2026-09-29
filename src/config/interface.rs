use super::HomeStyle;
use crate::config::token_enum::token_enum;
use serde::{Deserialize, Serialize};

/// The browser's own chrome (`[interface]` in the config): its scale, the
/// toolbar, the start page and cursor-visibility timing.
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct InterfaceConfig {
    /// UI zoom over the scale the panel's own size asks for, so one setting means
    /// the same thing on a handheld and on a desktop window. The page follows it.
    pub scale: f32,
    /// How long the virtual cursor stays visible after the last movement, in ms.
    /// It hides when idle (nothing to hover) but lingers so you can see where it
    /// landed before clicking.
    pub cursor_linger_ms: u64,
    /// Which edge the toolbar (address bar + nav buttons) sits on.
    pub toolbar_position: ToolbarPosition,
    /// Hide the toolbar while scrolling down, reveal it on scrolling up. Floats
    /// over the page on either edge: a strip that came and went would resize the
    /// web view, reflowing the page mid-scroll.
    pub toolbar_autohide: bool,
    /// Show site icons on tabs, bookmarks, history and the speed dial. Servo
    /// fetches them either way; this keeps a 32 px copy per tab and per host.
    pub page_icons: bool,
    /// What heads the start page above its search field.
    pub home_style: HomeStyle,
}

impl Default for InterfaceConfig {
    fn default() -> Self {
        Self {
            scale: 1.0,
            cursor_linger_ms: 1500,
            toolbar_position: ToolbarPosition::Top,
            toolbar_autohide: false,
            page_icons: true,
            home_style: HomeStyle::Banner,
        }
    }
}

token_enum! {
    /// Which window edge the toolbar sits on. Serializes to `"top"` / `"bottom"`
    /// in TOML; an unknown value falls back to `Top`.
    pub enum ToolbarPosition {
        default Top;
        /// At the top of the window, above the page (the default).
        Top => "top", "Top",
        /// At the bottom of the window, below the page — handy when the device's
        /// face buttons sit low and a top bar is a reach.
        Bottom => "bottom", "Bottom",
    }
}
