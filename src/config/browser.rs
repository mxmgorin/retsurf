use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct BrowserConfig {
    pub home_page: String,
    pub experimental_prefs_enabled: bool,
    pub search_page: String,
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
    /// Default page zoom for every tab (1.0 = 100%). Reflows the layout, so
    /// `1.25` makes the whole web bigger on a small screen; `zoom_in` /
    /// `zoom_out` step from here, `zoom_reset` returns.
    pub page_zoom: f32,
}

impl Default for BrowserConfig {
    fn default() -> Self {
        Self {
            // The built-in start page (crate::browser::home::HOME_URL).
            home_page: "retsurf:home".to_string(),
            experimental_prefs_enabled: true,
            search_page: "https://duckduckgo.com/?q=%s".to_string(),
            user_agent: String::new(),
            persist_site_data: true,
            page_zoom: 1.0,
        }
    }
}
