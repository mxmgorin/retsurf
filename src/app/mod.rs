//! The application core: construction, the main loop, and command execution.
//! The command vocabulary is defined in [`command`]; the contextual-input
//! routing (gamepad / keyboard intents against the current UI state) lives in
//! [`router`].

mod command;
mod execute;
mod router;

pub use command::{AppCommand, InputCommand, MenuAction, PromptAction, SettingsAction};

use crate::browser::AppBrowser;
use crate::data::session::Session;
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
    /// The tabs to reopen at startup (`[browser] restore_tabs`).
    session: Session,
    /// Last time the deferred stores (history, tab session) were written. The
    /// flush only fires on frames the loop is already awake for, so it never
    /// wakes an idle loop.
    last_flush: Instant,
    /// Last time a memory report was requested (debug overlay only). Throttles
    /// the requests to [`MEMORY_REPORT_INTERVAL`] since each one walks every reporter.
    last_memory_report: Instant,
    /// When to hand the allocator's free memory back (see [`HEAP_TRIM_DELAY`]).
    heap_trim_at: Option<Instant>,
    /// Holds `SDL_INIT_AUDIO` open for the WebAudio backend ([`crate::media`]);
    /// dropping it closes the sinks' devices. `None` when audio is off/unavailable.
    _audio: Option<sdl2::AudioSubsystem>,
}

/// How often the main loop opportunistically flushes the deferred stores —
/// history and the tab session — to disk (only on frames it's already awake
/// for: navigation, paint, input). Coalesces the per-navigation writes that
/// used to rewrite `history.toml` on every page load.
const FLUSH_INTERVAL: Duration = Duration::from_secs(5);

/// How often the debug memory overlay (`[debug] memory_overlay`) refreshes its
/// figures by asking Servo for a new report.
const MEMORY_REPORT_INTERVAL: Duration = Duration::from_secs(1);

/// How long after a navigation the allocator is asked for its free memory back:
/// long enough that the document being replaced has finished going away.
const HEAP_TRIM_DELAY: Duration = Duration::from_secs(5);

impl App {
    pub fn new(sdl: &mut Sdl, config: AppConfig) -> Result<Self, String> {
        log::info!("init: creating window");
        let window = AppWindow::new(sdl, &config.display)?;
        // Before the browser: whichever media backend lands first is the one that sticks.
        let audio = crate::media::init(sdl, &config.audio, &config.video);
        log::info!("init: window ready; creating browser");
        let event_sender = UserEventSender::new();
        let browser = AppBrowser::new(window.rendering_ctx(), event_sender.clone(), &config)?;
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
            crate::browser::effective_user_agent(&config.browser),
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
            session: Session::load(),
            last_flush: Instant::now(),
            last_memory_report: Instant::now(),
            heap_trim_at: None,
            _audio: audio,
        })
    }

    pub fn run(mut self) {
        self.open_first_tabs();
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
                self.ui.menu.record_history(&url);
                self.heap_trim_at = Some(Instant::now() + HEAP_TRIM_DELAY);
            }

            // A closed document leaves its memory with the allocator rather than
            // the kernel; here that is 200 MB the device swaps around for nothing.
            if self.heap_trim_at.is_some_and(|at| at <= Instant::now()) {
                crate::platform::heap::trim();
                self.heap_trim_at = None;
            }

            // Recording only marks history dirty; flush it on a throttle so a busy
            // browsing burst collapses to one write per interval. This piggybacks
            // on frames the loop is already awake for — it never schedules an idle
            // wake (the blocking wait stays battery-efficient). A clean exit and
            // menu close flush the remainder.
            if self.last_flush.elapsed() >= FLUSH_INTERVAL {
                self.ui.menu.flush_history();
                self.save_session();
                self.last_flush = Instant::now();
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
            self.ui.menu.downloads.poll();
            for request in self.browser.take_download_requests() {
                self.ui.menu.downloads.start(request, &self.event_sender);
            }
            // Files a page built in JS and handed us whole: ask the signalling
            // pages for them, then save whatever earlier reads returned.
            self.browser.poll_blob_downloads();
            for item in self.browser.take_blob_downloads() {
                self.ui.menu.downloads.save_captured(item);
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
            if self.ui.hints.take_refresh_due() {
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

        // Persist what was buffered since the last throttle tick — `Drop` won't
        // run (we `process::exit` below), so this must be explicit.
        self.ui.menu.flush_history();
        self.save_session();
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

    /// Fill the empty tab list at startup: the saved session, or the home page
    /// when there is none. `restore_tabs` off drops the stored session instead.
    fn open_first_tabs(&mut self) {
        if !self.config.browser.restore_tabs {
            self.session.discard();
        } else if self
            .browser
            .restore_tabs(self.session.urls(), self.session.active())
        {
            return;
        }
        self.browser.open_tab(&self.config.browser.home_page);
    }

    /// Snapshot the open tabs for the next launch. A no-op with `restore_tabs`
    /// off, and a tab list unchanged since the last snapshot writes nothing.
    fn save_session(&mut self) {
        if self.config.browser.restore_tabs {
            self.session.record(&self.browser.tabs());
        }
    }

    fn draw(&mut self) {
        self.ui.draw(&self.window);
    }
}
