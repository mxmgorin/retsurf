//! Junk Surfer, the game served at `retsurf:junk-surfer` and inlined into the
//! net-error page.
//! The script is inlined rather than linked because the error page is parsed
//! under the failed URL, so a script file would be fetched from that host.

/// The internal URL the game lives at.
pub const JUNK_SURFER_URL: &str = "retsurf:junk-surfer";

/// The marker a page puts where the game's script goes.
const GAME_PLACEHOLDER: &str = "${retsurf_game}";
const GAME_JS: &str = include_str!("assets/junk_surfer.js");

pub fn is_junk_surfer(url: &url::Url) -> bool {
    url.as_str() == JUNK_SURFER_URL
}

pub fn render() -> String {
    inline_game(include_str!("assets/junk_surfer.html"))
}

/// `page` with the game's script in place of its placeholder.
pub fn inline_game(page: &str) -> String {
    page.replace(GAME_PLACEHOLDER, GAME_JS)
}
