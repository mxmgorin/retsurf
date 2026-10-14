//! Game textures: shrink every texture a WebGL game uploads past a size limit,
//! so a game whose art outgrows a handheld's GPU memory still loads. A user
//! script wraps the page's WebGL before any page script runs, so the limit
//! reaches only documents loaded after it is set; a running game keeps the
//! textures it has until it is reloaded.

use super::AppBrowser;
use crate::config::{TextureFilter, TextureLimit};
use std::rc::Rc;

/// The wrapper; `GAME_TEXTURE_LIMIT` and `GAME_TEXTURE_FILTER` are filled in.
static GAME_TEXTURES_JS: &str = include_str!("assets/game_textures.js");

fn script(limit_px: u32, filter: TextureFilter) -> String {
    GAME_TEXTURES_JS
        .replace("GAME_TEXTURE_LIMIT", &limit_px.to_string())
        .replace("GAME_TEXTURE_FILTER", filter.as_str())
}

impl AppBrowser {
    /// Shrink textures past `limit` in each document loaded from now on, or
    /// stop with [`TextureLimit::Full`].
    pub fn set_game_textures(&self, limit: TextureLimit, filter: TextureFilter) {
        if let Some(old) = self.inner.game_textures_script.take() {
            self.inner.user_content.remove_script(old);
        }
        let Some(px) = limit.px() else {
            return;
        };
        let user_script = Rc::new(servo::UserScript::new(script(px, filter), None));
        self.inner.user_content.add_script(user_script.clone());
        self.inner.game_textures_script.replace(Some(user_script));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every placeholder is filled, or the page would throw on a bare name.
    #[test]
    fn the_script_is_filled_in() {
        let js = script(512, TextureFilter::Sharp);
        assert!(!js.contains("GAME_TEXTURE_"));
        assert!(js.contains("const LIMIT = 512;"));
        assert!(js.contains(r#"const FILTER = "sharp";"#));
    }
}
