//! The application core: construction, the main loop, and command execution.
//! The command vocabulary is defined in [`command`]; the contextual-input
//! routing (gamepad / keyboard intents against the current UI state) lives in
//! [`router`].

mod command;
mod router;

pub use command::{AppCommand, InputCommand, MenuAction, PromptAction, SettingsAction};

use crate::browser::adblock::Adblock;
use crate::browser::{AppBrowser, BrowserCommand};
use crate::event::handler::AppEventHandler;
use crate::event::user::UserEventSender;
use crate::overlay::dial_edit::EditItem;
use crate::overlay::menu::Section;
use crate::overlay::osk::OskCommand;
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
}

/// How often the main loop opportunistically flushes deferred history (only on
/// frames it's already awake for — navigation, paint, input). Coalesces the
/// per-navigation writes that used to rewrite `history.toml` on every page load.
const HISTORY_FLUSH_INTERVAL: Duration = Duration::from_secs(5);

impl App {
    pub fn new(sdl: &mut Sdl, config: AppConfig) -> Result<Self, String> {
        log::info!("init: creating window");
        let window = AppWindow::new(sdl, &config.display)?;
        log::info!("init: window ready; creating browser");
        let event_sender = UserEventSender::new();
        let browser = AppBrowser::new(
            window.get_rendering_ctx(),
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
        })
    }

    pub fn run(mut self) {
        self.browser.open_tab(&self.config.browser.home_page);
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
            AppCommand::Prompt(action) => match action {
                PromptAction::Activate => self.ui.prompt.activate(),
                PromptAction::Cancel => self.ui.prompt.cancel(),
                PromptAction::ClickSlot(index) => {
                    self.ui.prompt.set_selected(*index);
                    self.ui.prompt.activate();
                }
            },
            AppCommand::Settings(action) => self.settings_action(action, out),
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
            MenuAction::ToggleBookmark(url) => self.ui.toggle_bookmark(url),
            MenuAction::DialEdit => self.ui.open_pins_editor(),
            MenuAction::DialClose => self.ui.close_pins_editor(),
            MenuAction::DialAdd(url) => self.dial_add(url),
            MenuAction::DialRemoveAt(index) => self.ui.dial_remove_at(*index),
            MenuAction::DialToggleSettings => self.ui.dial_toggle(crate::data::dial::SETTINGS_PIN),
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
            if sel == 0 {
                self.new_tab(); // the "+ New tab" button (index 0)
            } else {
                self.browser.switch_to(sel - 1);
                self.ui.menu_close();
            }
        } else if self.ui.menu_history_clear_selected() {
            // History's "Clear all" top row (index 0): wipe the list, stay open.
            self.ui.menu_clear();
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
            // Index 0 is the "+ New tab" button (nothing to delete); tabs are 1.. .
            let sel = self.ui.menu_tab_selected();
            if sel > 0 {
                self.browser.close_tab(sel - 1);
                self.ui.menu_set_tab_count(self.browser.tab_count());
            }
        } else {
            self.ui.menu_remove_selected();
        }
    }

    /// Y in the menu (link-hint toggle elsewhere): the action depends on the
    /// section. Bookmarks pins/unpins the selected entry on the speed dial;
    /// History bookmarks (or un-bookmarks) the selected entry; Tabs bookmarks
    /// the selected tab's URL. Downloads has no Y action.
    fn menu_y_action(&mut self) {
        match self.ui.menu_section() {
            Section::Bookmarks => {
                if let Some(url) = self.ui.menu_selected_url() {
                    self.ui.dial_toggle(&url);
                }
            }
            Section::History => {
                if let Some(url) = self.ui.menu_selected_url() {
                    self.ui.toggle_bookmark(&url);
                }
            }
            Section::Tabs => {
                // Index 0 is the "+ New tab" button; the tabs follow at 1..=N.
                let sel = self.ui.menu_tab_selected();
                if sel > 0 {
                    if let Some(info) = self.browser.tabs().get(sel - 1) {
                        if !info.url.is_empty() {
                            self.ui.toggle_bookmark(&info.url);
                        }
                    }
                }
            }
            Section::Downloads => {}
        }
    }

    /// Apply a settings-overlay action (see [`crate::overlay::settings`]).
    fn settings_action(&mut self, action: &SettingsAction, out: &mut Vec<AppCommand>) {
        match action {
            // Re-triggering the settings gesture while it's already open is the
            // two-step quit (open settings, press Select+Start again to confirm):
            // save the draft like a normal close, then shut down. A first press
            // just opens, seeding the draft from the live config.
            SettingsAction::Open => {
                if self.ui.settings_visible() {
                    self.settings_close();
                    self.shutdown();
                } else {
                    self.ui.settings_open(&self.config);
                }
            }
            SettingsAction::Close => self.settings_close(),
            SettingsAction::SetSection(section) => self.ui.settings_set_section(*section),
            SettingsAction::Select(index) => self.ui.settings_select(*index),
            SettingsAction::Activate => self.settings_confirm(out),
            SettingsAction::Adjust(dx) => self.ui.settings_adjust(*dx),
            // A link on the About tab: save & close like a normal exit, then load
            // it in the focused tab (open_url also tidies the menu, harmless here).
            SettingsAction::OpenLink(url) => {
                self.settings_close();
                self.open_url(url.clone());
            }
            // Binding capture (Controls section): the gesture the user performed
            // (gamepad gesture or key combo), bound to the listening action. The
            // raw input comes from the event loop / pad while capturing (see
            // [`crate::event::handler`] / [`crate::event::gamepad`]).
            SettingsAction::CaptureBinding { gesture, keyboard } => {
                self.ui.settings_apply_capture(gesture.clone(), *keyboard);
            }
            SettingsAction::CaptureCancel => self.ui.settings_cancel_capture(),
        }
    }

    /// A / Enter on the focused settings row: add/remove a binding in the Controls
    /// section, open the on-screen keyboard on a text field, or step every other
    /// kind forward (◀▶ does the rest).
    fn settings_confirm(&mut self, out: &mut Vec<AppCommand>) {
        if self.ui.settings_is_controls() {
            self.ui.settings_controls_activate();
        } else if self.ui.settings_selected_is_text() {
            self.ui.osk(OskCommand::Show, &self.browser, out);
        } else {
            self.ui.settings_adjust(1);
        }
    }

    /// Close the settings overlay (B / ✖): take its edited drafts and adopt them
    /// — the config and the gamepad bindings, each saved and re-applied live.
    fn settings_close(&mut self) {
        let (config, bindings) = self.ui.settings_close();
        self.apply_config(config);
        if let Some(store) = bindings {
            self.apply_bindings(store);
        }
    }

    /// Adopt edited gamepad bindings from the settings overlay: persist them, then
    /// rebuild the gesture table and swap it into the running gamepad handler (no
    /// restart). Only called when the controls changed, so keyboard bindings and
    /// any hand-written comments in `bindings.toml` survive a config-only edit.
    fn apply_bindings(&mut self, store: crate::event::bindings::Store) {
        use crate::event::bindings::{Bindings, KeyBindings};
        crate::event::bindings::save(&store);
        self.event_handler
            .set_bindings(Bindings::from_store(&store));
        self.event_handler
            .set_key_bindings(KeyBindings::from_store(&store));
    }

    /// Adopt an edited config from the settings overlay: persist it to disk, then
    /// re-apply the parts the running app can change without a restart. The rest
    /// (window size, GL backend, engine threads, ad-block lists, persisted site
    /// data) take effect on the next launch — those rows are flagged with `*`.
    fn apply_config(&mut self, config: AppConfig) {
        self.config = config;
        self.config.save();
        // The router reads cursor/scroll speeds from the config each frame, but
        // the gamepad state machine and the UI cache a few values to push in.
        self.event_handler
            .set_gamepad_config(self.config.input.clone());
        self.ui
            .set_cursor_linger(self.config.display.cursor_linger_ms);
        self.ui
            .set_toolbar_position(self.config.display.toolbar_position);
        self.ui
            .set_toolbar_autohide(self.config.display.toolbar_autohide);
        self.ui.set_hint_badges(self.config.input.hint_badges);
        // Lightweight-mode block flags take effect on the next subresource load,
        // no restart needed (unlike the engine-thread counts beside them).
        self.browser.set_content_filter(
            crate::browser::content_filter::ContentFilter::from_config(&self.config.data_saving),
        );
    }

    /// A on the start page: open the focused speed-dial tile, open the speed-dial
    /// editor on the "Edit" tile, or — when the search field is focused — open
    /// the OSK to type into it.
    fn home_confirm(&mut self, out: &mut Vec<AppCommand>) {
        if self.ui.home_tile_is_edit() {
            self.ui.open_pins_editor();
        } else if let Some(url) = self.ui.home_selected_url() {
            self.open_url(url);
        } else {
            self.ui.osk(OskCommand::Show, &self.browser, out);
        }
    }

    /// A in the speed-dial editor: open the OSK on the field, pin via the Add
    /// button, or nothing on a tile (tiles are edit-only here).
    fn dial_edit_confirm(&mut self, out: &mut Vec<AppCommand>) {
        match self.ui.dial_edit_item() {
            EditItem::Field => {
                self.ui.dial_edit_focus_field();
                self.ui.osk(OskCommand::Show, &self.browser, out);
            }
            // A on the trailing ⚙ tile toggles the settings shortcut on/off the
            // dial; the regular pin tiles are edit-only (delete with X).
            EditItem::Tile(_) => {
                if self.ui.dial_edit_settings_selected() {
                    self.ui.dial_toggle(crate::data::dial::SETTINGS_PIN);
                }
            }
        }
    }

    /// Pin the speed-dial editor's field text to the dial, normalized to a URL
    /// the same way navigation is, then clear the field (it stays open to add
    /// more).
    fn dial_add(&mut self, text: &str) {
        if let Some(url) =
            crate::browser::try_into_url(text.trim(), &self.config.browser.search_page)
        {
            self.ui.dial_pin(url.as_str());
        }
        self.ui.dial_edit_clear_input();
    }

    /// Load `url` in the focused tab and close the menu. The settings pin is a
    /// sentinel, not a real address: it opens the settings overlay instead of
    /// navigating (so a ⚙ speed-dial tile / menu row behaves like the toolbar's).
    fn open_url(&mut self, url: String) {
        if url == crate::data::dial::SETTINGS_PIN {
            self.ui.menu_close();
            self.ui.settings_open(&self.config);
            return;
        }
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
