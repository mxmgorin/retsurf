//! The embedded Servo browser: tab list, painting, input, and commands. All
//! reactions to Servo (the `WebViewDelegate` impl, including download
//! interception and ad blocking) live in [`delegate`]; address-bar text
//! interpretation in [`url`].

pub mod adblock;
pub mod content_filter;

mod delegate;
mod home;
pub mod memory;
mod reader;
mod url;

pub use home::HOME_URL;
pub use url::try_into_url;

use crate::{
    browser::{adblock::Adblock, content_filter::ContentFilter},
    config::{BrowserConfig, DataSavingConfig, PerformanceConfig},
    event::user::{UserEvent, UserEventSender},
    overlay::hints::Hint,
};
use ::url::Url;
use servo::{EventLoopWaker, RenderingContext, WebView};
use std::{
    cell::{Cell, RefCell, RefMut},
    rc::Rc,
};

#[derive(Clone)]
pub enum BrowserCommand {
    Back,
    Foward,
    Reload,
    Load,
    /// Navigate the active tab to the configured home page (`home_page`).
    Home,
    /// Toggle reader mode on the active page (see [`reader`]).
    Reader,
    /// Step the active tab's page zoom along [`ZOOM_LADDER`] (+1 in, -1 out);
    /// `0` resets to the config default.
    Zoom(i32),
}

/// The page-zoom steps (Firefox's ladder), walked by [`BrowserCommand::Zoom`].
const ZOOM_LADDER: &[f32] = &[
    0.5, 0.67, 0.8, 0.9, 1.0, 1.1, 1.25, 1.5, 1.75, 2.0, 2.5, 3.0,
];

static EXPERIMENTAL_PREFS: &[&str] = &[
    "dom_async_clipboard_enabled",
    "dom_fontface_enabled",
    "dom_intersection_observer_enabled",
    "dom_notification_enabled",
    "dom_offscreen_canvas_enabled",
    "dom_permissions_enabled",
    "dom_resize_observer_enabled",
    "dom_webgl2_enabled",
    "dom_webgpu_enabled",
    "layout_columns_enabled",
    "layout_container_queries_enabled",
    "layout_grid_enabled",
];

pub struct AppBrowser {
    inner: Rc<AppBrowserInner>,
}

pub struct BrowserState {
    location: String,
    load_status: servo::LoadStatus,
}

impl BrowserState {
    pub fn is_loading(&self) -> bool {
        self.load_status != servo::LoadStatus::Complete
    }

    pub fn get_location_mut(&mut self) -> &mut String {
        &mut self.location
    }

    pub fn get_location(&self) -> &str {
        &self.location
    }
}

impl Default for BrowserState {
    fn default() -> Self {
        Self {
            location: "".into(),
            load_status: servo::LoadStatus::Complete,
        }
    }
}

/// One open tab: its WebView plus the toolbar state (URL text, load status) for
/// that tab. All tabs share the single rendering context; only the active one is
/// shown (see [`AppBrowser::switch_to`]).
struct Tab {
    webview: WebView,
    state: BrowserState,
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
    /// URLs whose navigation was denied because they look like file downloads
    /// (see [`delegate`]), drained once per frame by the main loop which hands
    /// them to the downloads store.
    download_requests: RefCell<Vec<String>>,
    /// Lowercased URL path extensions treated as downloads (from `[downloads]`).
    download_exts: Vec<String>,
    /// Network-level ad blocking, consulted for every resource load.
    adblock: Adblock,
    /// Lightweight-mode content filter (block images/media/fonts), consulted
    /// for every resource load alongside the ad blocker. Behind a `Cell` so a
    /// settings save can swap in new flags live (see [`AppBrowser::set_content_filter`]).
    content_filter: Cell<ContentFilter>,
    /// Clickable-element rects reported by the page for hint mode (see
    /// [`AppBrowser::collect_hints`]), drained once by the main loop.
    hint_rects: RefCell<Option<Vec<Hint>>>,
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
    /// `[browser] page_zoom`: applied to every new tab and the `zoom_reset`
    /// target (also hides the toolbar zoom chip when a tab is back at it).
    default_zoom: f32,
}

impl AppBrowserInner {
    pub fn new(
        servo: servo::Servo,
        rendering_ctx: Rc<dyn RenderingContext>,
        event_sender: UserEventSender,
        download_exts: Vec<String>,
        adblock: Adblock,
        content_filter: ContentFilter,
        default_zoom: f32,
    ) -> Self {
        Self {
            tabs: RefCell::new(vec![]),
            active: Cell::new(0),
            event_sender,
            servo,
            rendering_ctx,
            repaint_pending: Cell::new(false),
            visited: RefCell::new(vec![]),
            download_requests: RefCell::new(vec![]),
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
            default_zoom,
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
}

impl AppBrowser {
    pub fn new(
        rendering_ctx: Rc<dyn RenderingContext>,
        event_sender: UserEventSender,
        config: &BrowserConfig,
        perf: &PerformanceConfig,
        data_saving: &DataSavingConfig,
        download_exts: Vec<String>,
        adblock: Adblock,
    ) -> Result<Self, String> {
        // Path B: Servo renders into an FBO in SDL2's shared GL context
        // (see `SdlRenderingContext`); egui composites that FBO's texture.
        let servo = servo::ServoBuilder::default()
            .opts(build_opts(config))
            .preferences(build_preferences(config, perf))
            .event_loop_waker(event_sender.clone_box())
            .build();
        set_experimental_prefs(&servo, config.experimental_prefs_enabled);
        // Sanitize the config value: Servo clamps zoom to [0.1, 10.0] anyway,
        // and a zero/negative/NaN default would make every tab unusable.
        let default_zoom = if config.page_zoom.is_finite() && config.page_zoom > 0.0 {
            config.page_zoom.clamp(0.1, 10.0)
        } else {
            1.0
        };
        let inner = AppBrowserInner::new(
            servo,
            rendering_ctx,
            event_sender.clone(),
            download_exts,
            adblock,
            ContentFilter::from_config(data_saving),
            default_zoom,
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
            .is_some_and(|t| t.state.location == home::HOME_URL)
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

    /// Take and clear the URLs navigated to since the last call, for the history
    /// log. Drained once per frame by the main loop.
    #[inline]
    pub fn take_visited(&self) -> Vec<String> {
        std::mem::take(&mut self.inner.visited.borrow_mut())
    }

    /// Take and clear the download URLs whose navigation was denied since the
    /// last call. Drained once per frame by the main loop.
    #[inline]
    pub fn take_download_requests(&self) -> Vec<String> {
        std::mem::take(&mut self.inner.download_requests.borrow_mut())
    }

    /// Replace the lightweight-mode content filter (called when the settings
    /// overlay saves new `block_*` flags, so they take effect without a
    /// restart). Already-loaded resources stay; only subsequent loads see it.
    #[inline]
    pub fn set_content_filter(&self, filter: ContentFilter) {
        self.inner.content_filter.set(filter);
    }

    /// Ask the active page for its visible clickable elements (hint mode). The
    /// JS runs asynchronously; the resulting rects land in `hint_rects` (drained
    /// via [`AppBrowser::take_hint_rects`]) and a wake-up event is sent. An
    /// evaluation error yields an empty list, which exits hint mode.
    pub fn collect_hints(&self) {
        let Some(webview) = self.inner.active_webview() else {
            return;
        };
        let inner = self.inner.clone();
        webview.evaluate_javascript(COLLECT_HINTS_JS, move |result| {
            let mut hints = vec![];
            match result {
                // The script returns a flat array, five entries per element:
                // x, y, w, h (viewport-relative CSS px numbers) then the link's
                // absolute URL as a string (empty for non-link clickables).
                Ok(servo::JSValue::Array(values)) => {
                    let num = |v: &servo::JSValue| match v {
                        servo::JSValue::Number(n) => Some(*n as f32),
                        _ => None,
                    };
                    for c in values.chunks_exact(5) {
                        let (Some(x), Some(y), Some(w), Some(h)) =
                            (num(&c[0]), num(&c[1]), num(&c[2]), num(&c[3]))
                        else {
                            continue;
                        };
                        let url = match &c[4] {
                            servo::JSValue::String(s) if !s.is_empty() => Some(s.clone()),
                            _ => None,
                        };
                        hints.push(Hint { x, y, w, h, url });
                    }
                }
                Ok(other) => log::warn!("hint collection returned unexpected value: {other:?}"),
                Err(e) => log::warn!("hint collection failed: {e:?}"),
            }
            *inner.hint_rects.borrow_mut() = Some(hints);
            inner.event_sender.send(UserEvent::HintsReady);
        });
    }

    /// Take the rects from the last hint collection, if it has finished since
    /// the previous call. Drained once per frame by the main loop.
    #[inline]
    pub fn take_hint_rects(&self) -> Option<Vec<Hint>> {
        self.inner.hint_rects.borrow_mut().take()
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

    /// Number of open tabs.
    #[inline]
    pub fn tab_count(&self) -> usize {
        self.inner.tabs.borrow().len()
    }

    /// Index of the active tab.
    #[inline]
    pub fn active_tab(&self) -> usize {
        self.inner.active.get()
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
                    .or_else(|| Some(tab.state.location.clone()).filter(|l| !l.is_empty()))
                    .unwrap_or_else(|| "New tab".to_string());
                TabInfo {
                    title,
                    url: tab.state.location.clone(),
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
                // Device pixel ratio: 1.0 on desktop, the display density on
                // Android (see config::device_scale), so the page lays out for a
                // logical viewport and renders crisply rather than tiny.
                .hidpi_scale_factor(euclid::Scale::new(crate::config::device_scale()))
                .delegate(self.inner.clone())
                .build();
        if self.inner.default_zoom != 1.0 {
            webview.set_page_zoom(self.inner.default_zoom);
        }
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
        tabs.push(Tab {
            webview,
            state: BrowserState::default(),
        });
        self.inner.active.set(tabs.len() - 1);
        drop(tabs);
        self.inner.repaint_pending.set(true);
    }

    /// Open `url` in a new background tab: built and loading, but left unshown
    /// and unfocused so the current tab stays in view (the link-hints "open in
    /// new tab" gesture). Loading is independent of `show()`, so it fetches in
    /// the background; switch to it later via the tab cycle or menu.
    pub fn open_tab_background(&mut self, url: &str) {
        let Some(webview) = self.build_tab(url) else {
            return;
        };
        self.inner.tabs.borrow_mut().push(Tab {
            webview,
            state: BrowserState::default(),
        });
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

    /// Close the tab at `index`. Keeps at least one tab open. If the active tab is
    /// closed, the next tab becomes active and is shown.
    pub fn close_tab(&self, index: usize) {
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

    pub fn handle_input(&self, event: servo::InputEvent) {
        let Some(tab) = self.inner.active_webview() else {
            return;
        };

        tab.notify_input_event(event.clone());

        if let servo::InputEvent::MouseButton(be) = event {
            if be.action == servo::MouseButtonAction::Down {
                match be.button {
                    servo::MouseButton::Back => _ = tab.go_back(1),
                    servo::MouseButton::Forward => _ = tab.go_forward(1),
                    _ => {}
                }
            }
        }
    }

    /// Scroll the active page by a device-pixel delta at `(x, y)`. Positive `dy`
    /// reveals content lower on the page. This is the native compositor scroll
    /// (`InputEvent::Wheel` only fires the DOM `wheel` event, it does not scroll).
    pub fn scroll(&self, dx: f32, dy: f32, x: f32, y: f32) {
        let Some(tab) = self.inner.active_webview() else {
            return;
        };
        let delta = servo::Scroll::Delta(servo::DeviceVector2D::new(dx, dy).into());
        let point = servo::DevicePoint::new(x, y).into();
        tab.notify_scroll_event(delta, point);
    }

    pub fn execute_command(&mut self, command: &BrowserCommand, config: &BrowserConfig) {
        match command {
            BrowserCommand::Back => _ = self.inner.active_webview().map(|x| x.go_back(1)),
            BrowserCommand::Foward => _ = self.inner.active_webview().map(|x| x.go_forward(1)),
            BrowserCommand::Reload => _ = self.inner.active_webview().map(|x| x.reload()),
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
                webview.load(url);
            }
        }
    }

    /// Shut Servo down cleanly: close every webview, then drop the `Servo`
    /// handle — its `Drop` sends Exit to the constellation and spins the event
    /// loop to completion. That exit pass is when the net and storage threads
    /// write the persisted site data (`cookie_jar.json`, `localstorage.json`,
    /// …) into `config_dir`; skipping it (a bare `process::exit`) loses logins.
    pub fn shutdown(self) {
        // Dropping the webviews releases their delegate handles, making `self`
        // the last owner of the inner state — dropping it drops the `Servo`.
        self.inner.tabs.borrow_mut().clear();
    }

    /// Step the active tab's page zoom to the next [`ZOOM_LADDER`] entry in
    /// the given direction (so an off-ladder config default still steps
    /// sensibly); `0` resets to the config default. Page zoom reflows the
    /// layout and is per-WebView, so each tab keeps its own level.
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

    pub fn resize(&self, w: u32, h: u32) {
        if w == 0 || h == 0 {
            return;
        }
        let size = dpi::PhysicalSize::new(w, h);
        // Servo's `resize_rendering_context` resizes our rendering context *and*
        // reflows the page — but it early-returns when the context size already
        // matches. So we must NOT resize the context ourselves first: doing that
        // made Servo skip the reflow, so the page never adjusted. Let
        // `WebView::resize` drive both (it resizes the shared context, covering all
        // tabs). With no tab yet, resize the context directly.
        match self.inner.active_webview() {
            Some(tab) => tab.resize(size),
            None => self.inner.rendering_ctx.resize(size),
        }
    }
}

/// Collect the visible clickable elements as a flat `[x, y, w, h, …]` array
/// (viewport-relative CSS px). Skips off-viewport, zero-size, hidden, and
/// click-through elements; capped so a link-farm page can't flood the IPC
/// channel. Cross-origin iframes are unreachable from the top document — their
/// content gets no hints (the virtual cursor remains the fallback).
const COLLECT_HINTS_JS: &str = r#"
(function () {
    const out = [];
    const vw = window.innerWidth, vh = window.innerHeight;
    const els = document.querySelectorAll(
        'a[href], button, input:not([type="hidden"]), select, textarea, summary, ' +
        '[onclick], [role="button"], [role="link"], [role="tab"], [contenteditable="true"]'
    );
    for (const el of els) {
        if (out.length >= 750) break; // 150 hints (5 entries each)
        const r = el.getBoundingClientRect();
        if (r.width < 2 || r.height < 2) continue;
        if (r.bottom < 0 || r.right < 0 || r.top > vh || r.left > vw) continue;
        const s = window.getComputedStyle(el);
        if (s.visibility !== 'visible' || s.pointerEvents === 'none') continue;
        // el.href on an <a> is the resolved absolute URL; restrict to http(s)
        // so "open in new tab" skips javascript:/mailto:/fragment links (those
        // fall back to a normal click). '' marks a non-link clickable.
        const href = (el.tagName === 'A' && /^https?:/i.test(el.href)) ? el.href : '';
        out.push(r.left, r.top, r.width, r.height, href);
    }
    return out;
})()
"#;

/// Servo options: with `persist_site_data` on, point `config_dir` at the
/// `servo/` subfolder of the user data dir — Servo's net and storage threads
/// then load cookies / HSTS / localStorage from it at startup and write them
/// back on a clean shutdown (see [`AppBrowser::shutdown`]), so logins survive
/// restarts. The subfolder keeps Servo's files apart from retsurf's own.
fn build_opts(config: &BrowserConfig) -> servo::Opts {
    let mut opts = servo::Opts::default();
    if config.persist_site_data {
        opts.config_dir = Some(std::path::PathBuf::from(crate::config::servo_data_dir()));
    }
    opts
}

/// Servo preferences sized to the hardware (see [`PerformanceConfig`]) plus
/// the configured user agent. These must go through `ServoBuilder` — the
/// thread pools are created at startup, so `set_preference` after `build()`
/// would be too late.
fn build_preferences(config: &BrowserConfig, perf: &PerformanceConfig) -> servo::Preferences {
    let cores = std::thread::available_parallelism()
        .map(std::num::NonZeroUsize::get)
        .unwrap_or(4) as i64;

    // The memory profile is the holistic baseline: JS GC ceilings, back-forward
    // cache depth, HTTP/canvas caches, which DOM subsystems start, and tier-sized
    // thread counts. `auto` resolves from the build target + detected RAM.
    let profile = memory::resolve(perf.memory_profile);
    let mut prefs = memory::preferences(profile);

    // Tiers hardcode thread counts for their assumed core count (e.g. Generous
    // assumes an octa-core A527); clamp down so a quad-core board isn't
    // oversubscribed. Only ever clamps down — never raises the tier's choice.
    // Desktop is left untouched: it's Servo's own defaults, run as upstream ships.
    if profile != crate::config::MemoryProfile::Desktop {
        let cores_u = cores as u64;
        prefs.layout_threads = prefs.layout_threads.clamp(1, cores);
        let clamp = |v: &mut u64| *v = (*v).clamp(1, cores_u);
        clamp(&mut prefs.thread_pool_async_runtime_workers_max);
        clamp(&mut prefs.thread_pool_fallback_workers);
        clamp(&mut prefs.thread_pool_workers_max);
        clamp(&mut prefs.thread_pool_webrender_workers_max);
    }

    // The explicit [performance] knobs still win when set (non-zero); `0` keeps
    // the tier's choice.
    if perf.layout_threads != 0 {
        prefs.layout_threads = perf.layout_threads as i64;
    }
    if perf.worker_pool_max != 0 {
        let n = perf.worker_pool_max as u64;
        prefs.thread_pool_async_runtime_workers_max = n;
        prefs.thread_pool_fallback_workers = n;
        prefs.thread_pool_workers_max = n;
        prefs.thread_pool_webrender_workers_max = n;
    }

    if let Some(ua) = resolve_user_agent(&config.user_agent) {
        log::info!("user agent: {ua}");
        prefs.user_agent = ua;
    }

    log::info!(
        "servo: {cores} cores, memory profile `{}` -> layout={}, webrender pool={}, js_mem_max={}",
        profile.as_str(),
        prefs.layout_threads,
        prefs.thread_pool_webrender_workers_max,
        prefs.js_mem_max,
    );
    prefs
}

/// Resolve the `[browser] user_agent` config value: empty (or `default`)
/// keeps Servo's platform default, the keywords pick a stock UA string, and
/// anything else is sent verbatim. `mobile` is the interesting one on a
/// handheld — sites serve their phone layouts, which fit a small screen far
/// better than the desktop ones.
fn resolve_user_agent(value: &str) -> Option<String> {
    let value = value.trim();
    let platform = match value.to_ascii_lowercase().as_str() {
        "" | "default" => return None,
        "desktop" => servo::UserAgentPlatform::Desktop,
        "mobile" | "android" => servo::UserAgentPlatform::Android,
        "ios" => servo::UserAgentPlatform::Ios,
        _ => return Some(value.to_string()),
    };
    Some(platform.to_user_agent_string())
}

fn set_experimental_prefs(servo: &servo::Servo, value: bool) {
    let value = servo::PrefValue::Bool(value);

    for pref in EXPERIMENTAL_PREFS {
        servo.set_preference(pref, value.clone());
    }
}
