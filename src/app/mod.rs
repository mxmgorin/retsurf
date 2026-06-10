//! The application core: construction, the main loop, and command execution.
//! The command vocabulary is defined in [`command`]; the contextual-input
//! routing (gamepad / keyboard intents against the current UI state) lives in
//! [`router`].

mod command;
mod router;

pub use command::{AppCommand, InputCommand, MenuAction};

use crate::adblock::Adblock;
use crate::browser::{AppBrowser, BrowserCommand};
use crate::event::handler::AppEventHandler;
use crate::event::user::UserEventSender;
use crate::menu::Section;
use crate::ui::AppUi;
use crate::{config::AppConfig, window::AppWindow};
use sdl2::Sdl;
use std::time::Instant;

#[derive(PartialEq)]
pub enum AppState {
    Initialized,
    Running,
    ShuttingDown,
}

pub struct App {
    event_handler: AppEventHandler,
    config: AppConfig,
    window: AppWindow,
    state: AppState,
    browser: AppBrowser,
    ui: AppUi,
    /// For handing to download workers so they can wake the idle-blocked loop.
    event_sender: UserEventSender,
    /// Router timing for analog motion (cursor-speed integration).
    last_tick: Instant,
    /// Keyboard grid-navigation auto-repeat: latched direction and next fire time.
    osk_nav_dir: (i32, i32),
    osk_nav_next: Instant,
}

impl App {
    pub fn new(sdl: &mut Sdl, config: AppConfig) -> Result<Self, String> {
        log::info!("init: creating window");
        let window = AppWindow::new(sdl, &config.interface)?;
        log::info!("init: window ready; creating browser");
        let event_sender = UserEventSender::new();
        let browser = AppBrowser::new(
            window.get_rendering_ctx(),
            event_sender.clone(),
            &config.browser,
            config.downloads.extensions.clone(),
            Adblock::new(&config.adblock),
        )?;
        log::info!("init: browser ready; creating event handler + ui");
        let event_handler = AppEventHandler::new(sdl, config.gamepad.clone())?;
        let ui = AppUi::new(&window, &config.interface, &config.history, &config.downloads);
        log::info!("init: app constructed");

        Ok(Self {
            config,
            window,
            browser,
            event_handler,
            ui,
            event_sender,
            state: AppState::Initialized,
            last_tick: Instant::now(),
            osk_nav_dir: (0, 0),
            osk_nav_next: Instant::now(),
        })
    }

    pub fn run(mut self) {
        self.browser.open_tab(&self.config.browser.home_page);
        self.state = AppState::Running;
        let mut commands = Vec::with_capacity(4);

        while self.state == AppState::Running {
            self.browser.pump_event_loop();

            // Record any pages the focused webview navigated to this frame. Sourced
            // from real navigations (not address-bar text), so typing doesn't log.
            for url in self.browser.take_visited() {
                self.ui.menu_record_history(&url);
            }

            self.event_handler
                .wait(&self.window, &mut self.ui, &mut self.browser, &mut commands);

            // Apply background download progress/finishes before building the UI,
            // and start any downloads the browser denied navigation for.
            self.ui.downloads_poll();
            for url in self.browser.take_download_requests() {
                self.ui.start_download(&url, &self.event_sender);
            }

            // Hint mode: hand freshly collected clickable rects to the UI, and
            // start a re-collect once a post-scroll refresh comes due.
            if let Some(rects) = self.browser.take_hint_rects() {
                self.ui.hints_apply(rects);
            }
            if self.ui.hints_refresh_due() {
                self.browser.collect_hints();
            }

            // Render Servo into its FBO; egui composites that FBO's texture.
            self.browser.paint();

            self.ui.update(&mut self.browser, &mut commands);

            // Drain in waves: routing a command (e.g. an OSK Enter) may queue more.
            while !commands.is_empty() {
                for command in std::mem::take(&mut commands) {
                    self.execute_command(&command, &mut commands);
                }
            }

            self.draw();
        }

        self.ui.destroy();

        // Servo's SoftwareRenderingContext does not destroy its surfman context on
        // drop, which trips surfman's "destroy explicitly" guard and panics during
        // unwinding. Exit before running destructors; the OS reclaims everything.
        std::process::exit(0);
    }

    fn execute_command(&mut self, command: &AppCommand, out: &mut Vec<AppCommand>) {
        match command {
            AppCommand::Shutdown => self.shutdown(),
            // On a window resize, size the browser to the new central area straight
            // away from the actual window (egui's reactive sizing can lag a frame).
            AppCommand::Resize => self.ui.resize_browser(&self.window, &self.browser),
            AppCommand::Browser(command) => {
                self.browser.execute_command(command, &self.config.browser)
            }
            AppCommand::Input(command) => self.route_input(command, out),
            AppCommand::Menu(action) => self.menu_action(action),
            AppCommand::ToggleBookmark => self.toggle_current_bookmark(),
        };

        // Commands are drained after `ui.update` already built this frame, so a
        // discrete command that changes UI state needs a follow-up frame to show —
        // otherwise the loop blocks on input and the change lingers unrendered. The
        // per-frame analog tick is excluded: it fires every frame and forcing a
        // repaint from it would spin the idle loop.
        if !matches!(
            command,
            AppCommand::Input(InputCommand::Analog { .. })
                | AppCommand::Resize
                | AppCommand::Shutdown
        ) {
            self.ui.request_repaint();
        }
    }

    /// Apply a menu action (Tabs / Bookmarks / History / Downloads overlay).
    fn menu_action(&mut self, action: &MenuAction) {
        match action {
            // Select toggles the menu; the ☰ button only ever opens it (it's hidden
            // behind the menu once shown).
            MenuAction::Open => {
                if self.ui.menu_visible() {
                    self.ui.menu_close();
                } else {
                    self.ui.menu_open();
                }
            }
            MenuAction::Close => self.ui.menu_close(),
            MenuAction::SetSection(section) => self.ui.menu_set_section(*section),
            MenuAction::OpenSelected => self.menu_open_selected(),
            MenuAction::RemoveSelected => self.delete_menu_selection(),
            MenuAction::Clear => self.ui.menu_clear(),
            MenuAction::OpenUrl(url) => self.open_url(url.clone()),
            MenuAction::RemoveAt(index) => self.ui.menu_remove_at(*index),
            MenuAction::OpenTab(index) => {
                self.browser.switch_to(*index);
                self.ui.menu_close();
            }
            MenuAction::CloseTab(index) => {
                self.browser.close_tab(*index);
                self.ui.menu_set_tab_count(self.browser.tab_count());
            }
            MenuAction::NewTab => self.new_tab(),
        }
    }

    /// Open a new tab at the home page and close the menu.
    fn new_tab(&mut self) {
        let home = self.config.browser.home_page.clone();
        self.browser.open_tab(&home);
        self.ui.menu_close();
    }

    /// Toggle the current page in saved bookmarks (the ★ button / Start).
    fn toggle_current_bookmark(&mut self) {
        let url = self.browser.get_state_mut().get_location().to_string();
        if !url.is_empty() {
            self.ui.toggle_bookmark(&url);
        }
    }

    /// Open the highlighted menu entry (the **A** button / Enter). In Tabs this
    /// switches to the tab (or opens a new one on the "+ New tab" row); in the URL
    /// lists it loads the entry. Closes the menu either way.
    fn menu_open_selected(&mut self) {
        if self.ui.menu_section() == Section::Tabs {
            let sel = self.ui.menu_tab_selected();
            if sel < self.browser.tab_count() {
                self.browser.switch_to(sel);
                self.ui.menu_close();
            } else {
                self.new_tab(); // the "+ New tab" row
            }
        } else if let Some(url) = self.ui.menu_selected_url() {
            self.open_url(url);
        } else if self.ui.menu_section() != Section::Downloads {
            // A on an active/failed download has nothing to open — keep the menu
            // up so the user can watch the progress; other sections close.
            self.ui.menu_close();
        }
    }

    /// Delete the highlighted menu entry (the **X** button / Delete). In Tabs this
    /// closes the tab; in the URL lists it removes the bookmark / history entry.
    fn delete_menu_selection(&mut self) {
        if self.ui.menu_section() == Section::Tabs {
            let sel = self.ui.menu_tab_selected();
            if sel < self.browser.tab_count() {
                self.browser.close_tab(sel);
                self.ui.menu_set_tab_count(self.browser.tab_count());
            }
        } else {
            self.ui.menu_remove_selected();
        }
    }

    /// Load `url` in the focused tab and close the menu.
    fn open_url(&mut self, url: String) {
        *self.browser.get_state_mut().get_location_mut() = url;
        self.browser
            .execute_command(&BrowserCommand::Load, &self.config.browser);
        self.ui.menu_close();
    }

    fn shutdown(&mut self) {
        self.state = AppState::ShuttingDown;
    }

    fn draw(&mut self) {
        self.ui.draw(&self.window);
    }
}
