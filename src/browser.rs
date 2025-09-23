use crate::{
    config::BrowserConfig,
    event::user::{UserEvent, UserEventSender},
    resources::ServoResources,
    window::AppWindow,
};
use servo::{EventLoopWaker, WebView};
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
    Go(String),
}

static EXPERIMENTAL_PREFS: &[&str] = &[
    "dom_async_clipboard_enabled",
    "dom_fontface_enabled",
    "dom_intersection_observer_enabled",
    "dom_mouse_event_which_enabled",
    "dom_navigator_sendbeacon_enabled",
    "dom_notification_enabled",
    "dom_offscreen_canvas_enabled",
    "dom_permissions_enabled",
    "dom_resize_observer_enabled",
    "dom_trusted_types_enabled",
    "dom_webgl2_enabled",
    "dom_webgpu_enabled",
    "dom_xpath_enabled",
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
    repaint_pending: Cell<bool>,
    state: RefCell<BrowserState>,
}

impl AppBrowserInner {
    pub fn new(servo: servo::Servo, event_sender: UserEventSender) -> Self {
        Self {
            webviews: RefCell::new(vec![]),
            event_sender,
            servo,
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

    fn request_open_auxiliary_webview(&self, parent_webview: WebView) -> Option<WebView> {
        let webview = servo::WebViewBuilder::new_auxiliary(&self.servo)
            .hidpi_scale_factor(servo::euclid::Scale::new(1.0))
            .delegate(parent_webview.delegate())
            .build();
        webview.focus_and_raise_to_top(true);
        self.add_webview(webview.clone());

        Some(webview)
    }
}

impl AppBrowser {
    pub fn new(
        window: &AppWindow,
        event_sender: UserEventSender,
        config: &BrowserConfig,
    ) -> Result<Self, String> {
        ServoResources::init();
        let rendering_ctx = window.get_offscreen_rendering_ctx();
        let builder =
            servo::ServoBuilder::new(rendering_ctx).event_loop_waker(event_sender.clone_box());
        let servo = builder.build();
        set_experimental_prefs(&servo, config.experimental_prefs_enabled);
        let inner = AppBrowserInner::new(servo, event_sender.clone());

        Ok(Self {
            inner: Rc::new(inner),
        })
    }

    pub fn deinit(&self) {
        self.inner.servo.deinit();
    }

    pub fn get_state_mut(&mut self) -> std::cell::RefMut<'_, BrowserState> {
        self.inner.state.borrow_mut()
    }

    pub fn start_shutting_down(&self) {
        self.inner.servo.start_shutting_down();
    }

    pub fn open_tab(&mut self, url: &str) {
        let url = url::Url::parse(url).unwrap();
        let webview = servo::WebViewBuilder::new(&self.inner.servo)
            .url(url)
            .delegate(self.inner.clone())
            .build();

        webview.focus_and_raise_to_top(true);
        self.inner.add_webview(webview);
    }

    /// False indicates that no need to pump any more
    pub fn pump_event_loop(&self) -> bool {
        self.inner.servo.spin_event_loop()
    }

    pub fn paint(&self) -> bool {
        if !self.inner.repaint_pending.get() {
            return false;
        }

        if let Some(tab) = self.inner.get_focused_webview() {
            self.inner.repaint_pending.set(false);
            return tab.paint();
        }

        false
    }

    pub fn handle_input(&self, event: servo::InputEvent) {
        let Some(tab) = self.inner.get_focused_webview() else {
            return;
        };

        tab.notify_input_event(event.clone());

        match event {
            servo::InputEvent::Wheel(we) => {
                let (dx, dy) = into_scroll_delta(we.delta);
                let (x, y) = we.point.to_i32().to_tuple();
                scroll(&tab, dx, dy, x, y);
                self.inner.servo.spin_event_loop(); // doesn't scroll without this
            }
            servo::InputEvent::MouseButton(be) => {
                if be.action == servo::MouseButtonAction::Down {
                    match be.button {
                        servo::MouseButton::Left
                        | servo::MouseButton::Middle
                        | servo::MouseButton::Right
                        | servo::MouseButton::Other(_) => {}
                        servo::MouseButton::Back => _ = tab.go_back(1),
                        servo::MouseButton::Forward => _ = tab.go_forward(1),
                    }
                }
            }
            _ => {}
        }
    }

    pub fn execute_command(&mut self, command: &BrowserCommand, config: &BrowserConfig) {
        match command {
            BrowserCommand::Back => _ = self.inner.get_focused_webview().map(|x| x.go_back(1)),
            BrowserCommand::Foward => _ = self.inner.get_focused_webview().map(|x| x.go_forward(1)),
            BrowserCommand::Reload => _ = self.inner.get_focused_webview().map(|x| x.reload()),
            BrowserCommand::Go(location) => {
                let Some(url) = try_into_url(location, &config.search_page) else {
                    log::warn!("failed to parse location");
                    return;
                };

                self.inner.get_focused_webview().map(|x| x.load(url));
            }
        }
    }

    pub fn resize(&self, w: u32, h: u32) {
        if let Some(tab) = self.inner.get_focused_webview() {
            let mut rect = tab.rect();
            rect.set_size(servo::euclid::Size2D::new(w as f32, h as f32));
            tab.move_resize(rect);
            tab.resize(dpi::PhysicalSize::new(w, h));
        }
    }
}

fn scroll(tab: &WebView, dx: f32, dy: f32, x: i32, y: i32) {
    let location =
        servo::webrender_api::ScrollLocation::Delta(-servo::euclid::Vector2D::new(dx, dy));
    let point = servo::webrender_api::units::DeviceIntPoint::new(x, y);
    tab.notify_scroll_event(location, point);
}

fn into_scroll_delta(wd: servo::WheelDelta) -> (f32, f32) {
    let dx = wd.x as f32;
    let dy = wd.y as f32;

    let (dx, dy) = match wd.mode {
        servo::WheelMode::DeltaPixel => (dx * 4.0, dy * 4.0),
        servo::WheelMode::DeltaLine => (dx * 76.0, dy * 76.0),
        servo::WheelMode::DeltaPage => unreachable!(),
    };

    // Scroll events snap to the major axis of movement, with vertical
    // preferred over horizontal.
    // if dy.abs() >= dx.abs() {
    //     dx = 0.0;
    // } else {
    //     dy = 0.0;
    // }

    (dx, dy)
}

/// Interpret an input URL.
///
/// If this is not a valid URL, try to "fix" it by adding a scheme or if all else fails,
/// interpret the string as a search term.
pub fn try_into_url(request: &str, searchpage: &str) -> Option<Url> {
    let request = request.trim();

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

    if !request.contains(' ') && servo::net_traits::pub_domains::is_reg_domain(request)
        || is_domain_like(request)
    {
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
