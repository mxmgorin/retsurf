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
    /// Last time the report was written to the log (see [`MEMORY_LOG_INTERVAL`]).
    last_memory_log: Instant,
    /// Paint timing for `[debug] frame_timing`; inert unless that is on.
    frame_timer: FrameTimer,
    /// Per-thread cost for `[debug] thread_cpu`; inert unless that is on.
    thread_cpu: crate::platform::threads::ThreadCpu,
    /// When the last frame was presented, for [`App::pace_frame`].
    last_frame: Instant,
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

/// How often those figures also reach the log — the only way to read them on a
/// device whose screen is not where the answer is wanted.
const MEMORY_LOG_INTERVAL: Duration = Duration::from_secs(10);

/// How long after a navigation the allocator is asked for its free memory back:
/// long enough that the document being replaced has finished going away.
const HEAP_TRIM_DELAY: Duration = Duration::from_secs(5);

/// How long a pass that presented nothing waits, where the backend has no frame
/// cap of its own. Only the present blocks on a vsynced backend, so skipping it
/// leaves nothing to pace the loop; 60 Hz costs at most one frame of latency on
/// the input that does need a redraw.
const SKIPPED_PASS_INTERVAL: Duration = Duration::from_millis(16);

impl App {
    pub fn new(sdl: &mut Sdl, config: AppConfig) -> Result<Self, String> {
        log::info!("init: creating window");
        let window = AppWindow::new(sdl, &config.display, crate::ui::init_egui_ctx)?;
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

        // Read before `config` moves into the struct below.
        let frame_timer = FrameTimer::new(config.debug.frame_timing);
        let thread_cpu = crate::platform::threads::ThreadCpu::new(config.debug.thread_cpu);
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
            last_memory_log: Instant::now(),
            heap_trim_at: None,
            frame_timer,
            thread_cpu,
            last_frame: Instant::now(),
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
            self.ui.sync_window_size(&mut self.window);

            // Record any pages the focused webview navigated to this frame. Sourced
            // from real navigations (not address-bar text), so typing doesn't log.
            for url in self.browser.take_visited() {
                self.ui.menu.record_history(&url);
                self.schedule_heap_trim();
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
            if self.ui.memory_reports_wanted() {
                if self.last_memory_report.elapsed() >= MEMORY_REPORT_INTERVAL {
                    self.browser.request_memory_report();
                    self.last_memory_report = Instant::now();
                }
                if let Some(report) = self.browser.take_memory_report() {
                    self.ui.set_memory_summary(report);
                    // Slower than the overlay's refresh: the card should not
                    // be written to every second.
                    if self.last_memory_log.elapsed() >= MEMORY_LOG_INTERVAL {
                        self.ui.log_memory_summary(
                            self.window.egui_ctx(),
                            self.window.compose_bytes(),
                        );
                        self.last_memory_log = Instant::now();
                    }
                }
            }

            // Mirror whether the active tab is on the start page, so the UI's
            // focus precedence and the input router both see `Focus::Home` this
            // frame (set before input is handled in `wait`).
            let home_changed = self.ui.set_home_active(self.browser.on_home_page());

            self.event_handler.wait(
                &mut self.window,
                &mut self.ui,
                &mut self.browser,
                &mut commands,
            );

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

            // Render the page: into our FBO on GL, into swgl's CPU buffer
            // otherwise. The window composites it under the chrome either way.
            let at = self.frame_timer.mark();
            let page_painted = self.browser.paint();
            self.frame_timer.page_done(at);

            let at = self.frame_timer.mark();
            self.ui
                .update(&mut self.window, &mut self.browser, &mut commands);
            self.frame_timer.ui_done(at);

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

            let drew = self.draw(page_painted);
            self.frame_timer.tick();
            self.thread_cpu.tick();
            self.pace_frame(drew);
        }

        self.thread_cpu.report_run();

        // Persist what was buffered since the last throttle tick — `Drop` won't
        // run (we `process::exit` below), so this must be explicit.
        self.ui.menu.flush_history();
        self.save_session();
        self.save_window_size();
        self.window.destroy();

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

    /// Reopen at the size the window was left at. [`AppWindow::remembered_size`]
    /// offers only a size the user chose, so a handheld never rewrites its config.
    fn save_window_size(&mut self) {
        let Some((width, height)) = self.window.remembered_size() else {
            return;
        };
        let before = (self.config.display.width, self.config.display.height);
        (self.config.display.width, self.config.display.height) = (width, height);
        // Clamped like a hand-edited size, so an oversized window settles rather
        // than rewriting the file on every exit.
        self.config.sanitize();
        if (self.config.display.width, self.config.display.height) != before {
            self.config.save();
        }
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

    /// Ask the allocator for its free memory back once the document being torn
    /// down has finished going away (see [`HEAP_TRIM_DELAY`]).
    fn schedule_heap_trim(&mut self) {
        self.heap_trim_at = Some(Instant::now() + HEAP_TRIM_DELAY);
    }

    /// Snapshot the open tabs for the next launch. A no-op with `restore_tabs`
    /// off, and a tab list unchanged since the last snapshot writes nothing.
    fn save_session(&mut self) {
        if self.config.browser.restore_tabs {
            self.session.record(&self.browser.tabs());
        }
    }

    /// Put the frame on the panel — unless neither the page nor the chrome
    /// changed, which on a GPU-less device is most of them (see
    /// [`AppUi::take_frame_dirty`]). Reports whether anything was presented, which
    /// is what decides the pacing below.
    fn draw(&mut self, page_painted: bool) -> bool {
        if !page_painted && !self.ui.take_frame_dirty(&self.window) {
            return false;
        }
        let at = self.frame_timer.mark();
        let timing = self.ui.draw(&mut self.window, page_painted);
        self.frame_timer.chrome_done(at, timing);
        true
    }

    /// Hold the loop to a frame interval when presenting doesn't pace it. Two
    /// cases: the software renderer has no vsync to block on at all, and a pass
    /// that skipped the present never reached the vsync the GL path leans on —
    /// and `wait` deliberately does not block while a gamepad is connected, for
    /// exactly that reason. Measured free-running at 300-500 passes a second on
    /// an A55 handheld and 1600 on a desktop. Outside the frame timer on purpose,
    /// so the figures it logs stay the cost of the work.
    fn pace_frame(&mut self, drew: bool) {
        let interval = match self.window.frame_interval() {
            Some(interval) => Some(interval),
            None => (!drew).then_some(SKIPPED_PASS_INTERVAL),
        };
        if let Some(interval) = interval {
            let since = self.last_frame.elapsed();
            if since < interval {
                std::thread::sleep(interval - since);
            }
        }
        self.last_frame = Instant::now();
    }
}

/// How often [`FrameTimer`] logs its averages.
const FRAME_REPORT_INTERVAL: Duration = Duration::from_secs(1);

/// Rolling paint timing for `[debug] frame_timing`, split into the two halves
/// that cost differently on a GPU-less device: rasterizing the page (WebRender,
/// swgl in software mode) and drawing the chrome over it and presenting.
struct FrameTimer {
    enabled: bool,
    frames: u32,
    /// Loop passes that built the UI, which is more than `frames`: a pass whose
    /// picture came out identical is not drawn (see [`AppUi::take_frame_dirty`]).
    passes: u32,
    page: Duration,
    /// Building egui's shapes — the layout pass, before anything is rasterized.
    ui: Duration,
    chrome: Duration,
    /// The software backend's own split of `chrome`; `None` on GL, which leaves
    /// the work to the driver and has nothing to report.
    composite: Option<crate::platform::window::CompositeTiming>,
    since: Instant,
}

impl FrameTimer {
    fn new(enabled: bool) -> Self {
        Self {
            enabled,
            frames: 0,
            passes: 0,
            page: Duration::ZERO,
            ui: Duration::ZERO,
            chrome: Duration::ZERO,
            composite: None,
            since: Instant::now(),
        }
    }

    /// The start of a timed half, or `None` when timing is off — which is what
    /// keeps a disabled timer down to one branch per frame.
    #[inline]
    fn mark(&self) -> Option<Instant> {
        self.enabled.then(Instant::now)
    }

    #[inline]
    fn page_done(&mut self, at: Option<Instant>) {
        if let Some(at) = at {
            self.page += at.elapsed();
        }
    }

    #[inline]
    fn ui_done(&mut self, at: Option<Instant>) {
        if let Some(at) = at {
            self.ui += at.elapsed();
            self.passes += 1;
        }
    }

    #[inline]
    fn chrome_done(
        &mut self,
        at: Option<Instant>,
        composite: Option<crate::platform::window::CompositeTiming>,
    ) {
        let Some(at) = at else { return };
        self.chrome += at.elapsed();
        if let Some(split) = composite {
            self.composite
                .get_or_insert_with(Default::default)
                .add(split);
        }
        self.frames += 1;
    }

    /// Report the interval just ended. Driven once per loop pass rather than per
    /// frame, so a browser that has stopped drawing still reports the frames it
    /// drew — seeing that number reach zero is the whole point of it.
    fn tick(&mut self) {
        if !self.enabled || self.since.elapsed() < FRAME_REPORT_INTERVAL {
            return;
        }
        if self.frames == 0 {
            // Idle is the expected state and does not need a line a second
            // written to an SD card to say so.
            self.since = Instant::now();
            return;
        }
        let per = |total: Duration| total.as_secs_f32() * 1000.0 / self.frames as f32;
        // The UI pass runs whether or not the frame is drawn, so it averages over
        // the passes rather than the frames.
        let per_pass = |total: Duration| total.as_secs_f32() * 1000.0 / self.passes.max(1) as f32;
        let split = match self.composite {
            Some(c) => format!(
                " [page {:.1} tess {:.1} tex {:.1} raster {:.1} upload {:.1} present {:.1}]",
                per(c.page),
                per(c.tessellate),
                per(c.textures),
                per(c.chrome),
                per(c.upload),
                per(c.present)
            ),
            None => String::new(),
        };
        log::info!(
            "frame timing: page {:.1} ms, ui {:.1} ms, chrome+present {:.1} ms ({} frames, {} passes){}",
            per(self.page),
            per_pass(self.ui),
            per(self.chrome),
            self.frames,
            self.passes,
            split
        );
        *self = Self::new(self.enabled);
    }
}
