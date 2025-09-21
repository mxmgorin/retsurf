use crate::browser::AppBrowser;
use crate::event::handler::AppEventHandler;
use crate::event::user::UserEventSender;
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
}

impl App {
    pub fn new(sdl: &mut Sdl, config: AppConfig) -> Result<Self, String> {
        let window = AppWindow::new(sdl, &config.interface)?;
        let event_sender = UserEventSender::new();
        let browser = AppBrowser::new(&window, event_sender)?;
        let event_handler = AppEventHandler::new(sdl)?;
        let ui = AppUi::new(&window);

        Ok(Self {
            config,
            window,
            browser,
            event_handler,
            ui,
            state: AppState::Initialized,
        })
    }

    pub fn run(mut self) {
        self.browser
            .toggle_experimental_prefs(self.config.browser.experimental_prefs_enabled);
        self.browser.open_tab(&self.config.browser.home_url);
        self.state = AppState::Running;

        while self.state == AppState::Running {
            let commands = self.event_handler.wait(&mut self.ui);

            if !self.browser.pump_event_loop() {
                self.state = AppState::Quitting;
            }

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
            AppCommand::HandleInput(input_event) => self.browser.handle_input(input_event),
            AppCommand::Resize(w, h) => self.browser.resize(w, h),
        }
    }

    fn draw(&mut self) {
        self.ui.update(&self.browser);
        self.browser.paint();
        self.window.prepare_for_rendering();
        self.ui.paint(self.window.size());
        self.window.present();
    }
}
