//! The embedded Servo browser. The data model and shared state live here; tab
//! lifecycle in [`tabs`], page input and evaluated scripts in [`input`],
//! toolbar/router commands in [`command`]. All reactions to Servo (the
//! `WebViewDelegate` impl, download interception, ad blocking) live in
//! [`delegate`]; address-bar text interpretation in [`url`]. Around those:
//! [`engine`] (Servo construction and prefs), [`memory`] (reports and heap
//! profile), [`pads`] (what the page knows of the gamepads), [`home`] /
//! [`reader`] (the built-in pages), [`blob_download`] and [`forced_dark`]
//! (user-content scripts), [`adblock`] and [`content_filter`] (load filtering).

pub mod adblock;
mod blob_download;
pub mod content_filter;

mod command;
mod compositing;
mod delegate;
mod engine;
mod forced_dark;
mod home;
mod input;
pub mod memory;
mod reader;
mod tabs;
mod url;

pub use command::BrowserCommand;
pub use engine::effective_user_agent;
pub use home::HOME_URL;
pub use url::try_into_url;

mod pads;
pub use pads::PadSlots;

use crate::data::downloads::{BlobDownload, DownloadRequest};
use crate::{
    browser::{adblock::Adblock, content_filter::ContentFilter},
    config::{AppConfig, ExperimentalConfig, PageTheme},
    event::user::{FrameQueue, UserEvent, UserEventSender},
};
use servo::profile_traits::mem::MemoryReportResult;
use servo::{EventLoopWaker, RenderingContext, WebView};
use servo_base::generic_channel::GenericCallback;
use std::{
    cell::{Cell, Ref, RefCell, RefMut},
    rc::Rc,
    sync::{Arc, Mutex},
};

/// WebAudio factory methods Servo lacks (`createScriptProcessor` and etc.);
/// games die at init without them, so every document gets a facade first.
const WEBAUDIO_COMPAT_JS: &str = include_str!("assets/webaudio_compat.js");

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
    /// Whether the page holds the Fullscreen API. Per tab, so switching tabs and
    /// closing one need no reset of their own.
    fullscreen: bool,
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
            fullscreen: false,
        }
    }
}

/// One open tab: its WebView plus the toolbar state (URL text, load status) for
/// that tab. All tabs share the single rendering context; only the active one is
/// shown (see [`AppBrowser::switch_to`]).
struct Tab {
    webview: WebView,
    state: BrowserState,
    /// Images allowed on this tab under the per-page cap. Cleared on top-level
    /// navigations; per tab so a background load can't spend the visible
    /// page's budget.
    page_images: RefCell<content_filter::PageImages>,
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
    visited: FrameQueue<String>,
    /// Download navigations denied by [`delegate`], drained once per frame.
    download_requests: FrameQueue<DownloadRequest>,
    /// Webviews whose page signalled a captured blob download (see
    /// [`blob_download`]), drained once per frame into `blob_downloads`.
    blob_pings: FrameQueue<WebView>,
    /// Files captured from pages, waiting for the main loop to save them.
    blob_downloads: FrameQueue<BlobDownload>,
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
    embedder_controls: FrameQueue<servo::EmbedderControl>,
    /// Controls Servo retracted before they were answered, drained alongside
    /// `embedder_controls` so the overlay drops them.
    dismissed_controls: FrameQueue<servo::EmbedderControlId>,
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
    /// Pads the page has been told about, by the slot it sees them under. Held
    /// here rather than in the event handler because every fresh document has to
    /// be told again: a `Connected` only reaches the document that is loaded.
    pads: RefCell<PadSlots>,
    /// `[input] haptics`: whether a page may rumble the pad. Gates the requests
    /// and what a `Connected` advertises.
    haptics: Cell<bool>,
    /// Rumble requests from pages, queued by the delegate for the main loop to
    /// play on the SDL controllers it does not own. Drained every pass.
    haptic_requests: FrameQueue<servo::GamepadHapticEffectRequest>,
    /// The panel and the window on it, for the page's `screen` and `outerWidth`.
    /// Servo answers those with zeroes unless the delegate supplies them.
    screen: Cell<servo::ScreenGeometry>,
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
        adblock: Adblock,
        config: &AppConfig,
    ) -> Self {
        let browser = &config.browser;
        let download_exts = config.downloads.extensions.clone();
        let content_filter = ContentFilter::from_config(&config.data_saving);
        let haptics = config.input.haptics;
        // Sanitize the configured zoom: Servo clamps it to [0.1, 10.0] anyway,
        // and a zero/negative/NaN default would make every tab unusable.
        let zoom = browser.page_zoom;
        let default_zoom = if zoom.is_finite() && zoom > 0.0 {
            zoom.clamp(0.1, 10.0)
        } else {
            1.0
        };
        // The download-capture shim must wrap URL.createObjectURL before any page
        // script runs, so it is a user script and not an evaluate after load.
        let user_content = Rc::new(servo::UserContentManager::new(&servo));
        user_content.add_script(Rc::new(servo::UserScript::new(
            blob_download::capture_js().to_string(),
            None,
        )));
        user_content.add_script(Rc::new(servo::UserScript::new(
            WEBAUDIO_COMPAT_JS.to_string(),
            None,
        )));
        let forced_dark = forced_dark::stylesheet();
        if browser.page_theme.is_forced_dark() {
            user_content.add_stylesheet(forced_dark.clone());
        }
        Self {
            tabs: RefCell::new(vec![]),
            active: Cell::new(0),
            servo,
            rendering_ctx,
            repaint_pending: Cell::new(false),
            // History entries only matter once a pass happens anyway.
            visited: FrameQueue::silent(event_sender.clone()),
            download_requests: FrameQueue::new(UserEvent::DownloadUpdate, event_sender.clone()),
            blob_pings: FrameQueue::new(UserEvent::DownloadUpdate, event_sender.clone()),
            blob_downloads: FrameQueue::new(UserEvent::DownloadUpdate, event_sender.clone()),
            download_exts: download_exts
                .into_iter()
                .map(|e| e.trim_start_matches('.').to_ascii_lowercase())
                .collect(),
            adblock,
            content_filter: Cell::new(content_filter),
            hint_rects: RefCell::new(None),
            ime_control: Cell::new(None),
            embedder_controls: FrameQueue::new(UserEvent::ControlPending, event_sender.clone()),
            dismissed_controls: FrameQueue::new(UserEvent::ControlPending, event_sender.clone()),
            user_content,
            default_zoom,
            hidpi: Cell::new(crate::config::device_scale().unwrap_or(1.0)),
            max_tabs: Cell::new(browser.max_tabs as usize),
            page_theme: Cell::new(browser.page_theme),
            forced_dark,
            pads: RefCell::new(PadSlots::default()),
            haptics: Cell::new(haptics),
            haptic_requests: FrameQueue::new(UserEvent::HapticPending, event_sender.clone()),
            screen: Cell::new(servo::ScreenGeometry::default()),
            mem_report: Arc::new(Mutex::new(None)),
            event_sender,
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

    /// Finish `builder` with the setup every webview gets, however it is
    /// created — the popup path once drifted and lost the shim and the theme.
    fn build_webview(
        &self,
        builder: servo::WebViewBuilder,
        delegate: Rc<dyn servo::WebViewDelegate>,
        gamepad: Rc<dyn servo::GamepadDelegate>,
    ) -> WebView {
        let webview = builder
            .hidpi_scale_factor(euclid::Scale::new(self.hidpi.get()))
            .delegate(delegate)
            .gamepad_delegate(gamepad)
            .user_content_manager(self.user_content.clone())
            .build();
        if self.default_zoom != 1.0 {
            webview.set_page_zoom(self.default_zoom);
        }
        webview.notify_theme_change(engine::theme(self.page_theme.get()));
        webview
    }

    /// Adopt `tab` as the new shown tab: hide the current one first (all tabs
    /// share one rendering context, so only one may be shown at a time).
    fn adopt_tab(&self, tab: Tab) {
        if let Some(cur) = self.active_webview() {
            cur.hide();
        }
        tab.webview.show();
        tab.webview.focus();
        let mut tabs = self.tabs.borrow_mut();
        tabs.push(tab);
        self.active.set(tabs.len() - 1);
        drop(tabs);
        self.repaint_pending.set(true);
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
            Adblock::new(&config.adblock),
            config,
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

    /// Whether the active tab's page holds fullscreen, which hides the chrome.
    #[inline]
    pub fn is_fullscreen(&self) -> bool {
        let tabs = self.inner.tabs.borrow();
        tabs.get(self.inner.active.get())
            .is_some_and(|t| t.state.fullscreen)
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

    /// The same, read-only — so a read site cannot pass for a write.
    pub fn state(&self) -> Ref<'_, BrowserState> {
        let active = self.inner.active.get();
        Ref::map(self.inner.tabs.borrow(), move |tabs| &tabs[active].state)
    }

    /// Adopt an edited config's live-tunable knobs, mirroring what [`Self::new`]
    /// read at construction — one list, so a new knob cannot land in only one.
    pub fn apply_config(&self, config: &AppConfig) {
        self.set_haptics(config.input.haptics);
        // Lightweight-mode block flags take effect on the next subresource
        // load, no restart needed (unlike the engine-thread counts).
        self.set_content_filter(ContentFilter::from_config(&config.data_saving));
        // Experimental features apply live too — effective on the next page load.
        self.set_experimental_prefs(&config.experimental);
        // The page theme needs no reload at all: open tabs restyle in place.
        self.set_page_theme(config.browser.page_theme);
        // Binds later opens; the tabs already open stay.
        self.set_max_tabs(config.browser.max_tabs);
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
        self.inner.visited.take()
    }

    /// Take and clear the download navigations denied since the last call.
    #[inline]
    pub fn take_download_requests(&self) -> Vec<DownloadRequest> {
        self.inner.download_requests.take()
    }

    /// Ask Servo for a memory report, delivered asynchronously on an IPC router
    /// thread: the callback stashes it and wakes the loop. It walks every
    /// reporter, so the loop throttles the requests.
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

    /// Retheme every open tab by reloading it — notifying a loaded page flips
    /// `matchMedia` without restyling it. Guarded on an actual change, so an
    /// unrelated settings save cannot discard scroll and form state.
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
        self.inner.embedder_controls.take()
    }

    /// Take the ids of controls Servo retracted since the last call, so the
    /// prompt overlay drops them. Drained once per frame.
    #[inline]
    pub fn take_dismissed_controls(&self) -> Vec<servo::EmbedderControlId> {
        self.inner.dismissed_controls.take()
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

    /// Shut Servo down cleanly: drop every webview, then the `Servo` handle —
    /// its `Drop` spins the exit pass in which the net/storage threads write the
    /// persisted site data. Skipping it (a bare `process::exit`) loses logins.
    pub fn shutdown(self) {
        // Dropping the webviews releases their delegate handles, making `self`
        // the last owner of the inner state — dropping it drops the `Servo`.
        self.inner.tabs.borrow_mut().clear();
    }
}
