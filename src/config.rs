pub struct AppConfig {
    pub home_url: String,
    pub interface: InterfaceConfig,
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
            home_url: "https://servo.org/".to_string(),
            interface: InterfaceConfig::default(),
        }
    }
}
