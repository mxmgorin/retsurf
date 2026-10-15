//! Game patches: fixes for known games, each a user script that recognises its
//! game and does nothing on any other page. They wrap the page before its own
//! scripts run, so a change reaches only documents loaded after it.

use super::AppBrowser;
use std::rc::Rc;

const PATCHES: &[&str] = &[
    // Saves of Humble's asm.js ports (FTL).
    include_str!("assets/patches/humble_saves.js"),
];

pub(super) fn scripts() -> Vec<Rc<servo::UserScript>> {
    PATCHES
        .iter()
        .map(|js| Rc::new(servo::UserScript::new(js.to_string(), None)))
        .collect()
}

impl AppBrowser {
    /// Apply the game patches to each document loaded from now on, or stop.
    pub fn set_game_patches(&self, on: bool) {
        let mut held = self.inner.game_patch_scripts.borrow_mut();
        if on == !held.is_empty() {
            return;
        }
        for script in held.drain(..) {
            self.inner.user_content.remove_script(script);
        }
        if on {
            *held = scripts();
            for script in held.iter() {
                self.inner.user_content.add_script(script.clone());
            }
        }
    }
}
