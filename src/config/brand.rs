use crate::config::token_enum::token_enum;

token_enum! {
    /// What heads the start page. An unknown value falls back to `Banner`.
    pub enum Brand {
        default Banner;
        /// The brand scene: the sun, the sea and the surfer around the wordmark.
        Banner => "banner", "Banner",
        /// The wordmark over a single wave.
        Wordmark => "wordmark", "Wordmark",
        /// Nothing: the rest of the page moves up, for small screens.
        None => "none", "None",
    }
}
