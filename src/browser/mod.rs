//! The embedded Servo browser: tab list, painting, input, and commands. All
//! reactions to Servo (the `WebViewDelegate` impl, including download
//! interception and ad blocking) live in [`delegate`]; address-bar text
//! interpretation in [`url`].

mod delegate;
mod url;

pub use url::try_into_url;

use crate::{
    adblock::Adblock,
    config::BrowserConfig,
    event::user::UserEventSender,
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
}

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
}

impl AppBrowserInner {
    pub fn new(
        servo: servo::Servo,
        rendering_ctx: Rc<dyn RenderingContext>,
        event_sender: UserEventSender,
        download_exts: Vec<String>,
        adblock: Adblock,
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
        download_exts: Vec<String>,
        adblock: Adblock,
    ) -> Result<Self, String> {
        // Path B: Servo renders into an FBO in SDL2's shared GL context
        // (see `SdlRenderingContext`); egui composites that FBO's texture.
        let servo = servo::ServoBuilder::default()
            .event_loop_waker(event_sender.clone_box())
            .build();
        set_experimental_prefs(&servo, config.experimental_prefs_enabled);
        let inner = AppBrowserInner::new(
            servo,
            rendering_ctx,
            event_sender.clone(),
            download_exts,
            adblock,
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
                    active: i == active,
                }
            })
            .collect()
    }

    /// Open a new tab at `url` and make it the active (shown) one.
    pub fn open_tab(&mut self, url: &str) {
        let url = Url::parse(url).unwrap();
        let webview =
            servo::WebViewBuilder::new(&self.inner.servo, self.inner.rendering_ctx.clone())
                .url(url)
                .hidpi_scale_factor(euclid::Scale::new(1.0))
                .delegate(self.inner.clone())
                .build();

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
        }
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

fn set_experimental_prefs(servo: &servo::Servo, value: bool) {
    let value = servo::PrefValue::Bool(value);

    for pref in EXPERIMENTAL_PREFS {
        servo.set_preference(pref, value.clone());
    }
}
