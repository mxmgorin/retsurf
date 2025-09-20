use std::cell::Cell;
use std::time::Duration;

use crate::browser::AppBrowser;
use crate::event::handler::AppEventHandler;
use crate::ui::AppUi;
use crate::{config::AppConfig, window::AppWindow};
use sdl2::Sdl;

#[derive(PartialEq)]
pub enum AppState {
    Initialized,
    Running,
    Quitting,
}

pub enum AppCommand {
    Quit,
    Draw,
    Update,
    HandleInput(servo::InputEvent),
    Resize(u32, u32),
}

pub struct App {
    event_handler: AppEventHandler,
    config: AppConfig,
    window: AppWindow,
    state: AppState,
    browser: AppBrowser,
    ui: AppUi,
    event_timeout: Cell<Duration>,
}

impl App {
    pub fn new(sdl: &mut Sdl, config: AppConfig) -> Result<Self, String> {
        let window = AppWindow::new(sdl, &config.interface)?;
        let browser = AppBrowser::new(&window)?;
        let event_handler = AppEventHandler::new(sdl)?;
        let ui = AppUi::new(&window);

        Ok(Self {
            config,
            window,
            browser,
            event_handler,
            ui,
            state: AppState::Initialized,
            event_timeout: Cell::new(Duration::from_secs(1)),
        })
    }

    pub fn run(mut self) {
        self.browser
            .toggle_experimental_prefs(self.config.browser.experimental_prefs_enabled);
        self.browser.open_tab(&self.config.browser.home_url);
        self.state = AppState::Running;

        while self.state == AppState::Running {
            let commands = self.event_handler.wait(self.event_timeout.get());
            self.event_timeout.set(self.ui.update(&self.browser));

            for command in commands {
                self.execute_command(command);
            }
        }

        self.browser.shutdown();
        self.ui.destroy();
    }

    fn execute_command(&mut self, command: AppCommand) {
        match command {
            AppCommand::Quit => self.state = AppState::Quitting,
            AppCommand::Draw => self.draw(),
            AppCommand::Update => self.update(),
            AppCommand::HandleInput(input_event) => self.browser.handle_input(input_event),
            AppCommand::Resize(w, h) => self.browser.resize(w, h),
        }
    }

    fn update(&mut self) {
        self.browser.update();
    }

    fn draw(&mut self) {
        self.browser.paint();
        self.window.prepare_for_rendering();
        self.ui.paint(self.window.size());
        self.window.present();
    }
}
