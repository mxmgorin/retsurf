//! The application core: construction, the main loop, and command execution.
//! The command vocabulary is defined in [`command`]; the contextual-input
//! routing (gamepad / keyboard intents against the current UI state) lives in
//! [`router`].

mod command;
mod execute;
mod router;

pub use command::{AppCommand, InputCommand, MenuAction, PromptAction, SettingsAction};

use crate::browser::adblock::Adblock;
use crate::browser::AppBrowser;
use crate::event::handler::AppEventHandler;
use crate::event::user::UserEventSender;
use crate::ui::AppUi;
use crate::{config::AppConfig, platform::window::AppWindow};
use sdl2::Sdl;
use std::time::{Duration, Instant};

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
    /// When A/Enter went down on a hint, for the tap-vs-hold split (tap clicks
    /// the hint, hold opens its link in a background tab). `None` when no press
    /// is in flight over a hint.
    hint_press_at: Option<Instant>,
    /// Last time deferred history was flushed to disk. Bounds buffered-history
    /// loss to [`HISTORY_FLUSH_INTERVAL`] while browsing, without ever waking the
    /// idle loop — the flush only fires on frames the loop is already running.
    last_history_flush: Instant,
    /// Last time a memory report was requested (debug overlay only). Throttles
    /// the requests to [`MEMORY_REPORT_INTERVAL`] since each one walks every reporter.
    last_memory_report: Instant,
}

/// How often the main loop opportunistically flushes deferred history (only on
/// frames it's already awake for — navigation, paint, input). Coalesces the
/// per-navigation writes that used to rewrite `history.toml` on every page load.
const HISTORY_FLUSH_INTERVAL: Duration = Duration::from_secs(5);

/// How often the debug memory overlay (`[debug] memory_overlay`) refreshes its
/// figures by asking Servo for a new report.
const MEMORY_REPORT_INTERVAL: Duration = Duration::from_secs(1);

impl App {
    pub fn new(sdl: &mut Sdl, config: AppConfig) -> Result<Self, String> {
        log::info!("init: creating window");
        let window = AppWindow::new(sdl, &config.display)?;
        log::info!("init: window ready; creating browser");
        let event_sender = UserEventSender::new();
        let browser = AppBrowser::new(
            window.rendering_ctx(),
            event_sender.clone(),
            &config.browser,
            &config.performance,
            &config.data_saving,
            config.downloads.extensions.clone(),
            Adblock::new(&config.adblock),
        )?;
        log::info!("init: browser ready; creating event handler + ui");
        let event_handler = AppEventHandler::new(sdl, config.input.clone())?;
        let ui = AppUi::new(
            &window,
            &config.display,
            &config.history,
            &config.downloads,
            &config.osk,
            &config.input,
            &config.debug,
            &config.update,
        );
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
            hint_press_at: None,
            last_history_flush: Instant::now(),
            last_memory_report: Instant::now(),
        })
    }

    pub fn run(mut self) {
        self.browser.open_tab(&self.config.browser.home_page);
        // Throttled background check for a newer build (`[update] auto_check`); its
        // result surfaces via the toolbar update chip, never a blocking prompt.
        self.ui.update_auto_check(&self.event_sender);
        self.state = AppState::Running;
        let mut commands = Vec::with_capacity(4);

        while self.state == AppState::Running {
            self.browser.pump_event_loop();

            // Android can resize the surface on rotation without delivering an
            // SDL size-changed event, leaving egui laid out for the previous
            // orientation. Refresh egui's cached size from the live window each
            // frame so the layout follows the actual surface.
            #[cfg(target_os = "android")]
            self.ui.sync_window_size(&self.window);

            // Record any pages the focused webview navigated to this frame. Sourced
            // from real navigations (not address-bar text), so typing doesn't log.
            for url in self.browser.take_visited() {
                self.ui.menu_record_history(&url);
            }

            // Recording only marks history dirty; flush it on a throttle so a busy
            // browsing burst collapses to one write per interval. This piggybacks
            // on frames the loop is already awake for — it never schedules an idle
            // wake (the blocking wait stays battery-efficient). A clean exit and
            // menu close flush the remainder.
            if self.last_history_flush.elapsed() >= HISTORY_FLUSH_INTERVAL {
                self.ui.flush_history();
                self.last_history_flush = Instant::now();
            }

            // Debug memory overlay: on a throttle, ask Servo for a fresh report,
            // and adopt the latest one that has arrived (it comes back async, a
            // frame or two later). Both no-ops unless the overlay is enabled.
            if self.ui.memory_overlay_enabled() {
                if self.last_memory_report.elapsed() >= MEMORY_REPORT_INTERVAL {
                    self.browser.request_memory_report();
                    self.last_memory_report = Instant::now();
                }
                if let Some(report) = self.browser.take_memory_report() {
                    self.ui.set_memory_summary(report);
                }
            }

            // Mirror whether the active tab is on the start page, so the UI's
            // focus precedence and the input router both see `Focus::Home` this
            // frame (set before input is handled in `wait`).
            let home_changed = self.ui.set_home_active(self.browser.on_home_page());

            self.event_handler
                .wait(&self.window, &mut self.ui, &mut self.browser, &mut commands);

            // Apply background download progress/finishes before building the UI,
            // and start any downloads the browser denied navigation for.
            self.ui.downloads_poll();
            for url in self.browser.take_download_requests() {
                self.ui.start_download(&url, &self.event_sender);
            }

            // Modal page controls (select pickers, JS dialogs): queue fresh
            // ones for the prompt overlay and drop ones Servo retracted.
            let controls = self.browser.take_embedder_controls();
            let dismissed = self.browser.take_dismissed_controls();
            let prompt_changed = !controls.is_empty() || !dismissed.is_empty();
            for control in controls {
                self.ui.prompt.push(control);
            }
            for id in dismissed {
                self.ui.prompt.dismiss(id);
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

            // Android: raise/hide the system soft keyboard to match focus. The
            // address bar (egui) and page text fields (Servo) are the two sinks;
            // egui-sdl2 delivers the resulting SDL_TEXTINPUT to the focused field.
            // Desktop leaves SDL's always-on text input alone and uses the OSK.
            #[cfg(target_os = "android")]
            {
                let want = self.ui.wants_keyboard() || self.browser.text_input_focused();
                crate::platform::window::set_text_input(want);
            }

            // A prompt change needs a follow-up frame like commands below do
            // (egui sizes a fresh overlay invisibly on its first pass, and
            // `update` just rebuilt the idle wait) — request it after `update`
            // so it isn't clobbered.
            if prompt_changed || home_changed {
                self.ui.request_repaint();
            }

            // Drain in waves: routing a command (e.g. an OSK Enter) may queue more.
            while !commands.is_empty() {
                for command in std::mem::take(&mut commands) {
                    self.execute_command(&command, &mut commands);
                }
            }

            self.draw();
        }

        // Persist history buffered since the last throttle tick — `Drop` won't
        // run (we `process::exit` below), so this must be explicit.
        self.ui.flush_history();
        self.ui.destroy();

        // Shut Servo down cleanly first — that's when cookies / localStorage
        // are written to disk, so logins survive (see `AppBrowser::shutdown`).
        self.browser.shutdown();

        // Servo's SoftwareRenderingContext does not destroy its surfman context on
        // drop, which trips surfman's "destroy explicitly" guard and panics during
        // unwinding. Exit before running destructors; the OS reclaims everything.
        std::process::exit(0);
    }

    fn shutdown(&mut self) {
        self.state = AppState::ShuttingDown;
    }

    fn draw(&mut self) {
        self.ui.draw(&self.window);
    }
}
