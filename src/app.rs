use crate::browser::{AppBrowser, BrowserCommand};
use crate::event::gamepad::Gamepad;
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
    gamepad: Gamepad,
}

impl App {
    pub fn new(sdl: &mut Sdl, config: AppConfig) -> Result<Self, String> {
        log::info!("init: creating window");
        let window = AppWindow::new(sdl, &config.interface)?;
        log::info!("init: window ready; creating browser");
        let event_sender = UserEventSender::new();
        let browser =
            AppBrowser::new(window.get_rendering_ctx(), event_sender, &config.browser)?;
        log::info!("init: browser ready; creating event handler + ui");
        let event_handler = AppEventHandler::new(sdl)?;
        let ui = AppUi::new(&window);
        log::info!("init: app constructed");

        Ok(Self {
            config,
            window,
            browser,
            event_handler,
            ui,
            gamepad: Gamepad::new(),
            state: AppState::Initialized,
        })
    }

    pub fn run(mut self) {
        self.browser.open_tab(&self.config.browser.home_page);
        self.state = AppState::Running;
        let mut commands = Vec::with_capacity(4);

        while self.state == AppState::Running {
            self.browser.pump_event_loop();
            self.event_handler.wait(
                &self.window,
                &mut self.ui,
                &mut self.browser,
                &mut self.gamepad,
                &mut commands,
            );

            // Advance the gamepad cursor/scroll and feed input to the page.
            self.gamepad.tick(&self.window, &mut self.ui, &self.browser);

            // Render Servo into its FBO; egui composites that FBO's texture.
            self.browser.paint();

            self.ui.update(&mut self.browser, &mut commands);

            for command in commands.iter() {
                self.execute_command(command);
            }

            commands.clear();
            self.draw();
        }

        self.ui.destroy();

        // Servo's SoftwareRenderingContext does not destroy its surfman context on
        // drop, which trips surfman's "destroy explicitly" guard and panics during
        // unwinding. Exit before running destructors; the OS reclaims everything.
        std::process::exit(0);
    }

    fn execute_command(&mut self, command: &AppCommand) {
        match command {
            AppCommand::Shutdown => self.shutdown(),
            // Resizes are handled reactively: egui tracks the window size and
            // `AppUi::update` resizes the browser viewport to the central area.
            AppCommand::Resize(..) => {}
            AppCommand::Browser(command) => {
                self.browser.execute_command(command, &self.config.browser)
            }
        };
    }

    fn shutdown(&mut self) {
        self.state = AppState::ShuttingDown;
    }

    fn draw(&mut self) {
        self.ui.draw(&self.window);
    }
}
