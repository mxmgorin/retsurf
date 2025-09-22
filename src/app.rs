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
    ShuttingDown,
}

pub enum AppCommand {
    Shutdown,
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

        while self.browser.pump_event_loop() {
            let commands = self.event_handler.wait(&self.window, &mut self.ui);

            for command in commands {
                self.execute_command(command);
            }
        }

        self.browser.deinit();
        self.ui.destroy();
    }

    fn execute_command(&mut self, command: AppCommand) {
        match command {
            AppCommand::Shutdown => self.shutdown(),
            AppCommand::Draw => self.draw(),
            AppCommand::HandleInput(input_event) => self.browser.handle_input(input_event),
            AppCommand::Resize(w, h) => self.browser.resize(w, h),
        }
    }

    fn shutdown(&mut self) {
        self.state = AppState::ShuttingDown;
        self.browser.start_shutting_down();
    }

    fn draw(&mut self) {
        self.ui.update(&self.window, &self.browser);
        self.browser.paint();
        self.window.prepare_for_rendering();
        self.ui.paint(&self.window);
        self.window.present();
    }
}
