use crate::config::token_enum::token_enum;

token_enum! {
    /// How a texture over the [`crate::config::TextureLimit`] is shrunk.
    pub enum TextureFilter {
        default Smooth;
        /// Each pixel the average of the ones it replaces.
        Smooth => "smooth", "Smooth",
        /// Every other pixel kept as is, so pixel art stays crisp.
        Sharp => "sharp", "Sharp",
    }
}
