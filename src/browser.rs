use crate::window::AppWindow;

pub struct AppBrowser {
    servo: servo::Servo,
}

impl Drop for AppBrowser {
    fn drop(&mut self) {
        self.servo.deinit();
    }
}

impl AppBrowser {
    pub fn new(window: &AppWindow) -> Result<Self, String> {
        log::info!("new app browser");
        let builder = servo::ServoBuilder::new(window.get_rendering_ctx());
        let servo = builder.build();

        Ok(Self { servo })
    }

    pub fn go(&mut self, url: &str) {
        let url = url::Url::parse(url).unwrap();
        let webview = servo::WebViewBuilder::new(&self.servo)
            .url(url)
            // .delegate(app_state.clone())
            .build();

        webview.focus_and_raise_to_top(true);
    }
}
