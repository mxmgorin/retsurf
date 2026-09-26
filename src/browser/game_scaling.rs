//! Game scaling: cut the page's game (its largest canvas, or failing that its
//! largest embedded frame) out of its page and size it to the viewport over a
//! black backdrop, in place and reversibly — unlike reader mode, leaving must
//! not reload, or the game loses its state. While on, every new document scales
//! itself too, so the mode survives navigation; the per-page state lives in the
//! page.

use super::AppBrowser;
use crate::config::Scaling;
use std::rc::Rc;

/// The in-page switch; `GAME_SCALING_MODE` is the [`Scaling`] token.
static GAME_SCALING_JS: &str = include_str!("assets/game_scaling.js");

fn script(scaling: Scaling) -> String {
    GAME_SCALING_JS.replace("GAME_SCALING_MODE", scaling.as_str())
}

impl AppBrowser {
    /// Scale the game on every open tab and on each document loaded from now
    /// on, or put every page back with [`Scaling::Off`]. Idempotent.
    pub fn set_game_scaling(&self, scaling: Scaling) {
        if self.inner.game_scaling.replace(scaling) == scaling {
            return;
        }
        // A user script's source is fixed, so a new mode is a new script.
        if let Some(old) = self.inner.game_scaling_script.take() {
            self.inner.user_content.remove_script(old);
        }
        if scaling != Scaling::Off {
            let user_script = Rc::new(servo::UserScript::new(script(scaling), None));
            self.inner.user_content.add_script(user_script.clone());
            self.inner.game_scaling_script.replace(Some(user_script));
        }
        let js = script(scaling);
        for tab in self.inner.tabs.borrow().iter() {
            tab.webview
                .evaluate_javascript(js.clone(), |result| match result {
                    Ok(servo::JSValue::String(status)) => log::debug!("game scaling: {status}"),
                    Ok(other) => log::warn!("game scaling returned unexpected value: {other:?}"),
                    Err(e) => log::warn!("game scaling failed: {e:?}"),
                });
        }
    }
}
