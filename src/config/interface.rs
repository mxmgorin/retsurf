use crate::config::token_enum::token_enum;
use serde::{Deserialize, Serialize};

/// The browser's own chrome (`[interface]` in the config): its scale, the
/// toolbar and cursor-visibility timing.
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct InterfaceConfig {
    /// UI zoom over the scale the panel's own size asks for, so one setting means
    /// the same thing on a handheld and on a desktop window. The page follows it.
    pub scale: f32,
    /// The chrome's colors: the start page, the toolbar and every overlay.
    pub theme: UiTheme,
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
    /// Show site icons on tabs, bookmarks, history and the start page. Servo
    /// fetches them either way; this keeps a 32 px copy per tab and per host.
    pub page_icons: bool,
    /// Name what the buttons do at the foot of the menus, the settings, the
    /// start page and the edge strips, and the shoulders beside its filter tabs.
    pub button_hints: bool,
    /// How the lists show the open tabs: rows, or cards with a picture of each
    /// page, kept from when it was last on screen.
    pub tab_layout: TabLayout,
}

impl Default for InterfaceConfig {
    fn default() -> Self {
        Self {
            scale: 1.0,
            theme: UiTheme::Dusk,
            cursor_linger_ms: 1500,
            toolbar_position: ToolbarPosition::Top,
            toolbar_autohide: false,
            page_icons: true,
            button_hints: true,
            tab_layout: TabLayout::List,
        }
    }
}

pub use tab_layout::TabLayout;
pub use ui_theme::UiTheme;

// Own modules: a file holds one `token_enum!`, whose tests take a fixed name.
mod ui_theme {
    use crate::config::token_enum::token_enum;

    token_enum! {
        /// The chrome's palette. Serializes to its lowercase name in TOML; an unknown
        /// value falls back to `Dusk`.
        pub enum UiTheme {
            default Dusk;
            /// The brand's dark slate.
            Night => "night", "Night",
            /// Black under everything, for OLED screens.
            Oled => "oled", "OLED",
            /// Night warmed toward plum (the default).
            Dusk => "dusk", "Dusk",
            /// Dark ink on sand, for daylight.
            Sand => "sand", "Sand",
            /// The original Game Boy's four greens.
            Dmg => "dmg", "DMG",
        }
    }
}

mod tab_layout {
    use crate::config::token_enum::token_enum;

    token_enum! {
        /// How the lists show the open tabs. Serializes to its lowercase name in
        /// TOML; an unknown value falls back to `List`.
        pub enum TabLayout {
            default List;
            /// A row per tab (the default).
            List => "list", "List",
            /// A card per tab with a picture of its page.
            Grid => "grid", "Grid",
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
