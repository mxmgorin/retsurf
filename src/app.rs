use crate::browser::AppBrowser;
use crate::input::handler::InputHandler;
use crate::resources::AppResources;
use crate::{config::AppConfig, window::AppWindow};
use sdl2::Sdl;
use std::sync::mpsc::Sender;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

#[derive(Clone)]
struct AppEventLoopWaker {
    sender: Arc<Sender<()>>,
}

impl servo::EventLoopWaker for AppEventLoopWaker {
    fn wake(&self) {
        self.sender.send(()).unwrap();
    }

    fn clone_box(&self) -> Box<dyn servo::EventLoopWaker> {
        Box::new(AppEventLoopWaker {
            sender: self.sender.clone(),
        })
    }
}
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
    browser: AppBrowser,
    _resources: AppResources,
}

impl App {
    pub fn new(sdl: &mut Sdl, config: AppConfig) -> Result<Self, String> {
        log::info!("new app");
        let resources = AppResources::new();
        let window = AppWindow::new(sdl, &config.interface)?;
        let browser = AppBrowser::new(&window)?;

        Ok(Self {
            config,
            window,
            browser,
            _resources: resources,
            state: AppState::Running,
        })
    }

    pub fn run(mut self, input: &mut InputHandler) {
        log::info!("Run app");

        while self.state == AppState::Running {
            input.handle_events(&mut self);
            self.window.update();
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
