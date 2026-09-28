use crate::config::token_enum::token_enum;

token_enum! {
    /// What heads the start page. Serializes to `"banner"` / `"wordmark"` /
    /// `"compact"`; an unknown value falls back to `Banner`.
    pub enum HomeStyle {
        default Banner;
        /// The brand scene: the sun, the sea and the surfer around the wordmark.
        Banner => "banner", "Banner",
        /// The wordmark over a single wave.
        Wordmark => "wordmark", "Wordmark",
        /// Nothing: the search field and the dial move up, for small screens.
        Compact => "compact", "Compact",
    }
}
