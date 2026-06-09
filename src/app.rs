use crate::adblock::Adblock;
use crate::browser::{AppBrowser, BrowserCommand};
use crate::event::gamepad::Gamepad;
use crate::event::handler::AppEventHandler;
use crate::event::sdl2_servo::{into_mouse_button_event, into_mouse_move_event};
use crate::event::user::UserEventSender;
use crate::osk::OskCommand;
use crate::ui::AppUi;
use crate::menu::Section;
use crate::{
    config::{AppConfig, GamepadConfig},
    window::AppWindow,
};
use sdl2::Sdl;
use std::time::{Duration, Instant};

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
    Input(InputCommand),
    Menu(MenuAction),
    /// Add the current page to bookmarks, or remove it if already saved (★ / Start).
    ToggleBookmark,
}

/// Actions on the full-screen menu (Tabs / Bookmarks / History). The mouse pushes
/// the absolute variants (`SetSection`, `OpenUrl`, `RemoveAt`); the gamepad and
/// keyboard push the relative ones, routed from [`InputCommand`] via
/// [`App::route_input`].
#[derive(Clone)]
pub enum MenuAction {
    /// Toggle the menu open/closed (Select / ☰).
    Open,
    /// Close the menu (B / Close button / Esc).
    Close,
    /// Switch the active section by a delta (gamepad/keyboard ◀▶).
    SwitchSection(i32),
    /// Jump to a specific section (clicking its tab).
    SetSection(Section),
    /// Move the active section's selection by `dy` rows (gamepad/keyboard ▲▼).
    Move(i32),
    /// Open the highlighted entry and close the menu (A / Enter).
    OpenSelected,
    /// Remove the highlighted entry (X / Delete).
    RemoveSelected,
    /// Clear all entries in the active section (History's "Clear all").
    Clear,
    /// Load a specific URL and close the menu (clicking a list row).
    OpenUrl(String),
    /// Remove the entry at `index` in the active section (clicking its ✖).
    RemoveAt(usize),
    /// Switch to the tab at `index` and close the menu (clicking a tab row).
    OpenTab(usize),
    /// Close the tab at `index` (clicking a tab's ✖).
    CloseTab(usize),
    /// Open a new tab and close the menu (clicking "+ New tab").
    NewTab,
}

/// A *contextual* input intent from a control device — one whose effect depends
/// on what's on screen. The gamepad only translates physical buttons/sticks into
/// these (unambiguous navigation goes straight to [`BrowserCommand`]); the central
/// router ([`App::route_input`]) decides what each does given the current state
/// (keyboard open? cursor over the page or the toolbar?).
#[derive(Clone)]
pub enum InputCommand {
    /// Primary action (A): activate the keyboard key, or click the page/toolbar.
    /// Carries the press state so page clicks get matching down/up events.
    Primary(bool),
    /// Cancel (B): close the on-screen keyboard if open, else go back.
    Cancel,
    /// Keyboard (X): toggle the on-screen keyboard, or backspace while it's open.
    Keyboard,
    /// Shoulder (L1/R1) by direction (-1 left, +1 right): switch the menu's section
    /// while it's open, otherwise navigate the page back / forward.
    Shoulder(i32),
    /// Trigger (L2 = left, R2 = right) with its press state. Drives the on-screen
    /// keyboard (L2 Shift, R2 Enter) when it's open, otherwise cycles tabs.
    Trigger { right: bool, pressed: bool },
    /// A dedicated keyboard key (Y). Applied only while the keyboard is open.
    Osk(OskCommand),
    /// Per-frame analog state: aim vector (left stick + D-pad) and scroll (right
    /// stick Y), each normalized to -1..=1. Drives the cursor, keyboard grid
    /// navigation, or page scroll depending on context.
    Analog { aim: (f32, f32), scroll: f32 },
}

pub struct App {
    event_handler: AppEventHandler,
    config: AppConfig,
    window: AppWindow,
    state: AppState,
    browser: AppBrowser,
    ui: AppUi,
    gamepad: Gamepad,
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
        let event_handler = AppEventHandler::new(sdl)?;
        let ui = AppUi::new(&window, &config.interface, &config.history, &config.downloads);
        let gamepad = Gamepad::new(config.gamepad);
        log::info!("init: app constructed");

        Ok(Self {
            config,
            window,
            browser,
            event_handler,
            ui,
            gamepad,
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

            self.event_handler.wait(
                &self.window,
                &mut self.ui,
                &mut self.browser,
                &mut self.gamepad,
                &mut commands,
            );

            // Apply background download progress/finishes before building the UI,
            // and start any downloads the browser denied navigation for.
            self.ui.downloads_poll();
            for url in self.browser.take_download_requests() {
                self.ui.start_download(&url, &self.event_sender);
            }

            // Emit this frame's analog state as a command for the router to apply.
            self.gamepad.tick(&mut commands);

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
            AppCommand::Resize(..) => self.ui.resize_browser(&self.window, &self.browser),
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
                | AppCommand::Resize(..)
                | AppCommand::Shutdown
        ) {
            self.ui.request_repaint();
        }
    }

    /// Apply a menu action (Tabs / Bookmarks / History overlay).
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
            MenuAction::SwitchSection(delta) => self.ui.menu_switch(*delta),
            MenuAction::SetSection(section) => self.ui.menu_set_section(*section),
            MenuAction::Move(dy) => self.ui.menu_move(*dy),
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

    /// Central input router: decide what a contextual [`InputCommand`] does given
    /// the current state. This is where the "keyboard open? cursor over the page
    /// or toolbar?" branches live — the gamepad itself stays state-agnostic.
    fn route_input(&mut self, command: &InputCommand, out: &mut Vec<AppCommand>) {
        match command {
            InputCommand::Primary(pressed) => {
                if self.ui.menu_visible() {
                    if *pressed {
                        self.menu_open_selected();
                    }
                } else {
                    self.primary_action(*pressed, out);
                }
            }
            InputCommand::Cancel => {
                if self.ui.menu_visible() {
                    self.ui.menu_close();
                } else if self.ui.osk_visible() {
                    self.ui.osk(OskCommand::Hide, &self.browser, out);
                } else {
                    self.browser
                        .execute_command(&BrowserCommand::Back, &self.config.browser);
                }
            }
            InputCommand::Keyboard => {
                if self.ui.menu_visible() {
                    // X deletes the highlighted entry (closes a tab in the Tabs section).
                    self.delete_menu_selection();
                } else {
                    let cmd = if self.ui.osk_visible() {
                        OskCommand::Backspace
                    } else {
                        OskCommand::Show
                    };
                    self.ui.osk(cmd, &self.browser, out);
                }
            }
            // Dedicated keyboard keys act only while the keyboard is open. The one
            // exception is Y (Space): outside the keyboard it reloads the page.
            InputCommand::Shoulder(delta) => {
                if self.ui.menu_visible() {
                    self.ui.menu_switch(*delta);
                } else {
                    let cmd = if *delta < 0 {
                        BrowserCommand::Back
                    } else {
                        BrowserCommand::Foward
                    };
                    self.browser.execute_command(&cmd, &self.config.browser);
                }
            }
            InputCommand::Trigger { right, pressed } => {
                if self.ui.osk_visible() {
                    // Keyboard: L2 is a held Shift, R2 is Enter on the press edge.
                    if *right {
                        if *pressed {
                            self.ui.osk(OskCommand::Enter, &self.browser, out);
                        }
                    } else {
                        self.ui.osk(OskCommand::Shift(*pressed), &self.browser, out);
                    }
                } else if *pressed {
                    // Quick tab switch: L2 previous, R2 next (wraps).
                    self.browser.cycle_tab(if *right { 1 } else { -1 });
                }
            }
            InputCommand::Osk(cmd) => {
                if self.ui.osk_visible() {
                    self.ui.osk(*cmd, &self.browser, out);
                } else if matches!(cmd, OskCommand::Space) {
                    self.browser
                        .execute_command(&BrowserCommand::Reload, &self.config.browser);
                }
            }
            InputCommand::Analog { aim, scroll } => self.route_analog(*aim, *scroll, out),
        }
    }

    /// The A button: activate the selected keyboard key, click the page in Servo,
    /// or click the egui toolbar — whichever the cursor is currently over.
    fn primary_action(&mut self, pressed: bool, out: &mut Vec<AppCommand>) {
        if self.ui.osk_visible() {
            if pressed {
                self.ui.osk(OskCommand::Activate, &self.browser, out);
            }
        } else if self.ui.cursor_over_browser() {
            let (x, y) = self.ui.cursor_browser_rel();
            self.browser
                .handle_input(servo::InputEvent::MouseMove(into_mouse_move_event(x, y)));
            let event = into_mouse_button_event(sdl2::mouse::MouseButton::Left, x, y, pressed);
            self.browser
                .handle_input(servo::InputEvent::MouseButton(event));
        } else {
            self.ui.click_ui(pressed, &self.window);
        }
    }

    /// Apply per-frame analog state: keyboard grid navigation (with auto-repeat)
    /// while the keyboard is open, otherwise cursor movement and page scroll.
    fn route_analog(&mut self, aim: (f32, f32), scroll: f32, out: &mut Vec<AppCommand>) {
        let now = Instant::now();
        let dt = (now - self.last_tick).as_secs_f32();
        self.last_tick = now;
        // The loop blocks on input while idle, so the first frame after a press
        // sees the whole idle gap as `dt`. Integrating that teleports the cursor
        // (a D-pad tap jumps ~`cursor_speed * dt`), so treat any over-long frame
        // as a fresh start: no motion this frame, normal motion from the next.
        let dt = if dt > 0.1 { 0.0 } else { dt };
        let cfg = self.config.gamepad;

        // The menu: left/right switches section, up/down moves the selection
        // (dominant axis only, so a diagonal nudge does just one thing).
        if self.ui.menu_visible() {
            let dir = osk_nav_dir(aim, cfg.osk_nav_threshold);
            if self.nav_repeat(dir, now, &cfg) {
                if dir.0 != 0 {
                    self.ui.menu_switch(dir.0);
                } else if dir.1 != 0 {
                    self.ui.menu_move(dir.1);
                }
            }
            return;
        }

        // The keyboard: the stick navigates the key grid.
        if self.ui.osk_visible() {
            let dir = osk_nav_dir(aim, cfg.osk_nav_threshold);
            if self.nav_repeat(dir, now, &cfg) {
                self.ui.osk(OskCommand::Move(dir.0, dir.1), &self.browser, out);
            }
            return;
        }

        if aim != (0.0, 0.0) {
            self.ui.move_cursor(
                aim.0 * cfg.cursor_speed * dt,
                aim.1 * cfg.cursor_speed * dt,
                &self.window,
            );
            // Only hover the page while the cursor is over it; over the toolbar
            // there's nothing in Servo to point at.
            if self.ui.cursor_over_browser() {
                let (x, y) = self.ui.cursor_browser_rel();
                self.browser
                    .handle_input(servo::InputEvent::MouseMove(into_mouse_move_event(x, y)));
            }
        }

        if scroll != 0.0 && self.ui.cursor_over_browser() {
            // Stick down (+1) reveals lower content (positive Servo dy).
            let dy = scroll * cfg.scroll_speed * dt;
            let (x, y) = self.ui.cursor_browser_rel();
            self.browser.scroll(0.0, dy, x, y);
        }
    }

    /// Auto-repeat gate for held-stick overlay navigation: latches the direction
    /// and paces repeats, returning `true` on the frames a step should fire.
    fn nav_repeat(&mut self, dir: (i32, i32), now: Instant, cfg: &GamepadConfig) -> bool {
        if dir != self.osk_nav_dir {
            self.osk_nav_dir = dir;
            if dir != (0, 0) {
                self.osk_nav_next = now + Duration::from_millis(cfg.osk_nav_initial_delay_ms);
                return true;
            }
            return false;
        }
        if dir != (0, 0) && now >= self.osk_nav_next {
            self.osk_nav_next = now + Duration::from_millis(cfg.osk_nav_repeat_ms);
            return true;
        }
        false
    }

    fn shutdown(&mut self) {
        self.state = AppState::ShuttingDown;
    }

    fn draw(&mut self) {
        self.ui.draw(&self.window);
    }
}

/// Reduce a stick vector to a single discrete grid step along its dominant axis,
/// or `(0, 0)` when the stick is within the navigation dead zone (`threshold`).
fn osk_nav_dir(v: (f32, f32), threshold: f32) -> (i32, i32) {
    if v.0.abs().max(v.1.abs()) < threshold {
        (0, 0)
    } else if v.0.abs() >= v.1.abs() {
        (v.0.signum() as i32, 0)
    } else {
        (0, v.1.signum() as i32)
    }
}
