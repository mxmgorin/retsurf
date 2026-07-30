use serde::{Deserialize, Serialize};

/// Audio output (`[audio]`). Web Audio plays through SDL2 (see [`crate::media`]);
/// `<audio>`/`<video>` stay silent either way. Off never opens a device.
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioConfig {
    /// Master switch, read once at startup: the backend registers before Servo is built.
    pub enabled: bool,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self { enabled: true }
    }
}
