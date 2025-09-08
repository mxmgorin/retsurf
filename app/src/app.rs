use crate::{config::AppConfig, window::AppWindow};
use sdl2::Sdl;
use std::thread;
use std::time::Duration;

pub struct App {
    config: AppConfig,
    window: AppWindow,
}

impl App {
    pub fn new(sdl: &mut Sdl, config: AppConfig) -> Result<Self, String> {
        let window = AppWindow::new(sdl, &config.interface)?;

        Ok(Self { config, window })
    }

    pub fn run(&mut self) {
        loop {
            self.window.show();
            thread::sleep(Duration::from_millis(30));
        }
    }
}
