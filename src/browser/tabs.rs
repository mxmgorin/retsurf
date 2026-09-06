//! Tab lifecycle on [`AppBrowser`]: opening, closing, switching, and restoring
//! a saved session. All tabs share one rendering context, so exactly one is
//! shown at a time.

use super::{engine, AppBrowser, Tab, TabInfo};
use ::url::Url;

impl AppBrowser {
    /// Number of open tabs.
    #[inline]
    pub fn tab_count(&self) -> usize {
        self.inner.tabs.borrow().len()
    }

    /// A snapshot of the open tabs for the menu's Tabs section.
    pub fn tabs(&self) -> Vec<TabInfo> {
        let active = self.inner.active.get();
        self.inner
            .tabs
            .borrow()
            .iter()
            .enumerate()
            .map(|(i, tab)| {
                let title = tab
                    .webview
                    .page_title()
                    .filter(|t| !t.is_empty())
                    .or_else(|| Some(tab.state.page_url.clone()).filter(|l| !l.is_empty()))
                    .unwrap_or_else(|| "New tab".to_string());
                TabInfo {
                    title,
                    url: tab.state.page_url.clone(),
                    active: i == active,
                }
            })
            .collect()
    }

    /// Build a webview loading `url` (with the default page zoom applied), or
    /// `None` if the URL won't parse. It is not shown or focused — the callers
    /// decide whether the new tab is foreground or background.
    fn build_tab(&self, url: &str) -> Option<servo::WebView> {
        let url = Url::parse(url).ok()?;
        let webview =
            servo::WebViewBuilder::new(&self.inner.servo, self.inner.rendering_ctx.clone())
                .url(url)
                .hidpi_scale_factor(euclid::Scale::new(self.inner.hidpi.get()))
                .delegate(self.inner.clone())
                .user_content_manager(self.inner.user_content.clone())
                .build();
        if self.inner.default_zoom != 1.0 {
            webview.set_page_zoom(self.inner.default_zoom);
        }
        webview.notify_theme_change(engine::theme(self.inner.page_theme.get()));
        Some(webview)
    }

    /// Open a new tab at `url` and make it the active (shown) one.
    pub fn open_tab(&mut self, url: &str) {
        let Some(webview) = self.build_tab(url) else {
            return;
        };

        // Hide the previously shown tab before switching to the new one (all tabs
        // share one rendering context, so only one may be shown at a time).
        if let Some(cur) = self.inner.active_webview() {
            cur.hide();
        }
        webview.show();
        webview.focus();

        let mut tabs = self.inner.tabs.borrow_mut();
        tabs.push(Tab::loading(webview));
        self.inner.active.set(tabs.len() - 1);
        drop(tabs);
        self.inner.repaint_pending.set(true);
        self.trim_tabs();
    }

    /// Open `url` in a new background tab: built and loading, but left unshown
    /// and unfocused so the current tab stays in view (the link-hints "open in
    /// new tab" gesture). Loading is independent of `show()`.
    pub fn open_tab_background(&mut self, url: &str) {
        let Some(webview) = self.build_tab(url) else {
            return;
        };
        self.inner.tabs.borrow_mut().push(Tab::loading(webview));
        self.trim_tabs();
    }

    /// Trim to `[browser] max_tabs` after a push (`0` is unlimited): close the
    /// oldest tabs that aren't in view, so a cap of one replaces instead.
    fn trim_tabs(&self) {
        let cap = self.inner.max_tabs.get();
        if cap == 0 {
            return;
        }
        while self.tab_count() > cap {
            let active = self.inner.active.get();
            let Some(oldest) = (0..self.tab_count()).find(|i| *i != active) else {
                return;
            };
            log::info!("tab cap {cap} reached: closing tab {oldest}");
            self.remove_tab(oldest);
        }
    }

    /// Adopt an edited cap (settings overlay); it bounds later opens only.
    #[inline]
    pub fn set_max_tabs(&self, max_tabs: u32) {
        self.inner.max_tabs.set(max_tabs as usize);
    }

    /// Reopen a saved session (see [`crate::data::session`]): a tab per URL, with
    /// `active` shown, cut to `max_tabs` and past unparseable URLs. `false` when
    /// nothing was restored. Startup only: it assumes an empty tab list.
    pub fn restore_tabs(&mut self, urls: &[String], active: usize) -> bool {
        let window = session_window(urls.len(), active, self.inner.max_tabs.get());
        if window.len() < urls.len() {
            log::info!(
                "session: {} of {} tabs fit the cap",
                window.len(),
                urls.len()
            );
        }
        let active = active - window.start;

        let mut kept = Vec::with_capacity(window.len());
        let mut built = Vec::with_capacity(window.len());
        for (i, url) in urls[window].iter().enumerate() {
            match self.build_tab(url) {
                Some(webview) => {
                    kept.push(i);
                    built.push(webview);
                }
                None => log::warn!("session: dropping unparseable url `{url}`"),
            }
        }
        if built.is_empty() {
            return false;
        }
        let shown = shown_index(&kept, active);
        log::info!("session: restoring {} tabs, showing {shown}", built.len());

        let mut tabs = self.inner.tabs.borrow_mut();
        tabs.extend(built.into_iter().map(Tab::loading));
        tabs[shown].webview.show();
        tabs[shown].webview.focus();
        drop(tabs);
        self.inner.active.set(shown);
        self.inner.repaint_pending.set(true);
        true
    }

    /// Drop every tab and open a fresh one at `url` — the tab half of clearing
    /// browsing data (dropping a `WebView` closes it in Servo).
    pub fn reset_tabs(&mut self, url: &str) {
        self.inner.tabs.borrow_mut().clear();
        self.inner.active.set(0);
        self.open_tab(url);
    }

    /// Switch the shown tab to `index` (no-op if out of range or already active).
    pub fn switch_to(&self, index: usize) {
        let tabs = self.inner.tabs.borrow();
        let active = self.inner.active.get();
        if index >= tabs.len() || index == active {
            return;
        }
        if let Some(cur) = tabs.get(active) {
            cur.webview.hide();
        }
        let target = &tabs[index];
        target.webview.show();
        target.webview.focus();
        drop(tabs);
        self.inner.active.set(index);
        self.inner.repaint_pending.set(true);
    }

    /// Switch the active tab by `delta` positions, wrapping around (e.g. -1 for the
    /// previous tab, +1 for the next). No-op with fewer than two tabs.
    pub fn cycle_tab(&self, delta: i32) {
        let count = self.tab_count();
        if count <= 1 {
            return;
        }
        let active = self.inner.active.get() as i32;
        let next = (active + delta).rem_euclid(count as i32) as usize;
        self.switch_to(next);
    }

    /// Close the tab at `index`. Closing the last one leaves a fresh tab at
    /// `home` instead of no tab at all. If the active tab is closed, the next
    /// tab becomes active and is shown.
    pub fn close_tab(&self, index: usize, home: &str) {
        if self.tab_count() == 1 && index == 0 {
            self.replace_last_tab(home);
            return;
        }
        self.remove_tab(index);
    }

    /// Swap the sole open tab for a new one at `home`: dropping the old WebView
    /// closes it in Servo, so its document and history go with it.
    fn replace_last_tab(&self, home: &str) {
        let Some(webview) = self.build_tab(home) else {
            log::warn!("failed to parse home_page `{home}`");
            return;
        };
        let mut tabs = self.inner.tabs.borrow_mut();
        tabs.clear();
        webview.show();
        webview.focus();
        tabs.push(Tab::loading(webview));
        self.inner.active.set(0);
        drop(tabs);
        self.inner.repaint_pending.set(true);
    }

    /// Drop the tab at `index`, keeping at least one open.
    fn remove_tab(&self, index: usize) {
        let mut tabs = self.inner.tabs.borrow_mut();
        if index >= tabs.len() || tabs.len() == 1 {
            return;
        }
        let active = self.inner.active.get();
        let was_active = index == active;
        // Removing the WebView drops it, which closes it in Servo (see `Drop`).
        tabs.remove(index);

        let new_active = if was_active {
            index.min(tabs.len() - 1)
        } else if index < active {
            active - 1
        } else {
            active
        };
        self.inner.active.set(new_active);
        if was_active {
            let tab = &tabs[new_active];
            tab.webview.show();
            tab.webview.focus();
        }
        drop(tabs);
        self.inner.repaint_pending.set(true);
    }
}

/// Which restored tab to show: the saved `active` one, or the nearest earlier
/// survivor when its URL was skipped. `kept` holds their saved positions.
fn shown_index(kept: &[usize], active: usize) -> usize {
    kept.iter().rposition(|&i| i <= active).unwrap_or(0)
}

/// The stretch of a saved session that fits `cap` (`0` is unlimited): oldest
/// tabs go first, but the window always covers `active`, so `start <= active`.
fn session_window(len: usize, active: usize, cap: usize) -> std::ops::Range<usize> {
    if cap == 0 || len <= cap {
        return 0..len;
    }
    let start = (len - cap).min(active);
    start..start + cap
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The restored tabs keep their saved order, so the shown one is the saved
    /// index shifted back by however many earlier URLs were dropped.
    #[test]
    fn shown_index_follows_the_dropped_tabs() {
        assert_eq!(shown_index(&[0, 1, 2], 2), 2);
        assert_eq!(shown_index(&[1, 3], 3), 1);
        assert_eq!(shown_index(&[0, 2, 4], 4), 2);
    }

    /// The saved active tab itself may be the one dropped: show the nearest
    /// earlier survivor, or the first tab when none precedes it.
    #[test]
    fn shown_index_falls_back_when_the_active_tab_is_dropped() {
        assert_eq!(shown_index(&[0, 3], 2), 0);
        assert_eq!(shown_index(&[2, 3], 1), 0);
    }

    /// A session within the cap (or with no cap at all) restores whole.
    #[test]
    fn session_window_keeps_everything_that_fits() {
        assert_eq!(session_window(3, 1, 8), 0..3);
        assert_eq!(session_window(3, 1, 3), 0..3);
        assert_eq!(session_window(50, 49, 0), 0..50);
    }

    /// Over the cap the oldest tabs go — unless the one in view is among them,
    /// which pins the window to it.
    #[test]
    fn session_window_drops_the_oldest_but_keeps_the_active_tab() {
        assert_eq!(session_window(10, 9, 4), 6..10);
        assert_eq!(session_window(10, 6, 4), 6..10);
        assert_eq!(session_window(10, 2, 4), 2..6);
        assert_eq!(session_window(10, 0, 1), 0..1);
    }
}
