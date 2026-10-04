//! Game scaling: cut the page's game (its largest canvas, or failing that its
//! largest embedded frame) out of its page and size it to the viewport over a
//! black backdrop, in place and reversibly — unlike reader mode, leaving must
//! not reload, or the game loses its state. While on, every new document scales
//! itself too, so the mode survives navigation; the per-page state lives in the
//! page.

use super::AppBrowser;
use crate::config::Scaling;
use std::rc::Rc;
use std::time::{Duration, Instant};

/// The in-page switch; `GAME_SCALING_MODE` is the [`Scaling`] token.
static GAME_SCALING_JS: &str = include_str!("assets/game_scaling.js");
static GAME_GEOMETRY_JS: &str = include_str!("assets/game_geometry.js");

/// How stale [`AppBrowser::game_geometry`] may get before the page is asked again.
const GEOMETRY_POLL: Duration = Duration::from_millis(500);

/// The scaled game's pixels and where they are drawn, in device px of the web view.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct GameGeometry {
    /// The game's own resolution: a canvas's backbuffer, else its box.
    pub source: (f32, f32),
    /// Left, top, width and height on screen.
    pub rect: [f32; 4],
}

impl GameGeometry {
    fn parse(values: &[servo::JSValue]) -> Option<Self> {
        let n: Vec<f32> = values
            .iter()
            .map(|v| match v {
                servo::JSValue::Number(n) => Some(*n as f32),
                _ => None,
            })
            .collect::<Option<_>>()?;
        let [sw, sh, x, y, w, h] = n[..] else {
            return None;
        };
        (sw > 0.0 && sh > 0.0 && w > 0.0 && h > 0.0).then_some(Self {
            source: (sw, sh),
            rect: [x, y, w, h],
        })
    }
}

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
        self.inner.game_geometry.set(None);
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

    /// The scaled game's last reported geometry, asked again every
    /// [`GEOMETRY_POLL`]; `None` with scaling off or before the page has a game.
    pub fn game_geometry(&self) -> Option<GameGeometry> {
        if self.inner.game_scaling.get() == Scaling::Off {
            return None;
        }
        let now = Instant::now();
        let due = self
            .inner
            .game_geometry_asked
            .get()
            .is_none_or(|at| now.duration_since(at) >= GEOMETRY_POLL);
        let webview = self.inner.active_webview().filter(|_| due);
        if let Some(webview) = webview {
            self.inner.game_geometry_asked.set(Some(now));
            let inner = self.inner.clone();
            webview.evaluate_javascript(GAME_GEOMETRY_JS, move |result| match result {
                Ok(servo::JSValue::Array(values)) => {
                    inner.game_geometry.set(GameGeometry::parse(&values));
                }
                Ok(other) => log::warn!("game geometry returned unexpected value: {other:?}"),
                // Asked between documents; the next poll asks the new one.
                Err(e) => log::debug!("game geometry failed: {e:?}"),
            });
        }
        self.inner.game_geometry.get()
    }
}
