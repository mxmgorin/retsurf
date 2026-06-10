//! Every reaction to Servo lives here: the [`servo::WebViewDelegate`] impl on
//! [`AppBrowserInner`] — frame/URL/load-status notifications, the
//! download-navigation interception (see [`crate::data::downloads`]), and the
//! ad-block hook over every resource load (see [`crate::adblock`]). New
//! delegate hooks (favicons, dialogs, notifications, …) belong in this file.

use super::AppBrowserInner;
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
        !stem.is_empty() && self.download_exts.iter().any(|e| e.eq_ignore_ascii_case(ext))
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

    /// Run every resource load through the ad blocker. Blocked loads get an
    /// empty 200 response, so scripts/images fail soft instead of raising
    /// network errors; everything else proceeds untouched (dropping the load
    /// means "do not intercept").
    fn load_web_resource(&self, _webview: WebView, load: servo::WebResourceLoad) {
        if self.adblock.should_block(load.request()) {
            log::debug!("adblock: blocked {}", load.request().url);
            let response = servo::WebResourceResponse::new(load.request().url.clone());
            load.intercept(response).finish();
        }
    }
}
