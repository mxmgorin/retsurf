//! Toolbar/router commands on [`AppBrowser`], and the page-zoom ladder.

use super::{try_into_url, AppBrowser};
use crate::config::BrowserConfig;

#[derive(Clone)]
pub enum BrowserCommand {
    Back,
    Forward,
    Reload,
    Load,
    /// Navigate the active tab to the configured home page (`home_page`).
    Home,
    /// Toggle reader mode on the active page (see [`super::reader`]).
    Reader,
    /// Step the active tab's page zoom along [`ZOOM_LADDER`] (+1 in, -1 out);
    /// `0` resets to the config default.
    Zoom(i32),
}

/// The page-zoom steps (Firefox's ladder), walked by [`BrowserCommand::Zoom`].
const ZOOM_LADDER: &[f32] = &[
    0.5, 0.67, 0.8, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0,
];

impl AppBrowser {
    pub fn execute_command(&mut self, command: &BrowserCommand, config: &BrowserConfig) {
        match command {
            BrowserCommand::Back => _ = self.inner.active_webview().map(|x| x.go_back(1)),
            BrowserCommand::Forward => _ = self.inner.active_webview().map(|x| x.go_forward(1)),
            BrowserCommand::Reload => {
                if let Some(webview) = self.inner.active_webview() {
                    self.mark_loading();
                    webview.reload();
                }
            }
            BrowserCommand::Reader => self.toggle_reader(),
            BrowserCommand::Zoom(delta) => self.zoom(*delta),
            BrowserCommand::Load => {
                let active = self.inner.active.get();
                let tabs = self.inner.tabs.borrow();
                let Some(tab) = tabs.get(active) else {
                    return;
                };
                let Some(url) = try_into_url(&tab.state.location, &config.search_page) else {
                    log::warn!("failed to parse location");
                    return;
                };
                let webview = tab.webview.clone();
                drop(tabs);
                self.mark_loading();
                webview.load(url);
            }
            BrowserCommand::Home => {
                let Some(webview) = self.inner.active_webview() else {
                    return;
                };
                let Some(url) = try_into_url(&config.home_page, &config.search_page) else {
                    log::warn!("failed to parse home_page `{}`", config.home_page);
                    return;
                };
                self.mark_loading();
                webview.load(url);
            }
        }
    }

    /// Arm the active tab's loading flag; `Complete` clears it. Servo sends
    /// `LoadStatus::Started` only for page-initiated navigations, and back /
    /// forward reuse the session-history document with no load at all.
    fn mark_loading(&self) {
        let active = self.inner.active.get();
        if let Some(tab) = self.inner.tabs.borrow_mut().get_mut(active) {
            tab.state.loading = true;
        }
    }

    /// Step the active tab's page zoom to the next [`ZOOM_LADDER`] entry in the
    /// given direction (so an off-ladder config default still steps sensibly);
    /// `0` resets to the default. Page zoom reflows and is per-WebView.
    fn zoom(&self, delta: i32) {
        let Some(webview) = self.inner.active_webview() else {
            return;
        };
        let current = webview.page_zoom();
        let target = match delta {
            0 => self.inner.default_zoom,
            d if d > 0 => *ZOOM_LADDER
                .iter()
                .find(|z| **z > current + 0.005)
                .unwrap_or(ZOOM_LADDER.last().unwrap()),
            _ => *ZOOM_LADDER
                .iter()
                .rev()
                .find(|z| **z < current - 0.005)
                .unwrap_or(&ZOOM_LADDER[0]),
        };
        webview.set_page_zoom(target);
    }

    /// The active tab's page zoom as a percentage, when it differs from the
    /// config default — feeds the toolbar's zoom chip (hidden at the default).
    pub fn zoom_chip(&self) -> Option<u16> {
        let zoom = self.inner.active_webview().map(|w| w.page_zoom())?;
        ((zoom - self.inner.default_zoom).abs() > 0.005).then(|| (zoom * 100.0).round() as u16)
    }
}
