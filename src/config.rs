pub struct AppConfig {
    pub browser: BrowserConfig,
    pub interface: InterfaceConfig,
}

pub struct BrowserConfig {
    pub home_page: String,
    pub experimental_prefs_enabled: bool,
    pub search_page: String,
}

impl Default for BrowserConfig {
    fn default() -> Self {
        Self {
            home_page: "https://duckduckgo.com".to_string(),
            experimental_prefs_enabled: true,
            search_page: "https://duckduckgo.com/?q=%s".to_string(),
        }
    }
}

pub struct InterfaceConfig {
    pub width: u32,
    pub height: u32,
    /// Request an OpenGL ES context (required on Mali handhelds) instead of
    /// desktop GL. Can be overridden at startup via `RETSURF_GLES=0`.
    pub use_gles: bool,
}

impl Default for InterfaceConfig {
    fn default() -> Self {
        Self {
            width: 640,
            height: 480,
            use_gles: true,
        }
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            browser: BrowserConfig::default(),
            interface: InterfaceConfig::default(),
        }
    }
}
