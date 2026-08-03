use serde::{Deserialize, Serialize};

/// Audio output (`[audio]`). Web Audio plays through SDL2 (see [`crate::media`]);
/// `<audio>`/`<video>` stay silent either way. Off never opens a device, but
/// `decodeAudioData` keeps working.
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioConfig {
    /// Master switch, read once at startup: the backend registers before Servo is built.
    pub enabled: bool,
    /// Longest clip `decodeAudioData` will decode, in seconds (`0` is unlimited). Costs
    /// `seconds * rate * channels * 4` bytes, twice that while resampling.
    pub max_decode_seconds: u32,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_decode_seconds: 300,
        }
    }
}
