use crate::browser::AppBrowser;
use crate::event::handler::AppEventHandler;
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
}

impl App {
    pub fn new(sdl: &mut Sdl, config: AppConfig) -> Result<Self, String> {
        let window = AppWindow::new(sdl, &config.interface)?;
        let browser = AppBrowser::new(&window)?;
        let event_handler = AppEventHandler::new(sdl)?;

        Ok(Self {
            config,
            window,
            browser,
            event_handler,
            state: AppState::Initialized,
        })
    }

    pub fn run(mut self) {
        self.browser
            .toggle_experimental_prefs(self.config.browser.experimental_prefs_enabled);
        self.browser.open_tab(&self.config.browser.home_url);
        self.state = AppState::Running;

        while self.state == AppState::Running {
            let commands = self.event_handler.wait();

            for command in commands {
                self.execute_command(command);
            }
        }

        self.browser.shutdown();
    }

    fn execute_command(&mut self, command: AppCommand) {
        match command {
            AppCommand::Quit => self.state = AppState::Quitting,
            AppCommand::Draw => self.draw(),
            AppCommand::Update => self.browser.update(),
            AppCommand::HandleInput(input_event) => self.browser.handle_input(input_event),
            AppCommand::Resize(w, h) => self.browser.resize(w, h),
        }
    }

    fn draw(&mut self) {
        self.browser.draw();
        self.window.show();
    }
}
