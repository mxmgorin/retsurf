use crate::{
    input::user::{UserEvent, UserEventSender},
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

    pub fn get_active_tab(&self) -> Option<WebView> {
        self.tabs.borrow().last().cloned()
    }
}

impl Drop for AppBrowserInner {
    fn drop(&mut self) {
        self.servo.deinit();
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
        let builder = servo::ServoBuilder::new(window.get_rendering_ctx())
            .event_loop_waker(event_sender.clone_box());
        let servo = builder.build();

        Ok(Self {
            inner: Rc::new(AppBrowserInner::new(servo, event_sender)),
        })
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
        if let Some(tab) = self.inner.get_active_tab() {
            tab.paint();
        }
    }
}
