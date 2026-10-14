use crate::config::token_enum::token_enum;

token_enum! {
    /// The largest side a game's texture keeps; a larger one is halved until it
    /// fits, so a game whose art outgrows a handheld's GPU memory still loads.
    pub enum TextureLimit {
        default Full;
        Full => "full", "Full",
        L1024 => "1024", "1024",
        L512 => "512", "512",
        L256 => "256", "256",
    }
}

impl TextureLimit {
    /// The limit in px, `None` for no limit.
    pub fn px(self) -> Option<u32> {
        match self {
            TextureLimit::Full => None,
            TextureLimit::L1024 => Some(1024),
            TextureLimit::L512 => Some(512),
            TextureLimit::L256 => Some(256),
        }
    }
}
