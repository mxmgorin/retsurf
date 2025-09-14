use crate::browser::AppBrowser;
use crate::input::handler::InputHandler;
use crate::resources::AppResources;
use crate::{config::AppConfig, window::AppWindow};
use sdl2::Sdl;

#[derive(PartialEq)]
pub enum AppState {
    Running,
    Quitting,
}

pub enum AppCmd {
    Quit = 0,
    Draw = 1,
    Update = 2,
}

pub struct App {
    config: AppConfig,
    window: AppWindow,
    state: AppState,
    pub browser: AppBrowser,
    _resources: AppResources,
}

impl App {
    pub fn new(sdl: &mut Sdl, config: AppConfig) -> Result<Self, String> {
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
        self.browser.open_tab(&self.config.home_url);
        self.browser.update();
        self.draw();
        while self.state == AppState::Running {

            input.wait_event(&mut self);
        }

        self.window.close();
    }

    pub fn handle_cmd(&mut self, cmd: AppCmd) {
        match cmd {
            AppCmd::Quit => self.state = AppState::Quitting,
            AppCmd::Draw => self.draw(),
            AppCmd::Update => self.browser.update(),
        }
    }

    fn draw(&self) {
        self.browser.draw();
        self.window.show();
    }
}
