//! The embedded Servo browser. The data model and shared state live here; tab
//! lifecycle in [`tabs`], page input and evaluated scripts in [`input`],
//! toolbar/router commands in [`command`]. All reactions to Servo (the
//! `WebViewDelegate` impl, download interception, ad blocking) live in
//! [`delegate`]; address-bar text interpretation in [`url`].

pub mod adblock;
mod blob_download;
pub mod content_filter;

mod command;
mod delegate;
mod engine;
mod forced_dark;
mod home;
mod input;
pub mod memory;
mod reader;
mod tabs;
mod url;

pub use blob_download::BlobDownload;
pub use command::BrowserCommand;
pub use engine::effective_user_agent;
pub use home::HOME_URL;
pub use url::try_into_url;

use crate::{
    browser::{adblock::Adblock, content_filter::ContentFilter},
    config::{AppConfig, BrowserConfig, ExperimentalConfig, PageTheme},
    event::user::{UserEvent, UserEventSender},
};
use servo::profile_traits::mem::MemoryReportResult;
use servo::{EventLoopWaker, RenderingContext, WebView};
use servo_base::generic_channel::GenericCallback;
use std::{
    cell::{Cell, RefCell, RefMut},
    collections::{HashMap, HashSet},
    rc::Rc,
    sync::{Arc, Mutex},
};

pub struct AppBrowser {
    inner: Rc<AppBrowserInner>,
}

pub struct BrowserState {
    /// Address bar text: an edit buffer, not necessarily the loaded page (a
    /// half-typed draft must not pass for the current page — see `page_url`).
    pub location: String,
    /// The tab's loaded URL, written only by [`delegate`] on real navigations.
    page_url: String,
    /// Set when a load starts, cleared on ready state complete. Not Servo's
    /// [`servo::LoadStatus`] verbatim — see [`delegate`] for why.
    loading: bool,
}

impl BrowserState {
    pub fn is_loading(&self) -> bool {
        self.loading
    }

    /// What the tab has loaded, as opposed to the `location` edit buffer.
    pub fn page_url(&self) -> &str {
        &self.page_url
    }

    /// For a tab whose webview was built already fetching a URL.
    fn loading() -> Self {
        Self {
            loading: true,
            ..Default::default()
        }
    }
}

impl Default for BrowserState {
    fn default() -> Self {
        Self {
            location: "".into(),
            page_url: "".into(),
            loading: false,
        }
    }
}

/// One open tab: its WebView plus the toolbar state (URL text, load status) for
/// that tab. All tabs share the single rendering context; only the active one is
/// shown (see [`AppBrowser::switch_to`]).
struct Tab {
    webview: WebView,
    state: BrowserState,
    /// Images allowed on this tab, hashed for the per-page cap (see
    /// `delegate::image_key`), bucketed by the load's referrer — the closest
    /// thing to a frame identity — so an iframe can't spend the page's budget.
    /// Cleared on top-level navigations; per tab so a background load can't
    /// spend the visible page's budget.
    page_images: RefCell<HashMap<u64, HashSet<u64>>>,
}

impl Tab {
    /// A tab around a webview that was built already fetching its URL.
    fn loading(webview: WebView) -> Self {
        Self {
            webview,
            state: BrowserState::loading(),
            page_images: RefCell::default(),
        }
    }
}

/// A denied download navigation or an `a[download]` link, for
/// [`crate::data::downloads`] to fetch.
pub struct DownloadRequest {
    pub url: String,
    /// Linking page, sent as Referer.
    pub referer: Option<String>,
    /// Name the page's `download` attribute asked for (already sanitized).
    pub suggested_name: Option<String>,
}

/// A read-only snapshot of a tab for the menu's Tabs section.
pub struct TabInfo {
    /// Page title, falling back to the URL (then "New tab") when unknown.
    pub title: String,
    /// The tab's current location (the bookmark target for Y in the menu); may
    /// be empty for a freshly opened tab that hasn't navigated yet.
    pub url: String,
    /// Whether this is the currently shown tab.
    pub active: bool,
}

/// Shared state behind the [`AppBrowser`] handle. Servo calls back into it as
/// the webviews' delegate — see [`delegate`] for that side.
struct AppBrowserInner {
    tabs: RefCell<Vec<Tab>>,
    /// Index of the shown tab in `tabs`.
    active: Cell<usize>,
    event_sender: UserEventSender,
    servo: servo::Servo,
    rendering_ctx: Rc<dyn RenderingContext>,
    repaint_pending: Cell<bool>,
    /// URLs the active webview has actually navigated to since the last drain, for
    /// the history log. Sourced from `notify_url_changed` (a real navigation), *not*
    /// the address-bar text — so typing a URL doesn't pollute history.
    visited: RefCell<Vec<String>>,
    /// Download navigations denied by [`delegate`], drained once per frame.
    download_requests: RefCell<Vec<DownloadRequest>>,
    /// Webviews whose page signalled a captured blob download (see
    /// [`blob_download`]), drained once per frame into `blob_downloads`.
    blob_pings: RefCell<Vec<WebView>>,
    /// Files captured from pages, waiting for the main loop to save them.
    blob_downloads: RefCell<Vec<BlobDownload>>,
    /// Lowercased URL path extensions treated as downloads (from `[downloads]`).
    download_exts: Vec<String>,
    /// Network-level ad blocking, consulted for every resource load.
    adblock: Adblock,
    /// Lightweight-mode content filter (block images/media/fonts), consulted for
    /// every resource load. A `Cell` so a settings save can swap in new flags live.
    content_filter: Cell<ContentFilter>,
    /// Clickable-element rects reported by the page for hint mode (see
    /// [`AppBrowser::collect_hints`]), drained once by the main loop.
    hint_rects: RefCell<Option<Vec<crate::overlay::hints::Hint>>>,
    /// The live IME request, present while an editable element on the page
    /// holds focus (see [`delegate`]). Plain-key keyboard shortcuts are
    /// suppressed while it's set so they can't hijack typing.
    ime_control: Cell<Option<servo::EmbedderControlId>>,
    /// Select pickers and JS dialogs the page opened (see [`delegate`]),
    /// drained once per frame by the main loop into the prompt overlay.
    embedder_controls: RefCell<Vec<servo::EmbedderControl>>,
    /// Controls Servo retracted before they were answered, drained alongside
    /// `embedder_controls` so the overlay drops them.
    dismissed_controls: RefCell<Vec<servo::EmbedderControlId>>,
    /// Injects the download-capture shim (see [`blob_download`]) into every
    /// document before its own scripts run; attached to each webview at build.
    user_content: Rc<servo::UserContentManager>,
    /// `[browser] page_zoom`: applied to every new tab and the `zoom_reset`
    /// target (also hides the toolbar zoom chip when a tab is back at it).
    default_zoom: f32,
    /// Device pixel ratio in force — the UI scale, so a CSS pixel and a chrome
    /// point are the same size. A `Cell`: the scale follows the window.
    hidpi: Cell<f32>,
    /// `[browser] max_tabs` (`0` unlimited); a `Cell` so a settings save can
    /// change it live.
    max_tabs: Cell<usize>,
    /// `[browser] page_theme`. Behind a `Cell` so a settings save can retheme
    /// the open tabs and still be inherited by tabs opened later.
    page_theme: Cell<PageTheme>,
    /// The forced-dark sheet, attached to `user_content` while the theme asks
    /// for it. Kept so it can be detached again.
    forced_dark: Rc<servo::user_contents::UserStyleSheet>,
    /// Latest memory report from Servo (see [`AppBrowser::request_memory_report`]).
    /// `Arc<Mutex>` because the report arrives on an IPC router thread, not the
    /// main loop. Drained by [`AppBrowser::take_memory_report`].
    mem_report: Arc<Mutex<Option<MemoryReportResult>>>,
}

impl AppBrowserInner {
    pub fn new(
        servo: servo::Servo,
        rendering_ctx: Rc<dyn RenderingContext>,
        event_sender: UserEventSender,
        download_exts: Vec<String>,
        adblock: Adblock,
        content_filter: ContentFilter,
        browser: &BrowserConfig,
    ) -> Self {
        // Sanitize the configured zoom: Servo clamps it to [0.1, 10.0] anyway,
        // and a zero/negative/NaN default would make every tab unusable.
        let zoom = browser.page_zoom;
        let default_zoom = if zoom.is_finite() && zoom > 0.0 {
            zoom.clamp(0.1, 10.0)
        } else {
            1.0
        };
        // The download-capture shim must wrap URL.createObjectURL before any
        // page script runs, so it's a user script (per document, iframes
        // included), not an evaluate_javascript after load.
        let user_content = Rc::new(servo::UserContentManager::new(&servo));
        user_content.add_script(Rc::new(servo::UserScript::new(
            blob_download::capture_js().to_string(),
            None,
        )));
        let forced_dark = forced_dark::stylesheet();
        if browser.page_theme.is_forced_dark() {
            user_content.add_stylesheet(forced_dark.clone());
        }
        Self {
            tabs: RefCell::new(vec![]),
            active: Cell::new(0),
            event_sender,
            servo,
            rendering_ctx,
            repaint_pending: Cell::new(false),
            visited: RefCell::new(vec![]),
            download_requests: RefCell::new(vec![]),
            blob_pings: RefCell::new(vec![]),
            blob_downloads: RefCell::new(vec![]),
            download_exts: download_exts
                .into_iter()
                .map(|e| e.trim_start_matches('.').to_ascii_lowercase())
                .collect(),
            adblock,
            content_filter: Cell::new(content_filter),
            hint_rects: RefCell::new(None),
            ime_control: Cell::new(None),
            embedder_controls: RefCell::new(vec![]),
            dismissed_controls: RefCell::new(vec![]),
            user_content,
            default_zoom,
            hidpi: Cell::new(crate::config::device_scale().unwrap_or(1.0)),
            max_tabs: Cell::new(browser.max_tabs as usize),
            page_theme: Cell::new(browser.page_theme),
            forced_dark,
            mem_report: Arc::new(Mutex::new(None)),
        }
    }

    /// Attach or detach the forced-dark sheet. Only ever called on a real theme
    /// change: `add_stylesheet` pushes, so attaching twice would double it.
    fn sync_forced_dark(&self, theme: PageTheme) {
        if theme.is_forced_dark() {
            self.user_content.add_stylesheet(self.forced_dark.clone());
        } else {
            self.user_content
                .remove_stylesheet(self.forced_dark.clone());
        }
    }

    /// The currently shown tab's webview, if any.
    fn active_webview(&self) -> Option<WebView> {
        self.tabs
            .borrow()
            .get(self.active.get())
            .map(|t| t.webview.clone())
    }

    /// Index of the tab owning `id`, if any.
    fn tab_index(&self, id: servo::WebViewId) -> Option<usize> {
        self.tabs.borrow().iter().position(|t| t.webview.id() == id)
    }

    /// Whether another tab fits under `[browser] max_tabs`. Only page-opened
    /// tabs ask: they are declined at the cap, never granted an eviction.
    fn has_tab_room(&self) -> bool {
        let cap = self.max_tabs.get();
        cap == 0 || self.tabs.borrow().len() < cap
    }
}

impl AppBrowser {
    pub fn new(
        rendering_ctx: Rc<dyn RenderingContext>,
        event_sender: UserEventSender,
        config: &AppConfig,
    ) -> Result<Self, String> {
        // Path B: Servo renders into an FBO in SDL2's shared GL context
        // (see `SdlRenderingContext`); egui composites that FBO's texture.
        let servo = servo::ServoBuilder::default()
            .opts(engine::build_opts(&config.browser))
            .preferences(engine::build_preferences(
                &config.browser,
                &config.performance,
            ))
            .event_loop_waker(event_sender.clone_box())
            .build();
        engine::set_experimental_prefs(&servo, &config.experimental);
        let inner = AppBrowserInner::new(
            servo,
            rendering_ctx,
            event_sender.clone(),
            config.downloads.extensions.clone(),
            Adblock::new(&config.adblock),
            ContentFilter::from_config(&config.data_saving),
            &config.browser,
        );

        Ok(Self {
            inner: Rc::new(inner),
        })
    }

    #[inline]
    pub fn is_animating(&self) -> bool {
        self.inner
            .active_webview()
            .map(|tab| tab.animating())
            .unwrap_or(false)
    }

    /// Whether the active tab is showing the built-in start page (see [`home`]).
    /// The router uses this to steer the D-pad as focus navigation on that page.
    #[inline]
    pub fn on_home_page(&self) -> bool {
        let tabs = self.inner.tabs.borrow();
        tabs.get(self.inner.active.get())
            .is_some_and(|t| t.state.page_url == home::HOME_URL)
    }

    /// The active tab's toolbar state (address bar text + load status). Panics if
    /// there are no tabs — there is always at least one once the app is running.
    #[inline]
    pub fn get_state_mut(&self) -> RefMut<'_, BrowserState> {
        let active = self.inner.active.get();
        RefMut::map(self.inner.tabs.borrow_mut(), move |tabs| {
            &mut tabs[active].state
        })
    }

    /// Whether any tab is fetching, not just the shown one — a background tab's
    /// load competes for the same cores.
    pub fn any_loading(&self) -> bool {
        self.inner
            .tabs
            .borrow()
            .iter()
            .any(|t| t.state.is_loading())
    }

    /// Take and clear the URLs navigated to since the last call, for the history
    /// log. Drained once per frame by the main loop.
    #[inline]
    pub fn take_visited(&self) -> Vec<String> {
        std::mem::take(&mut self.inner.visited.borrow_mut())
    }

    /// Take and clear the download navigations denied since the last call.
    #[inline]
    pub fn take_download_requests(&self) -> Vec<DownloadRequest> {
        std::mem::take(&mut self.inner.download_requests.borrow_mut())
    }

    /// Read back entries captured by the injected script (see [`blob_download`]).
    /// One signalled page yields one entry per call; the read is asynchronous, so
    /// files land in `blob_downloads` and links in `download_requests`.
    pub fn poll_blob_downloads(&self) {
        let pings: Vec<WebView> = self.inner.blob_pings.borrow_mut().drain(..).collect();
        for webview in pings {
            let inner = self.inner.clone();
            // Snapshot the linking page now; the callback runs frames later.
            let referer = self
                .inner
                .tab_index(webview.id())
                .and_then(|i| delegate::referer_for(&self.inner.tabs.borrow()[i].state.page_url));
            webview.evaluate_javascript(blob_download::TAKE_JS, move |result| {
                match result {
                    Ok(servo::JSValue::String(taken)) => match blob_download::parse_taken(&taken) {
                        Some(blob_download::Captured::File(item)) => {
                            inner.blob_downloads.borrow_mut().push(item);
                        }
                        Some(blob_download::Captured::Link { url, name }) => {
                            inner.download_requests.borrow_mut().push(DownloadRequest {
                                url,
                                referer,
                                suggested_name: name,
                            });
                        }
                        None => {}
                    },
                    Ok(other) => log::warn!("blob download returned unexpected value: {other:?}"),
                    Err(e) => log::warn!("blob download read failed: {e:?}"),
                }
                inner.event_sender.send(UserEvent::DownloadUpdate);
            });
        }
    }

    /// Take and clear the files captured from pages since the last call.
    #[inline]
    pub fn take_blob_downloads(&self) -> Vec<BlobDownload> {
        std::mem::take(&mut self.inner.blob_downloads.borrow_mut())
    }

    /// Ask Servo for a memory report (the data behind `about:memory`), delivered
    /// asynchronously on an IPC router thread: the callback stashes it and wakes
    /// the loop, which drains it via [`Self::take_memory_report`]. Not free (it
    /// walks every reporter), so the loop throttles requests.
    pub fn request_memory_report(&self) {
        let slot = self.inner.mem_report.clone();
        let waker = self.inner.event_sender.clone();
        let callback = GenericCallback::new(move |result| {
            if let Ok(report) = result {
                if let Ok(mut guard) = slot.lock() {
                    *guard = Some(report);
                }
                // Wake the (possibly idle-blocked) loop so it renders the report.
                waker.send(UserEvent::BrowserWakeup);
            }
        })
        .expect("create memory-report callback");
        self.inner.servo.create_memory_report(callback);
    }

    /// Take the most recent memory report, if one has arrived since the last call.
    #[inline]
    pub fn take_memory_report(&self) -> Option<MemoryReportResult> {
        self.inner.mem_report.lock().ok().and_then(|mut g| g.take())
    }

    /// Replace the lightweight-mode content filter (a settings save), so new
    /// `block_*` flags apply without a restart. Only subsequent loads see it.
    #[inline]
    pub fn set_content_filter(&self, filter: ContentFilter) {
        self.inner.content_filter.set(filter);
    }

    /// Re-apply the experimental prefs live (settings overlay). Like
    /// [`Self::set_content_filter`], effective on the next page load.
    #[inline]
    pub fn set_experimental_prefs(&self, exp: &ExperimentalConfig) {
        engine::set_experimental_prefs(&self.inner.servo, exp);
    }

    /// Retheme every open tab (a reload: measured on Servo 0.4, notifying a
    /// loaded page flips `matchMedia` but does not restyle it) and inherit the
    /// choice into later tabs. Guarded on an actual change so an unrelated
    /// settings save can't discard scroll and form state.
    pub fn set_page_theme(&self, theme: PageTheme) {
        if self.inner.page_theme.replace(theme) == theme {
            return;
        }
        self.inner.sync_forced_dark(theme);
        for tab in self.inner.tabs.borrow_mut().iter_mut() {
            tab.webview.notify_theme_change(engine::theme(theme));
            tab.state.loading = true;
            tab.webview.reload();
        }
    }

    /// Take the select pickers / JS dialogs the pages opened since the last
    /// call, for the modal prompt overlay. Drained once per frame.
    #[inline]
    pub fn take_embedder_controls(&self) -> Vec<servo::EmbedderControl> {
        std::mem::take(&mut self.inner.embedder_controls.borrow_mut())
    }

    /// Take the ids of controls Servo retracted since the last call, so the
    /// prompt overlay drops them. Drained once per frame.
    #[inline]
    pub fn take_dismissed_controls(&self) -> Vec<servo::EmbedderControlId> {
        std::mem::take(&mut self.inner.dismissed_controls.borrow_mut())
    }

    /// Whether an editable element on the page currently holds focus (guards
    /// plain-key keyboard shortcuts against hijacking typed input).
    #[inline]
    pub fn text_input_focused(&self) -> bool {
        self.inner.ime_control.get().is_some()
    }

    /// Clear every site's cookies and web storage, and the HTTP cache. The
    /// on-disk copies follow at exit, when Servo writes the emptied jars.
    pub fn clear_site_data(&self) {
        let manager = self.inner.servo.site_data_manager();
        let storage = servo::StorageType::Local | servo::StorageType::Session;
        // No wholesale web-storage clear exists, so list the sites and name them.
        let sites: Vec<String> = manager
            .site_data(storage)
            .iter()
            .map(servo::SiteData::name)
            .collect();
        let names: Vec<&str> = sites.iter().map(String::as_str).collect();
        manager.clear_site_data(&names, storage);
        manager.clear_cookies(None);
        self.inner.servo.network_manager().clear_cache();
        log::info!("cleared cookies, cache, storage of {} sites", sites.len());
    }

    /// Spin the Servo event loop once, running delegate callbacks and updating paint output.
    #[inline]
    pub fn pump_event_loop(&self) {
        self.inner.servo.spin_event_loop();
    }

    /// Paint the contents of the active WebView into its RenderingContext. Returns true if a paint was performed.
    pub fn paint(&self) -> bool {
        if !self.inner.repaint_pending.get() {
            return false;
        }

        if let Some(tab) = self.inner.active_webview() {
            self.inner.repaint_pending.set(false);
            tab.paint();
            return true;
        }

        false
    }

    /// Follow the chrome's zoom with the page's device pixel ratio. Every open
    /// tab, not just the active one: a hidden tab would lay out for the old scale.
    pub fn set_hidpi(&self, scale: f32) {
        if self.inner.hidpi.replace(scale) == scale {
            return;
        }
        for tab in self.inner.tabs.borrow().iter() {
            tab.webview
                .set_hidpi_scale_factor(euclid::Scale::new(scale));
        }
    }

    /// Shut Servo down cleanly: drop every webview, then the `Servo` handle —
    /// its `Drop` spins the exit pass in which the net/storage threads write the
    /// persisted site data. Skipping it (a bare `process::exit`) loses logins.
    pub fn shutdown(self) {
        // Dropping the webviews releases their delegate handles, making `self`
        // the last owner of the inner state — dropping it drops the `Servo`.
        self.inner.tabs.borrow_mut().clear();
    }

    pub fn resize(&self, w: u32, h: u32) {
        if w == 0 || h == 0 {
            return;
        }
        let size = dpi::PhysicalSize::new(w, h);
        // A full reflow each, so the count is the measurement when chrome
        // that comes and goes is suspected of resizing the page.
        log::debug!("viewport resize: {w}x{h}");
        // Servo's resize reflows *and* resizes the context, but early-returns
        // when the context size already matches — resizing the context ourselves
        // first made Servo skip the reflow. Let `WebView::resize` drive both;
        // with no tab yet, resize the context directly.
        match self.inner.active_webview() {
            Some(tab) => tab.resize(size),
            None => self.inner.rendering_ctx.resize(size),
        }
    }
}
