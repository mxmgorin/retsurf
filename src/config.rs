pub struct AppConfig {
    pub browser: BrowserConfig,
    pub interface: InterfaceConfig,
}

pub struct BrowserConfig {
    pub home_url: String,
    pub experimental_prefs_enabled: bool,
}

impl Default for BrowserConfig {
    fn default() -> Self {
        Self {
            home_url: "https://duckduckgo.com".to_string(),
            experimental_prefs_enabled: true,
        }
    }
}

pub struct InterfaceConfig {
    pub width: u32,
    pub height: u32,
}

impl Default for InterfaceConfig {
    fn default() -> Self {
        Self {
            width: 640,
            height: 480,
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
