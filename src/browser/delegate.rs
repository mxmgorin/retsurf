//! Every reaction to Servo lives here: the [`servo::WebViewDelegate`] impl on
//! [`AppBrowserInner`] — frame/URL/load-status notifications, the
//! download-navigation interception (see [`crate::data::downloads`]), and the
//! ad-block hook over every resource load (see [`crate::browser::adblock`]). New
//! delegate hooks (favicons, dialogs, notifications, …) belong in this file.

use super::{AppBrowserInner, BrowserState, Tab};
use crate::event::user::UserEvent;
use servo::WebView;
use url::Url;

impl AppBrowserInner {
    /// Whether navigating to `url` should download it instead: an `http(s)` URL
    /// whose path's last segment carries one of the configured file extensions.
    fn is_download_url(&self, url: &Url) -> bool {
        if url.scheme() != "http" && url.scheme() != "https" {
            return false;
        }
        let Some(name) = url.path_segments().and_then(|mut s| s.next_back()) else {
            return false;
        };
        let Some((stem, ext)) = name.rsplit_once('.') else {
            return false;
        };
        !stem.is_empty()
            && self
                .download_exts
                .iter()
                .any(|e| e.eq_ignore_ascii_case(ext))
    }
}

impl servo::WebViewDelegate for AppBrowserInner {
    fn notify_new_frame_ready(&self, _: WebView) {
        self.repaint_pending.set(true);
        self.event_sender.send(UserEvent::BrowserFrameReady);
    }

    fn notify_url_changed(&self, webview: WebView, url: Url) {
        // Update whichever tab navigated (so its address bar is right once shown);
        // only log to history when it's the tab the user is actually viewing.
        if let Some(i) = self.tab_index(webview.id()) {
            let url = url.to_string();
            self.tabs.borrow_mut()[i].state.location = url.clone();
            if i == self.active.get() {
                self.visited.borrow_mut().push(url);
            }
        }
    }

    fn notify_load_status_changed(&self, webview: WebView, status: servo::LoadStatus) {
        if let Some(i) = self.tab_index(webview.id()) {
            self.tabs.borrow_mut()[i].state.load_status = status;
        }
    }

    /// Servo can't download: navigating to a file URL would just fail to render.
    /// Deny those navigations and queue the URL for our own fetch instead (see
    /// [`crate::data::downloads`]). Everything else proceeds normally.
    fn request_navigation(&self, _webview: WebView, request: servo::NavigationRequest) {
        if !self.is_download_url(&request.url) {
            request.allow();
            return;
        }
        let url = request.url.to_string();
        log::info!("intercepting download navigation: {url}");
        request.deny();
        self.download_requests.borrow_mut().push(url);
        // Wake the main loop so the download starts right away even when idle.
        self.event_sender.send(UserEvent::DownloadUpdate);
    }

    /// Servo requests an IME whenever an editable element gains focus — we
    /// don't show one, but the request marks "the user is typing", which mutes
    /// plain-key keyboard shortcuts. Select pickers and JS dialogs are queued
    /// for the modal prompt overlay (see [`crate::overlay::prompt`]); the rest (color /
    /// file pickers, context menus) aren't rendered yet — dropping them
    /// dismisses them with their defaults.
    fn show_embedder_control(&self, _webview: WebView, control: servo::EmbedderControl) {
        match control {
            servo::EmbedderControl::InputMethod(ime) => self.ime_control.set(Some(ime.id())),
            servo::EmbedderControl::SelectElement(_) | servo::EmbedderControl::SimpleDialog(_) => {
                self.embedder_controls.borrow_mut().push(control);
                // Wake the main loop so the prompt shows even when idle.
                self.event_sender.send(UserEvent::ControlPending);
            }
            _ => log::info!("unhandled embedder control: dismissed with its default"),
        }
    }

    fn hide_embedder_control(&self, _webview: WebView, id: servo::EmbedderControlId) {
        if self.ime_control.get() == Some(id) {
            self.ime_control.set(None);
            return;
        }
        // A queued select/dialog Servo retracted (navigation, element removal,
        // …) — ids we never queued are harmless to push, the drain ignores them.
        self.dismissed_controls.borrow_mut().push(id);
        self.event_sender.send(UserEvent::ControlPending);
    }

    /// A page asked to open a new webview — a `target="_blank"` link or
    /// `window.open`. Build it (reusing this webview's delegate and our shared
    /// rendering context) and adopt it as a new foreground tab. Servo destroys
    /// the new webview immediately unless we keep a live handle, so it must go
    /// into `tabs`. The new webview drives its own navigation, so we don't set a
    /// URL — mirroring [`super::AppBrowser::build_tab`] otherwise.
    fn request_create_new(&self, parent_webview: WebView, request: servo::CreateNewWebViewRequest) {
        let webview = request
            .builder(self.rendering_ctx.clone())
            .hidpi_scale_factor(euclid::Scale::new(crate::config::device_scale()))
            .delegate(parent_webview.delegate())
            .build();
        if self.default_zoom != 1.0 {
            webview.set_page_zoom(self.default_zoom);
        }

        // Only one tab may be shown (all share one rendering context), so hide
        // the current one before showing the new tab — matching `open_tab`.
        if let Some(cur) = self.active_webview() {
            cur.hide();
        }
        webview.show();
        webview.focus();

        let mut tabs = self.tabs.borrow_mut();
        tabs.push(Tab {
            webview,
            state: BrowserState::default(),
        });
        self.active.set(tabs.len() - 1);
        drop(tabs);
        self.repaint_pending.set(true);
        self.event_sender.send(UserEvent::BrowserFrameReady);
    }

    /// Intercept resource loads. A top-level navigation to the built-in start
    /// page (`retsurf:home`) is answered with locally rendered HTML (see
    /// [`super::home`]); otherwise loads run through the ad blocker, where a
    /// blocked load gets an empty 200 response so scripts/images fail soft
    /// instead of raising network errors. Everything else proceeds untouched
    /// (dropping the load means "do not intercept").
    fn load_web_resource(&self, _webview: WebView, load: servo::WebResourceLoad) {
        let req = load.request();
        let url = req.url.clone();
        let is_home = req.is_for_main_frame && super::home::is_home(&url);
        // Block ads and any lightweight-mode content categories (images / media
        // / fonts). Never the main document itself — only its subresources.
        let block = !is_home
            && !req.is_for_main_frame
            && (self.adblock.should_block(req) || self.content_filter.get().blocks(req.destination));

        if is_home {
            let html = super::home::render().into_bytes();
            let mut headers = http::HeaderMap::new();
            headers.insert(
                http::header::CONTENT_TYPE,
                http::HeaderValue::from_static("text/html; charset=utf-8"),
            );
            let response = servo::WebResourceResponse::new(url).headers(headers);
            let mut intercepted = load.intercept(response);
            intercepted.send_body_data(html);
            intercepted.finish();
        } else if block {
            log::debug!("adblock: blocked {url}");
            let response = servo::WebResourceResponse::new(url);
            load.intercept(response).finish();
        }
    }
}
