use crate::browser::{AppBrowser, BrowserCommand};
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

#[derive(Clone)]
pub enum AppCommand {
    Shutdown,
    Resize(u32, u32),
    Browser(BrowserCommand),
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
        let browser = AppBrowser::new(&window, event_sender, &config.browser)?;
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
        self.browser.open_tab(&self.config.browser.home_page);
        self.state = AppState::Running;
        let mut commands = Vec::with_capacity(4);

        while self.browser.pump_event_loop() {
            self.event_handler
                .wait(&self.window, &mut self.ui, &mut self.browser, &mut commands);
            self.ui.update(&mut self.browser, &mut commands);

            for command in commands.iter() {
                self.execute_command(command);
            }

            commands.clear();
            self.draw();
        }

        self.browser.deinit();
        self.ui.destroy();
    }

    fn execute_command(&mut self, command: &AppCommand) {
        match command {
            AppCommand::Shutdown => self.shutdown(),
            AppCommand::Resize(w, h) => self.browser.resize(*w, *h),
            AppCommand::Browser(command) => {
                self.browser.execute_command(command, &self.config.browser)
            }
        };
    }

    fn shutdown(&mut self) {
        self.state = AppState::ShuttingDown;
        self.browser.start_shutting_down();
    }

    fn draw(&mut self) {
        let painted = self.browser.paint();
        self.ui.draw(&self.window, painted);
    }
}
