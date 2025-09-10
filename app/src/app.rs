use crate::input::handler::InputHandler;
use crate::{config::AppConfig, window::AppWindow};
use sdl2::Sdl;
use std::thread;
use std::time::Duration;

#[derive(PartialEq)]
pub enum AppState {
    Running,
    Quitting,
}

pub enum AppCmd {
    Quit,
}

pub struct App {
    config: AppConfig,
    window: AppWindow,
    state: AppState,
}

impl App {
    pub fn new(sdl: &mut Sdl, config: AppConfig) -> Result<Self, String> {
        let window = AppWindow::new(sdl, &config.interface)?;

        Ok(Self { config, window, state: AppState::Running })
    }

    pub fn run(mut self, input: &mut InputHandler) {
        while self.state == AppState::Running {
            input.handle_events(&mut self);
            self.window.show();
            thread::sleep(Duration::from_millis(30));
        }

        self.window.close();
    }

    pub fn handle_cmd(&mut self, cmd: AppCmd) {
        match cmd {
            AppCmd::Quit => self.state = AppState::Quitting,
        }
    }
}
