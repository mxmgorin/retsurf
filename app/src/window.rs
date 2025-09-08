use crate::{config::InterfaceConfig, render::sdl2::Sdl2Renderer};
use sdl2::Sdl;

pub struct AppWindow {
    renderer: Sdl2Renderer,
}

impl AppWindow {
    pub fn new(sdl: &Sdl, config: &InterfaceConfig) -> Result<Self, String> {
        let renderer = Sdl2Renderer::new(sdl, config);
        Ok(Self { renderer })
    }

    pub fn show(&mut self) {
        self.renderer.show();
    }
}
