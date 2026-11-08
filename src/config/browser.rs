use super::search::{self, SearchEngine};
use crate::config::token_enum::token_enum;
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BrowserConfig {
    /// The name of the engine searches go to (see [`search`]).
    pub search_engine: String,
    /// The engines to pick from, in the order they are offered.
    pub search_engines: Vec<SearchEngine>,
    /// The User-Agent header sites see. Empty keeps Servo's platform default;
    /// the keywords `desktop`, `mobile` (or `android`), and `ios` pick the
    /// matching stock UA — `mobile` makes sites serve their phone layouts,
    /// which fit a small screen far better; anything else is sent verbatim.
    pub user_agent: String,
    /// Keep site data (cookies, localStorage, HSTS) across restarts, so logins
    /// survive. Stored in the `servo/` subfolder of the data dir
    /// (`cookie_jar.json`, `localstorage.json`, …). When false everything is
    /// in-memory only and gone on exit.
    pub persist_site_data: bool,
    /// Reopen the tabs that were open at exit instead of `[home] page`; off also
    /// drops the stored session (see [`crate::data::session`]).
    pub restore_tabs: bool,
    /// Open-tab ceiling; `0` is unlimited. Each tab is a live webview Servo
    /// cannot suspend, so opening past the cap closes the oldest inactive one.
    pub max_tabs: u32,
    /// Default page zoom for every tab (1.0 = 100%). Reflows the layout, so
    /// `1.25` makes the whole web bigger on a small screen; `zoom_in` /
    /// `zoom_out` step from here, `zoom_reset` returns.
    pub page_zoom: f32,
    /// Page color scheme: `dark` only reaches sites that ship a dark theme,
    /// `forced-dark` inverts every page. Changing it reloads the open tabs (see
    /// [`crate::browser::AppBrowser::set_page_theme`]).
    pub page_theme: PageTheme,
}

impl Default for BrowserConfig {
    fn default() -> Self {
        Self {
            search_engine: search::default_name(),
            search_engines: search::defaults(),
            user_agent: String::new(),
            persist_site_data: true,
            restore_tabs: true,
            max_tabs: 8,
            page_zoom: 1.0,
            page_theme: PageTheme::Light,
        }
    }
}

token_enum! {
    /// How page content is themed. No `System` variant: neither SDL2 nor the
    /// handheld targets expose a system theme to follow.
    pub enum PageTheme {
        default Light;
        Light => "light", "Light",
        /// Sites with a dark stylesheet serve it; sites without one are unchanged.
        Dark => "dark", "Dark",
        /// Invert every page (see [`crate::browser::page_theme`]).
        ForcedDark => "forced-dark", "Forced dark",
    }
}

impl PageTheme {
    /// The scheme pages are told to prefer. `ForcedDark` asks for light on
    /// purpose: the filter inverts what it is given, so an already-dark page
    /// would come out light.
    pub fn prefers_dark(self) -> bool {
        matches!(self, PageTheme::Dark)
    }

    pub fn is_forced_dark(self) -> bool {
        matches!(self, PageTheme::ForcedDark)
    }
}
