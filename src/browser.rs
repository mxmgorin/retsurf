use crate::{
    input::user::{UserEvent, UserEventSender},
    resources::ServoResources,
    window::AppWindow,
};
use servo::{EventLoopWaker, WebView};
use std::{cell::RefCell, rc::Rc};

pub struct AppBrowser {
    inner: Rc<AppBrowserInner>,
}

struct AppBrowserInner {
    tabs: RefCell<Vec<WebView>>,
    event_sender: UserEventSender,
    servo: servo::Servo,
}

impl AppBrowserInner {
    pub fn new(servo: servo::Servo, event_sender: UserEventSender) -> Self {
        Self {
            tabs: RefCell::new(vec![]),
            event_sender,
            servo,
        }
    }

    pub fn add_tab(&self, tab: WebView) {
        self.tabs.borrow_mut().push(tab);
    }

    pub fn get_focused_tab(&self) -> Option<WebView> {
        self.tabs.borrow().last().cloned()
    }
}

impl servo::WebViewDelegate for AppBrowserInner {
    fn notify_new_frame_ready(&self, _: WebView) {
        self.event_sender.send(UserEvent::FrameReady);
    }

    fn request_open_auxiliary_webview(&self, parent_webview: WebView) -> Option<WebView> {
        let webview = servo::WebViewBuilder::new_auxiliary(&self.servo)
            .hidpi_scale_factor(servo::euclid::Scale::new(1.0))
            .delegate(parent_webview.delegate())
            .build();
        webview.focus_and_raise_to_top(true);
        self.add_tab(webview.clone());

        Some(webview)
    }
}

impl AppBrowser {
    pub fn new(window: &AppWindow) -> Result<Self, String> {
        let event_sender = UserEventSender::new();
        ServoResources::init();
        let builder = servo::ServoBuilder::new(window.get_rendering_ctx())
            .event_loop_waker(event_sender.clone_box());
        let servo = builder.build();

        Ok(Self {
            inner: Rc::new(AppBrowserInner::new(servo, event_sender)),
        })
    }

    pub fn shutdown(&self) {
        self.inner.servo.start_shutting_down();
        self.inner.servo.deinit();
    }

    pub fn open_tab(&mut self, url: &str) {
        let url = url::Url::parse(url).unwrap();
        let webview = servo::WebViewBuilder::new(&self.inner.servo)
            .url(url)
            .delegate(self.inner.clone())
            .build();

        webview.focus_and_raise_to_top(true);
        self.inner.add_tab(webview);
    }

    pub fn update(&self) {
        self.inner.servo.spin_event_loop();
    }

    pub fn draw(&self) {
        if let Some(tab) = self.inner.get_focused_tab() {
            tab.paint();
        }
    }

    pub fn handle_input(&self, event: servo::InputEvent) {
        if let Some(tab) = self.inner.get_focused_tab() {
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
    }

    pub fn resize(&self, w: u32, h: u32) {
        if let Some(tab) = self.inner.get_focused_tab() {
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
