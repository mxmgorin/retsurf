use crate::{config::InterfaceConfig, render::AppRender};
use sdl2::Sdl;

pub struct AppWindow {
    renderer: AppRender,
}

impl AppWindow {
    pub fn new(sdl: &Sdl, config: &InterfaceConfig) -> Result<Self, String> {
        let renderer = AppRender::new(sdl, config);
        Ok(Self { renderer })
    }

    pub fn show(&mut self) {
        self.renderer.show();
    }

    pub fn close(self) {
        self.renderer.deinit();
    }
}
