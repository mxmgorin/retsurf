//! Every reaction to Servo lives here: the [`servo::WebViewDelegate`] impl on
//! [`AppBrowserInner`] — frame/URL/load-status notifications, the
//! download-navigation interception (see [`crate::data::downloads`]), and the
//! ad-block hook over every resource load (see [`crate::browser::adblock`]). New
//! delegate hooks (favicons, dialogs, notifications, …) belong in this file.

use super::{AppBrowserInner, BrowserState, Tab};
use crate::event::user::UserEvent;
use content_security_policy::Destination;
use servo::WebView;
use std::cell::RefCell;
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
            let mut tabs = self.tabs.borrow_mut();
            tabs[i].state.location = url.clone();
            tabs[i].state.page_url = url.clone();
            drop(tabs);
            if i == self.active.get() {
                self.visited.push(url);
            }
        }
    }

    /// `HeadParsed` is dropped: Servo sends it from `HTMLBodyElement::bind_to_tree`,
    /// so a late `<body>` emits one with no `Complete` to follow. `Started` covers
    /// only page-initiated navigations; ours arm the flag themselves.
    fn notify_load_status_changed(&self, webview: WebView, status: servo::LoadStatus) {
        let loading = match status {
            servo::LoadStatus::Started => true,
            servo::LoadStatus::Complete => false,
            servo::LoadStatus::HeadParsed => return,
        };
        if let Some(i) = self.tab_index(webview.id()) {
            self.tabs.borrow_mut()[i].state.loading = loading;
        }
        // A `Connected` only reaches the document loaded when it was sent, so
        // each new one is told again.
        if !loading {
            for (slot, name) in self.pads.borrow().live() {
                webview.notify_input_event(servo::InputEvent::Gamepad(
                    crate::event::gamepad_api::connected(slot, name, self.haptics.get()),
                ));
            }
        }
    }

    /// Without this Servo answers `screen.width`, `availWidth` and `outerWidth`
    /// with zeroes, and a page that branches on them takes its narrowest layout.
    fn screen_geometry(&self, _webview: WebView) -> Option<servo::ScreenGeometry> {
        Some(self.screen.get())
    }

    /// The page enters and leaves fullscreen internally whatever we do, so this
    /// is where the chrome follows it, not a gate on the request.
    fn notify_fullscreen_state_changed(&self, webview: WebView, fullscreen: bool) {
        if let Some(i) = self.tab_index(webview.id()) {
            self.tabs.borrow_mut()[i].state.fullscreen = fullscreen;
        }
        // The chrome is rebuilt after the wait in the same pass, so without an
        // event of its own the bar would hide only on whatever came next.
        self.event_sender.send(UserEvent::BrowserFrameReady);
    }

    /// Servo can't download: navigating to a file URL would just fail to render.
    /// Deny those navigations and queue the URL for our own fetch instead (see
    /// [`crate::data::downloads`]). Everything else proceeds normally.
    fn request_navigation(&self, webview: WebView, request: servo::NavigationRequest) {
        if !self.is_download_url(&request.url) {
            request.allow();
            return;
        }
        let url = request.url.to_string();
        log::info!("intercepting download navigation: {url}");
        request.deny();
        let referer = self
            .tab_index(webview.id())
            .and_then(|i| referer_for(&self.tabs.borrow()[i].state.page_url));
        self.download_requests.push(super::DownloadRequest {
            url,
            referer,
            suggested_name: None,
        });
    }

    /// An IME request marks "the user is typing", which mutes plain-key
    /// shortcuts; we show no keyboard for it. Select pickers and JS dialogs are
    /// queued for the prompt overlay, and the rest dismissed with their defaults.
    fn show_embedder_control(&self, _webview: WebView, control: servo::EmbedderControl) {
        match control {
            servo::EmbedderControl::InputMethod(ime) => self.ime_control.set(Some(ime.id())),
            servo::EmbedderControl::SelectElement(_) | servo::EmbedderControl::SimpleDialog(_) => {
                self.embedder_controls.push(control);
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
        self.dismissed_controls.push(id);
    }

    /// A page asked to open a new webview (`target="_blank"`, `window.open`).
    /// Servo destroys it immediately unless we keep a live handle, so it must go
    /// into `tabs`; it drives its own navigation, so no URL is set.
    fn request_create_new(&self, parent_webview: WebView, request: servo::CreateNewWebViewRequest) {
        // A page must not evict a tab of the user's, so the popup is declined
        // at the cap — except at one, where declining is a dead link.
        let replaces = self.max_tabs.get() == 1;
        if !replaces && !self.has_tab_room() {
            log::warn!("tab cap reached: declined a page-opened tab");
            return;
        }
        let webview = self.build_webview(
            request.builder(self.rendering_ctx.clone()),
            parent_webview.delegate(),
            parent_webview.gamepad_delegate(),
        );

        // Dropping a `WebView` closes it in Servo, so the tab it replaces goes
        // with it rather than lingering behind the cap.
        if replaces {
            self.tabs.borrow_mut().clear();
        }
        self.adopt_tab(Tab {
            webview,
            state: BrowserState::default(),
            page_images: RefCell::default(),
        });
        self.event_sender.send(UserEvent::BrowserFrameReady);
    }

    /// Intercept resource loads: the built-in start page is answered with local
    /// HTML, and a load the ad blocker refuses gets an empty 200 so it fails soft.
    /// Dropping the load means "do not intercept".
    fn load_web_resource(&self, webview: WebView, load: servo::WebResourceLoad) {
        let req = load.request();
        let url = req.url.clone();

        // The injected capture script signals a waiting file by loading this URL,
        // which names no real host; the bytes come back over `evaluate_javascript`.
        if url.as_str().starts_with(super::blob_download::PING_URL) {
            self.blob_pings.push(webview);
            finish_intercepted(load, servo::WebResourceResponse::new(url), Vec::new());
            return;
        }

        let is_home = req.is_for_main_frame && super::home::is_home(&url);

        let filter = self.content_filter.get();
        let is_subresource = !is_home && !req.is_for_main_frame;
        // Block ads and any lightweight-mode content categories (images / media
        // / fonts). Never the main document itself — only its subresources.
        let mut block =
            is_subresource && (self.adblock.should_block(req) || filter.blocks(req.destination));

        // Per-page image cap: Servo loads every image eagerly, and a huge grid
        // freezes the device. Counted per distinct image, not per element.
        if let Some(i) = self.tab_index(webview.id()) {
            let tabs = self.tabs.borrow();
            let images = &tabs[i].page_images;
            if req.is_for_main_frame {
                images.borrow_mut().clear();
            } else if is_subresource && !block && req.destination == Destination::Image {
                if let Some(cap) = filter.image_cap() {
                    if !images
                        .borrow_mut()
                        .allow(&url, req.referrer_url.as_ref(), cap)
                    {
                        log::debug!("image cap: blocked {url}");
                        block = true;
                    }
                }
            }
        }

        if is_home {
            let html = super::home::render().into_bytes();
            let mut headers = http::HeaderMap::new();
            headers.insert(
                http::header::CONTENT_TYPE,
                http::HeaderValue::from_static("text/html; charset=utf-8"),
            );
            let response = servo::WebResourceResponse::new(url).headers(headers);
            finish_intercepted(load, response, html);
        } else if block {
            log::debug!("adblock: blocked {url}");
            let response = servo::WebResourceResponse::new(url);
            finish_intercepted(load, response, Vec::new());
        }
    }
}

impl servo::GamepadDelegate for AppBrowserInner {
    /// Queued for the main loop, which owns the SDL controllers this must play
    /// on (see [`crate::event::handler::AppEventHandler::haptic`]).
    fn handle_haptic_effect_request(&self, request: servo::GamepadHapticEffectRequest) {
        if !self.haptics.get() {
            // Only a document told rumble was supported before the toggle flipped
            // gets here; "complete" — reporting failure strands its promise.
            request.succeeded();
            return;
        }
        log::debug!("haptics: queued for pad slot {}", request.gamepad_index());
        self.haptic_requests.push(request);
    }
}

/// The linking page as a Referer: http(s) only, fragment and credentials stripped.
pub(super) fn referer_for(location: &str) -> Option<String> {
    let mut url = Url::parse(location).ok()?;
    if url.scheme() != "http" && url.scheme() != "https" {
        return None;
    }
    url.set_fragment(None);
    let _ = url.set_username("");
    let _ = url.set_password(None);
    Some(url.to_string())
}

/// Answer an intercepted load with `body`, always sending a chunk — even an
/// empty one. Servo marks a body `Done` only once a chunk arrived, and net's
/// subresource-integrity check panics on a body that isn't.
fn finish_intercepted(
    load: servo::WebResourceLoad,
    response: servo::WebResourceResponse,
    body: Vec<u8>,
) {
    let mut intercepted = load.intercept(response);
    intercepted.send_body_data(body);
    intercepted.finish();
}
