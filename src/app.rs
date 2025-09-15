use crate::browser::AppBrowser;
use crate::input::handler::AppInputHandler;
use crate::{config::AppConfig, window::AppWindow};
use sdl2::Sdl;

#[derive(PartialEq)]
pub enum AppState {
    Running,
    Quitting,
}

pub enum AppCmd {
    Quit,
    Draw,
    Update,
    HandleInput(servo::InputEvent),
    Resize(u32, u32),
}

pub struct App {
    input_handler: AppInputHandler,
    config: AppConfig,
    window: AppWindow,
    state: AppState,
    browser: AppBrowser,
}

impl App {
    pub fn new(sdl: &mut Sdl, config: AppConfig) -> Result<Self, String> {
        let window = AppWindow::new(sdl, &config.interface)?;
        let browser = AppBrowser::new(&window)?;
        let input_handler = AppInputHandler::new(sdl)?;

        Ok(Self {
            config,
            window,
            browser,
            input_handler,
            state: AppState::Running,
        })
    }

    pub fn run(mut self) {
        self.browser.open_tab(&self.config.home_url);

        while self.state == AppState::Running {
            let cmds = self.input_handler.wait_event();

            for cmd in cmds {
                self.handle_cmd(cmd);
            }
        }

        self.browser.shutdown();
    }

    fn handle_cmd(&mut self, cmd: AppCmd) {
        match cmd {
            AppCmd::Quit => self.state = AppState::Quitting,
            AppCmd::Draw => self.draw(),
            AppCmd::Update => self.browser.update(),
            AppCmd::HandleInput(input_event) => self.browser.handle_input(input_event),
            AppCmd::Resize(w, h) => self.browser.resize(w, h),
        }
    }

    fn draw(&self) {
        self.browser.draw();
        self.window.show();
    }
}
