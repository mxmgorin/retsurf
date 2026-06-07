use crate::{
    config::BrowserConfig,
    event::user::{UserEvent, UserEventSender},
};
use servo::{EventLoopWaker, RenderingContext, WebView};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
use url::Url;

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
}

impl Default for BrowserState {
    fn default() -> Self {
        Self {
            location: "".into(),
            load_status: servo::LoadStatus::Complete,
        }
    }
}

struct AppBrowserInner {
    webviews: RefCell<Vec<WebView>>,
    event_sender: UserEventSender,
    servo: servo::Servo,
    rendering_ctx: Rc<dyn RenderingContext>,
    repaint_pending: Cell<bool>,
    state: RefCell<BrowserState>,
}

impl AppBrowserInner {
    pub fn new(
        servo: servo::Servo,
        rendering_ctx: Rc<dyn RenderingContext>,
        event_sender: UserEventSender,
    ) -> Self {
        Self {
            webviews: RefCell::new(vec![]),
            event_sender,
            servo,
            rendering_ctx,
            repaint_pending: Cell::new(false),
            state: RefCell::new(BrowserState::default()),
        }
    }

    pub fn add_webview(&self, tab: WebView) {
        self.webviews.borrow_mut().push(tab);
    }

    pub fn get_focused_webview(&self) -> Option<WebView> {
        self.webviews.borrow().last().cloned()
    }

    fn is_focused_webview(&self, id: servo::WebViewId) -> bool {
        if let Some(focused) = self.get_focused_webview() {
            return focused.id() == id;
        }

        false
    }
}

impl servo::WebViewDelegate for AppBrowserInner {
    fn notify_new_frame_ready(&self, _: WebView) {
        self.repaint_pending.set(true);
        self.event_sender.send(UserEvent::BrowserFrameReady);
    }

    fn notify_url_changed(&self, webview: WebView, url: Url) {
        if self.is_focused_webview(webview.id()) {
            self.state.borrow_mut().location = url.to_string();
        }
    }

    fn notify_load_status_changed(&self, webview: WebView, status: servo::LoadStatus) {
        if self.is_focused_webview(webview.id()) {
            self.state.borrow_mut().load_status = status;
        }
    }
}

impl AppBrowser {
    pub fn new(
        rendering_ctx: Rc<dyn RenderingContext>,
        event_sender: UserEventSender,
        config: &BrowserConfig,
    ) -> Result<Self, String> {
        // Path B: Servo renders into an FBO in SDL2's shared GL context
        // (see `SdlRenderingContext`); egui composites that FBO's texture.
        let servo = servo::ServoBuilder::default()
            .event_loop_waker(event_sender.clone_box())
            .build();
        set_experimental_prefs(&servo, config.experimental_prefs_enabled);
        let inner = AppBrowserInner::new(servo, rendering_ctx, event_sender.clone());

        Ok(Self {
            inner: Rc::new(inner),
        })
    }


    #[inline]
    pub fn is_animating(&self) -> bool {
        self.inner
            .get_focused_webview()
            .map(|tab| tab.animating())
            .unwrap_or(false)
    }

    #[inline]
    pub fn get_state_mut(&mut self) -> std::cell::RefMut<'_, BrowserState> {
        self.inner.state.borrow_mut()
    }

    pub fn open_tab(&mut self, url: &str) {
        let url = url::Url::parse(url).unwrap();
        let webview =
            servo::WebViewBuilder::new(&self.inner.servo, self.inner.rendering_ctx.clone())
                .url(url)
                .hidpi_scale_factor(euclid::Scale::new(1.0))
                .delegate(self.inner.clone())
                .build();

        webview.focus();
        self.inner.add_webview(webview);
    }

    /// Spin the Servo event loop once, running delegate callbacks and updating paint output.
    #[inline]
    pub fn pump_event_loop(&self) {
        self.inner.servo.spin_event_loop();
    }

    /// Paint the contents of the focused WebView into its RenderingContext. Returns true if a paint was performed.
    pub fn paint(&self) -> bool {
        if !self.inner.repaint_pending.get() {
            return false;
        }

        if let Some(tab) = self.inner.get_focused_webview() {
            self.inner.repaint_pending.set(false);
            tab.paint();
            return true;
        }

        false
    }

    pub fn handle_input(&self, event: servo::InputEvent) {
        let Some(tab) = self.inner.get_focused_webview() else {
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

    pub fn execute_command(&mut self, command: &BrowserCommand, config: &BrowserConfig) {
        match command {
            BrowserCommand::Back => _ = self.inner.get_focused_webview().map(|x| x.go_back(1)),
            BrowserCommand::Foward => _ = self.inner.get_focused_webview().map(|x| x.go_forward(1)),
            BrowserCommand::Reload => _ = self.inner.get_focused_webview().map(|x| x.reload()),
            BrowserCommand::Load => {
                let location = &self.inner.state.borrow().location;
                let Some(url) = try_into_url(location, &config.search_page) else {
                    log::warn!("failed to parse location");
                    return;
                };

                self.inner.get_focused_webview().map(|x| x.load(url));
            }
        }
    }

    pub fn resize(&self, w: u32, h: u32) {
        if w == 0 || h == 0 {
            return;
        }
        let size = dpi::PhysicalSize::new(w, h);
        self.inner.rendering_ctx.resize(size);
        if let Some(tab) = self.inner.get_focused_webview() {
            tab.resize(size);
        }
    }
}

/// Interpret an input URL.
///
/// If this is not a valid URL, try to "fix" it by adding a scheme or if all else fails,
/// interpret the string as a search term.
pub fn try_into_url<S: AsRef<str>>(request: S, searchpage: &str) -> Option<Url> {
    let request = request.as_ref().trim();

    Url::parse(request)
        .ok()
        .or_else(|| try_as_file(request))
        .or_else(|| try_as_domain(request))
        .or_else(|| try_as_search_page(request, searchpage))
}

fn try_as_file(request: &str) -> Option<Url> {
    if request.starts_with('/') {
        return Url::parse(&format!("file://{}", request)).ok();
    }
    None
}

fn try_as_domain(request: &str) -> Option<Url> {
    fn is_domain_like(s: &str) -> bool {
        !s.starts_with('/') && s.contains('/')
            || (!s.contains(' ') && !s.starts_with('.') && s.split('.').count() > 1)
    }

    if !request.contains(' ') && servo::is_reg_domain(request) || is_domain_like(request) {
        return Url::parse(&format!("https://{}", request)).ok();
    }

    None
}

fn try_as_search_page(request: &str, searchpage: &str) -> Option<Url> {
    if request.is_empty() {
        return None;
    }

    Url::parse(&searchpage.replace("%s", request)).ok()
}

fn set_experimental_prefs(servo: &servo::Servo, value: bool) {
    let value = servo::PrefValue::Bool(value);

    for pref in EXPERIMENTAL_PREFS {
        servo.set_preference(pref, value.clone());
    }
}
