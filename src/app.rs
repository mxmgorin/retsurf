use crate::browser::{AppBrowser, BrowserCommand};
use crate::event::gamepad::Gamepad;
use crate::event::handler::AppEventHandler;
use crate::event::sdl2_servo::{into_mouse_button_event, into_mouse_move_event};
use crate::event::user::UserEventSender;
use crate::osk::OskCommand;
use crate::ui::AppUi;
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
    Bookmark(BookmarkAction),
}

/// Toolbar-driven bookmark actions (the in-overlay navigation is routed from
/// [`InputCommand`] instead, see [`App::route_input`]).
#[derive(Clone)]
pub enum BookmarkAction {
    /// Add the current page to bookmarks, or remove it if already saved (★).
    ToggleCurrent,
    /// Open the full-screen bookmarks overlay (☰).
    Open,
    /// Close the overlay (clicking its Close button).
    Close,
    /// Load a specific bookmark and close the overlay (clicking a list row).
    OpenUrl(String),
    /// Remove the bookmark at `index` (clicking its ✖ button).
    Remove(usize),
    /// Open the highlighted bookmark (keyboard Enter).
    OpenSelected,
    /// Remove the highlighted bookmark (keyboard Delete).
    RemoveSelected,
    /// Move the overlay selection by `dy` rows (keyboard arrows).
    Move(i32),
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
    /// A dedicated keyboard key (Y/L2/R2). Applied only while the keyboard is open.
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
        let browser =
            AppBrowser::new(window.get_rendering_ctx(), event_sender, &config.browser)?;
        log::info!("init: browser ready; creating event handler + ui");
        let event_handler = AppEventHandler::new(sdl)?;
        let ui = AppUi::new(&window, &config.interface);
        let gamepad = Gamepad::new(config.gamepad);
        log::info!("init: app constructed");

        Ok(Self {
            config,
            window,
            browser,
            event_handler,
            ui,
            gamepad,
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
            self.event_handler.wait(
                &self.window,
                &mut self.ui,
                &mut self.browser,
                &mut self.gamepad,
                &mut commands,
            );

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
            // Resizes are handled reactively: egui tracks the window size and
            // `AppUi::update` resizes the browser viewport to the central area.
            AppCommand::Resize(..) => {}
            AppCommand::Browser(command) => {
                self.browser.execute_command(command, &self.config.browser)
            }
            AppCommand::Input(command) => self.route_input(command, out),
            AppCommand::Bookmark(action) => self.bookmark_action(action),
        };
    }

    /// Toolbar bookmark buttons: toggle the current page, or open the overlay.
    fn bookmark_action(&mut self, action: &BookmarkAction) {
        match action {
            BookmarkAction::ToggleCurrent => {
                let url = self.browser.get_state_mut().get_location().to_string();
                if !url.is_empty() {
                    self.ui.bookmark_toggle(&url);
                }
            }
            // Select toggles the overlay (the ☰ button only ever opens it, since
            // it's hidden behind the overlay once shown).
            BookmarkAction::Open => {
                if self.ui.bookmarks_visible() {
                    self.ui.bookmarks_hide();
                } else {
                    self.ui.bookmarks_open();
                }
            }
            BookmarkAction::Close => self.ui.bookmarks_hide(),
            BookmarkAction::OpenUrl(url) => self.open_bookmark(url.clone()),
            BookmarkAction::Remove(index) => self.ui.bookmarks_remove_at(*index),
            BookmarkAction::OpenSelected => self.open_selected_bookmark(),
            BookmarkAction::RemoveSelected => self.ui.bookmarks_remove_selected(),
            BookmarkAction::Move(dy) => self.ui.bookmarks_move(*dy),
        }
    }

    /// Open the highlighted bookmark (the **A** button) and close the overlay.
    fn open_selected_bookmark(&mut self) {
        if let Some(url) = self.ui.bookmarks_selected_url() {
            self.open_bookmark(url);
        } else {
            self.ui.bookmarks_hide();
        }
    }

    /// Load `url` in the focused tab and close the overlay.
    fn open_bookmark(&mut self, url: String) {
        *self.browser.get_state_mut().get_location_mut() = url;
        self.browser
            .execute_command(&BrowserCommand::Load, &self.config.browser);
        self.ui.bookmarks_hide();
    }

    /// Central input router: decide what a contextual [`InputCommand`] does given
    /// the current state. This is where the "keyboard open? cursor over the page
    /// or toolbar?" branches live — the gamepad itself stays state-agnostic.
    fn route_input(&mut self, command: &InputCommand, out: &mut Vec<AppCommand>) {
        match command {
            InputCommand::Primary(pressed) => {
                if self.ui.bookmarks_visible() {
                    if *pressed {
                        self.open_selected_bookmark();
                    }
                } else {
                    self.primary_action(*pressed, out);
                }
            }
            InputCommand::Cancel => {
                if self.ui.bookmarks_visible() {
                    self.ui.bookmarks_hide();
                } else if self.ui.osk_visible() {
                    self.ui.osk(OskCommand::Hide, &self.browser, out);
                } else {
                    self.browser
                        .execute_command(&BrowserCommand::Back, &self.config.browser);
                }
            }
            InputCommand::Keyboard => {
                if self.ui.bookmarks_visible() {
                    // X deletes the highlighted bookmark.
                    self.ui.bookmarks_remove_selected();
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

        // The bookmarks overlay: the stick scrolls the highlighted row (vertical).
        if self.ui.bookmarks_visible() {
            let dir = osk_nav_dir(aim, cfg.osk_nav_threshold);
            if self.nav_repeat((0, dir.1), now, &cfg) {
                self.ui.bookmarks_move(dir.1);
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
