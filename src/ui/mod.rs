//! The egui layer: [`AppUi`] holds the gamepad cursor and the overlay state and
//! lays the chrome out over the page; the widgets live in the submodules. egui
//! itself belongs to [`crate::platform::window::AppWindow`].

mod chrome;
mod cursor;
mod dial_edit;
mod game;
mod game_mode;
mod hints;
mod home;
mod memory;
mod menu;
mod osk;
mod overlays;
mod panel;
mod prompt;
mod scale;
mod settings;
mod theme;
mod toolbar;

pub use self::game_mode::game_mode_toast_text;
pub use self::overlays::Focus;

use crate::{
    browser::AppBrowser,
    command::AppCommand,
    config::{
        DebugConfig, DisplayConfig, DownloadsConfig, HistoryConfig, InputConfig, OskConfig,
        PadLayout, ToolbarPosition, UpdateConfig,
    },
    overlay::dial_edit::DialEdit,
    overlay::game::input_maps::InputMaps,
    overlay::game::map_edit::MapEdit,
    overlay::game::menu::GameMenu,
    overlay::hints::Hints,
    overlay::home::Home,
    overlay::menu::Menu,
    overlay::osk::Osk,
    overlay::prompt::Prompt,
    overlay::settings::Settings,
    platform::window::AppWindow,
    update::{UpdateState, Updater},
};
use egui_sdl2::egui;
use std::time::{Duration, Instant};

/// Style a freshly built [`egui::Context`]: the accent theme plus a launcher's
/// pinned zoom. The fit to the panel needs a frame's measurements and arrives
/// with `sync_scale` (see [`scale`]).
pub fn init_egui_ctx(ctx: &egui::Context) {
    theme::apply(ctx);
    if let Some(scale) = crate::config::device_scale() {
        ctx.set_zoom_factor(scale);
    }
}

/// The field the OSK types into, so its renderer can park egui's caret — egui
/// keys the caret by widget id and won't follow an external edit. `None` where
/// there is no egui caret (the page, a settings row).
#[derive(PartialEq, Eq, Clone, Copy)]
pub(super) enum OskField {
    None,
    AddressBar,
    DialEdit,
    Prompt,
    Home,
}

/// The egui ids of the chrome's text fields — spelled once, because the
/// widget that creates one and the focus query that asks about it must agree.
pub(super) mod ids {
    pub const LOCATION: &str = "location";
    pub const HOME_SEARCH: &str = "home_search";
    pub const DIAL_EDIT_URL: &str = "dial_edit_url";
}

/// Drop egui's keyboard focus, so Enter reaches only the overlay's selected row.
fn drop_egui_focus(ctx: &egui::Context) {
    ctx.memory_mut(|m| {
        if let Some(id) = m.focused() {
            m.surrender_focus(id);
        }
    });
}

/// Park egui's caret at char index `pos` (clamped to `char_count`) in an
/// externally-edited single-line `TextEdit`, so it tracks the OSK's caret (see
/// [`OskField`]). Call *before* the field renders so the caret lands this frame.
pub(super) fn park_caret(ctx: &egui::Context, id: egui::Id, pos: usize, char_count: usize) {
    let mut state = egui::TextEdit::load_state(ctx, id).unwrap_or_default();
    let at = egui::text::CCursor::new(pos.min(char_count));
    state
        .cursor
        .set_char_range(Some(egui::text::CCursorRange::one(at)));
    egui::TextEdit::store_state(ctx, id, state);
}

/// Select a `TextEdit`'s whole text, the way a desktop browser's address bar does
/// on focus — otherwise typing appends to the URL already there.
pub(super) fn select_all(ctx: &egui::Context, id: egui::Id, char_count: usize) {
    let mut state = egui::TextEdit::load_state(ctx, id).unwrap_or_default();
    let range = egui::text::CCursorRange::two(
        egui::text::CCursor::new(0),
        egui::text::CCursor::new(char_count),
    );
    state.cursor.set_char_range(Some(range));
    egui::TextEdit::store_state(ctx, id, state);
}

/// Toolbar layout decided before the egui closure ([`AppUi::toolbar_layout`]):
/// these reads borrow all of `self`, which can't overlap `egui.run`.
struct ToolbarLayout {
    position: ToolbarPosition,
    /// Whether the bar is shown this frame (auto-hide forces it while typing).
    shown: bool,
    /// Bottom auto-hide bar: floats as an overlay instead of reserving a strip.
    overlay: bool,
}

/// Per-frame inputs snapshotted from the browser before the render closure (see
/// [`AppUi::frame_snapshot`]), as owned copies — so the browser borrow doesn't
/// leak into `egui.run` (which borrows `self`).
struct FrameInputs {
    tab_count: usize,
    /// Page-zoom chip percentage (`None` at the default zoom).
    zoom_pct: Option<u16>,
    /// Tab snapshots for the menu's Tabs section (empty unless the menu is open).
    tab_infos: Vec<crate::data::session::TabInfo>,
    osk_field: OskField,
    /// Where the OSK's caret sits, mirrored into each `TextEdit`.
    osk_caret: usize,
    chrome_hidden: ChromeHidden,
}

/// The reasons the chrome hides, kept apart so leaving one does not reveal the
/// bar while the other still holds it.
#[derive(Clone, Copy, Default)]
struct ChromeHidden {
    page_fullscreen: bool,
    game_mode: bool,
}

impl ChromeHidden {
    fn any(self) -> bool {
        self.page_fullscreen || self.game_mode
    }
}

pub struct AppUi {
    /// Cached so focus/scale queries don't each need the window; refreshed every
    /// frame, because a software resize builds a fresh context.
    egui_ctx: egui::Context,
    repaint_delay: Option<Duration>,
    /// The web view's rect (logical px), measured from the central panel each
    /// frame. Maps cursor/browser coordinates and anchors the overlays.
    webview_rect: egui::Rect,
    /// Toolbar thickness (logical px), measured each frame. Stays valid across
    /// window-size changes, so the SDL-resize fast path sizes the viewport with it.
    toolbar_height: f32,
    /// The toolbar's on-screen rect (logical px), `NOTHING` while hidden; the
    /// hit-tests tell chrome from page with it.
    toolbar_rect: egui::Rect,
    /// The on-screen keyboard's drawn height (logical px), measured while it is up.
    osk_height: f32,
    /// The keyboard was opened over a page field: scroll the page so the field
    /// clears the keys, once [`Self::osk_height`] is known (see [`AppUi::update`]).
    osk_lift_pending: bool,
    repaint_pending: bool,
    /// Loop passes still to be drawn whatever else says, for the changes egui
    /// cannot report: the first frame, a resize, an explicit repaint.
    forced_passes: u8,
    /// egui handle to Servo's FBO color texture (rendered directly by WebRender).
    /// `None` in software mode, where the window composites the page itself.
    browser_tex_id: Option<egui::TextureId>,
    /// Last browser viewport size (physical px) we requested, to avoid churn.
    browser_viewport: (u32, u32),
    /// Game Mode: the browser stops consuming input and the chrome hides.
    /// [`crate::event::keyboard`] reads it to forward a key instead of binding it.
    game_mode: bool,
    /// When Game Mode was entered; the chrome hides with nothing else on screen,
    /// so a toast names the way out for [`GAME_MODE_TOAST`], then fades.
    game_mode_toast: Option<Instant>,
    /// Worded at entry from the ways out this device has.
    game_mode_toast_text: String,
    /// Game Mode's own menu (the `game_mode` gesture).
    pub game_menu: GameMenu,
    /// Its map list and one map's rows, opened from that menu.
    pub input_maps: InputMaps,
    /// Its map editor, opened from a map.
    pub map_edit: MapEdit,
    /// The live map's name, for the menu's row. Empty until that menu opens:
    /// naming it earlier would load every map for a row nobody has asked for.
    input_map_name: String,
    /// Gamepad cursor position (logical px). The UI owns it — it draws the
    /// overlay — and the gamepad moves it via `move_cursor` (see [`cursor`]).
    cursor: (f32, f32),
    /// When the cursor last moved, or `None` if it has never moved. Drives the
    /// auto-hide: the overlay shows only within `cursor_linger` of this.
    cursor_last_move: Option<Instant>,
    /// How long the cursor stays visible after a move (from the interface config).
    cursor_linger: Duration,
    /// `[display] scale`: the user's factor over the fit to the panel.
    ui_scale: f32,
    /// `RETSURF_SCALE`, standing in for the panel's own fit where a launcher
    /// knows better (Android, which reports a density a resolution cannot).
    forced_scale: Option<f32>,
    toolbar_position: ToolbarPosition,
    toolbar_autohide: bool,
    /// Auto-hide target: hidden after a scroll down, forced shown while typing.
    toolbar_shown: bool,
    /// Signed scroll distance accumulated since the last direction flip; the
    /// toolbar flips visibility once it crosses a threshold (debounces jitter).
    scroll_accum: f32,
    osk: Osk,
    /// The full-screen menu (Tabs / Bookmarks / History / Downloads); open it
    /// via [`AppUi::menu_open`] so competing overlays close.
    pub menu: Menu,
    /// The full-screen settings overlay (edits a config draft).
    pub settings: Settings,
    /// Self-update manager (About tab): in-place on PortMaster / desktop
    /// installs, "open the release page" elsewhere. See [`crate::update`].
    pub update: Updater,
    /// The built-in start page overlay's selection / search-field state.
    pub home: Home,
    /// The standalone speed-dial editor overlay (opened from the start page).
    pub dial_edit: DialEdit,
    /// Whether the active tab is on the start page (mirrored each frame from
    /// [`crate::browser::AppBrowser::on_home_page`]); drives [`Focus::Home`].
    home_active: bool,
    /// Link-hint navigation state; the rects come from the browser.
    pub hints: Hints,
    /// Modal page prompts: queued `<select>` pickers and JS dialogs.
    pub prompt: Prompt,
    /// The gamepad's latched D-pad scroll mode, drawn in place of the cursor.
    scroll_mode: bool,
    /// Per-axis direction the cursor is edge-scrolling, `(0, 0)` when not.
    edge_scroll: (i8, i8),
    /// Whether hint mode draws combo badges; off = plain spatial hops.
    hint_badges: bool,
    /// Which letters and colours the face buttons wear on screen.
    pad_layout: PadLayout,
    /// Whether the last input came from the keyboard; picks the badge alphabet.
    last_input_keyboard: bool,
    /// Whether the debug memory overlay is enabled (`[debug] memory_overlay`).
    memory_overlay: bool,
    /// Whether the same report goes to the log (`[debug] memory_log`) — how a
    /// handheld answers the question without covering its own screen.
    memory_log: bool,
    /// The latest rolled-up memory report to draw, refreshed from the main loop.
    memory_summary: Option<memory::MemorySummary>,
}

impl AppUi {
    // Each config section is passed in explicitly rather than the whole AppConfig,
    // to keep the UI's dependencies visible; that legitimately runs past the lint.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        window: &AppWindow,
        display: &DisplayConfig,
        history: &HistoryConfig,
        downloads: &DownloadsConfig,
        osk: &OskConfig,
        input: &InputConfig,
        debug: &DebugConfig,
        update: &UpdateConfig,
        user_agent: String,
    ) -> Self {
        Self {
            egui_ctx: window.egui_ctx().clone(),
            repaint_delay: None,
            webview_rect: egui::Rect::ZERO,
            toolbar_height: 0.0,
            toolbar_rect: egui::Rect::NOTHING,
            osk_height: 0.0,
            osk_lift_pending: false,
            repaint_pending: false,
            forced_passes: 1,
            browser_tex_id: window.browser_texture(),
            browser_viewport: (0, 0),
            game_mode: false,
            game_mode_toast: None,
            game_mode_toast_text: String::new(),
            game_menu: GameMenu::new(),
            input_maps: InputMaps::new(),
            map_edit: MapEdit::new(),
            input_map_name: String::new(),
            cursor: {
                // Points, like every rect it is tested against.
                let (w, h) = window.size();
                let ppp = window.egui_ctx().pixels_per_point();
                (w as f32 / ppp / 2.0, h as f32 / ppp / 2.0)
            },
            cursor_last_move: None,
            cursor_linger: Duration::from_millis(display.cursor_linger_ms),
            ui_scale: display.scale,
            forced_scale: crate::config::device_scale(),
            toolbar_position: display.toolbar_position,
            toolbar_autohide: display.toolbar_autohide,
            toolbar_shown: true,
            scroll_accum: 0.0,
            osk: Osk::new(osk, input.pad_layout),
            menu: Menu::new(history, downloads, user_agent),
            settings: Settings::new(),
            update: Updater::new(update),
            home: Home::new(),
            dial_edit: DialEdit::new(),
            home_active: false,
            hints: Hints::new(),
            prompt: Prompt::new(),
            scroll_mode: false,
            edge_scroll: (0, 0),
            hint_badges: input.hint_badges,
            pad_layout: input.pad_layout,
            last_input_keyboard: false,
            memory_overlay: debug.memory_overlay,
            memory_log: debug.memory_log,
            memory_summary: None,
        }
    }

    /// Whether anything wants Servo's memory report; drives the main loop's
    /// periodic requests.
    #[inline]
    pub fn memory_reports_wanted(&self) -> bool {
        self.memory_overlay || self.memory_log
    }

    /// Adopt edited diagnostics live. Clears the stale snapshot once nothing
    /// wants it, so it doesn't flash on the next enable.
    pub fn set_memory_debug(&mut self, overlay: bool, to_log: bool) {
        self.memory_overlay = overlay;
        self.memory_log = to_log;
        if !self.memory_reports_wanted() {
            self.memory_summary = None;
        }
    }

    /// Adopt a fresh memory report for the overlay, rolling it up for display.
    pub fn set_memory_summary(&mut self, report: servo::profile_traits::mem::MemoryReportResult) {
        self.memory_summary = Some(memory::MemorySummary::from_report(report));
    }

    /// Write the latest figures to the log, if `[debug] memory_log` asked for it.
    pub fn log_memory_summary(&self, ctx: &egui::Context, compose_bytes: usize) {
        if let (true, Some(summary)) = (self.memory_log, &self.memory_summary) {
            summary.log();
            memory::log_chrome(ctx, compose_bytes);
        }
    }

    /// Whether an egui widget (e.g. the address bar) currently wants keyboard
    /// input. Used on Android to show/hide the system soft keyboard.
    #[allow(dead_code)] // only called on Android
    pub fn wants_keyboard(&self) -> bool {
        self.egui_ctx.egui_wants_keyboard_input()
    }

    #[inline]
    pub fn take_repaint_delay(&mut self) -> Option<Duration> {
        self.repaint_delay.take()
    }

    /// Render one more frame without blocking on input: commands drain *after*
    /// [`AppUi::update`], so a UI change they make would wait for the next input.
    /// Two passes — this one draws output built before the command ran.
    #[inline]
    pub fn request_repaint(&mut self) {
        self.repaint_delay = Some(Duration::ZERO);
        self.forced_passes = 2;
    }

    /// Whether the frame just built differs from the one on the panel. Compose
    /// and present is the whole frame cost without a GPU (~90 ms at 752x560), so
    /// an identical frame is worth not drawing.
    pub fn take_frame_dirty(&mut self, window: &AppWindow) -> bool {
        let forced = self.forced_passes > 0;
        self.forced_passes = self.forced_passes.saturating_sub(1);
        forced
            || self.repaint_pending
            || window.repaint_delay() < Duration::MAX
            || self.cursor_visible_for().is_some()
            || self.toast_visible_for().is_some()
    }

    /// Height of the browser viewport (logical px) — for screen-relative scrolls.
    #[inline]
    pub fn browser_area_height(&self) -> f32 {
        self.webview_rect.height()
    }

    /// Width of the browser viewport (logical px) — keeps hit-test points inside.
    #[inline]
    pub fn browser_area_width(&self) -> f32 {
        self.webview_rect.width()
    }

    /// A window pixel (as SDL reports events) in web-view points — the space the
    /// page's own rects come back in.
    #[inline]
    pub fn to_browser_rel_pos(&self, x: f32, y: f32) -> (f32, f32) {
        browser_rel(self.to_points(x, y), self.webview_rect)
    }

    /// A window-pixel distance in points, for the deltas SDL reports in pixels.
    #[inline]
    pub fn to_points(&self, dx: f32, dy: f32) -> (f32, f32) {
        let ppp = self.egui_ctx.pixels_per_point();
        (dx / ppp, dy / ppp)
    }

    /// The gamepad cursor as a page coordinate; it is kept in points already.
    #[inline]
    pub fn cursor_browser_rel(&self) -> (f32, f32) {
        browser_rel(self.cursor, self.webview_rect)
    }

    /// Refresh egui's cached window size once per frame: SDL delivers no
    /// size-changed event on Android rotation, and the UI would keep laying out
    /// for the previous orientation.
    #[cfg(target_os = "android")]
    pub fn sync_window_size(&mut self, window: &mut AppWindow) {
        window.sync_egui_window_size();
    }

    pub fn handle_event(&mut self, window: &mut AppWindow, event: &sdl2::event::Event) -> bool {
        let resp = window.on_event(event);
        self.repaint_pending |= resp.repaint;
        // don't consume when pointer over browser area
        resp.consumed & self.is_pointer_over_toolbar(window)
    }

    /// Fold the idle-repaint sources into `repaint_delay` so the blocking wait
    /// wakes when something time-based comes due. Each source only shortens it.
    fn schedule_idle_repaints(&mut self, cursor_visible: Option<Duration>) {
        self.repaint_delay = cursor_visible;
        // Without this the wait blocks on input and the hint refresh never fires.
        if let Some(refresh) = self.hints.refresh_in() {
            self.repaint_delay = Some(self.repaint_delay.map_or(refresh, |d| d.min(refresh)));
        }
        // ~1 Hz while a report is wanted, so its periodic request fires when idle.
        if self.memory_reports_wanted() {
            let tick = Duration::from_secs(1);
            self.repaint_delay = Some(self.repaint_delay.map_or(tick, |d| d.min(tick)));
        }
        // The Game Mode toast needs one wake at its expiry to be erased.
        if let Some(left) = self.toast_visible_for() {
            self.repaint_delay = Some(self.repaint_delay.map_or(left, |d| d.min(left)));
        }
    }

    /// Snapshot the per-frame browser inputs as owned copies, so the browser
    /// borrow doesn't leak into `egui.run`. The tab reads happen *before* the
    /// long `get_state_mut` borrow below: both touch the browser's tab list.
    fn frame_snapshot(&mut self, browser: &mut AppBrowser) -> FrameInputs {
        let tab_count = browser.tab_count();
        let zoom_pct = browser.zoom_chip();
        let tab_infos = if self.menu.visible {
            self.menu.set_tab_count(browser.tab_count());
            browser.tabs()
        } else {
            Vec::new()
        };
        FrameInputs {
            tab_count,
            zoom_pct,
            tab_infos,
            osk_field: self.osk_target_field(),
            osk_caret: self.osk.caret(),
            chrome_hidden: ChromeHidden {
                page_fullscreen: browser.is_fullscreen(),
                game_mode: self.game_mode,
            },
        }
    }

    /// Keep the start-page / dial-editor selections in range before they render.
    fn clamp_overlay_selections(&mut self) {
        if self.home_active {
            self.home.clamp(home::slot_count(self.menu.dial.urls()));
        }
        if self.dial_edit.visible() {
            // The grid is the pins plus a trailing "Pin settings" tile while the
            // settings shortcut is off the dial.
            let slots = self.dial_edit_slots();
            self.dial_edit.clamp(slots);
        }
    }

    pub fn update(
        &mut self,
        window: &mut AppWindow,
        browser: &mut AppBrowser,
        commands: &mut Vec<AppCommand>,
    ) {
        let mut desired_px: Option<(u32, u32)> = None;
        // A software resize leaves a fresh context behind; adopt it before
        // anything queries it.
        if self.egui_ctx != *window.egui_ctx() {
            self.egui_ctx = window.egui_ctx().clone();
        }
        // Before the layout: every rect below is measured in the zoom in force.
        self.sync_scale(browser);

        // The cursor draws only while it lingers; wake at the end of the linger
        // so it gets erased without another event.
        let cursor_visible = if self.focus() == Focus::Page {
            self.cursor_visible_for()
        } else {
            None
        };
        self.schedule_idle_repaints(cursor_visible);

        let snapshot = self.frame_snapshot(browser);
        let face = self.pad_layout.labels();
        // Android's system bars follow the chrome: whatever hides the toolbar
        // wants the whole panel.
        #[cfg(target_os = "android")]
        window.set_system_fullscreen(snapshot.chrome_hidden.any());
        self.clamp_overlay_selections();

        {
            let mut state = browser.get_state_mut();
            // `caret_for` parks a `TextEdit`'s cursor at the OSK caret, for the
            // one field the OSK types into.
            let FrameInputs {
                tab_count,
                zoom_pct,
                tab_infos,
                osk_field,
                osk_caret,
                chrome_hidden,
            } = snapshot;
            let caret_for = |f| (osk_field == f).then_some(osk_caret);
            let ToolbarLayout {
                position,
                shown: toolbar_shown,
                overlay: toolbar_overlay,
            } = self.toolbar_layout(chrome_hidden);
            // The `self.update` borrow can't overlap the `self`-borrowing closure.
            let update = self.update.snapshot();
            // Toolbar update chip: shown only once a check has found a newer build.
            let update_available = matches!(update, UpdateState::Available { .. });
            window.run_ui(|ctx| {
                let ppp = ctx.pixels_per_point();
                let mut root = egui::Ui::new(
                    ctx.clone(),
                    egui::Id::new("root_ui"),
                    egui::UiBuilder::new().max_rect(ctx.content_rect()),
                );
                root.set_clip_rect(ctx.content_rect());

                let inputs = toolbar::ToolbarInputs {
                    bookmarked: self.menu.is_bookmarked(&state.location),
                    tab_count,
                    active_downloads: self.menu.downloads.active_count(),
                    update_available,
                    zoom_pct,
                    osk_caret: caret_for(OskField::AddressBar),
                    position,
                };

                // 1) Reserved-space toolbar (auto-hide off): the panel reserves
                //    its strip and the page reflows below it.
                if !toolbar_overlay && toolbar_shown {
                    self.toolbar_rect =
                        toolbar::add_toolbar(&mut root, &mut state, commands, &inputs);
                } else {
                    self.toolbar_rect = egui::Rect::NOTHING;
                }

                // 2) Web view fills whatever's left — the full window when no panel
                //    was reserved. Its size is the viewport we send to Servo.
                let frame = egui::Frame::default().inner_margin(0.0);
                egui::CentralPanel::default()
                    .frame(frame)
                    .show(&mut root, |ui| {
                        let rect = ui.max_rect();
                        self.webview_rect = rect;
                        // Panel mode: toolbar thickness is whatever the full content
                        // rect has that this doesn't. The overlay measures its own.
                        if !toolbar_overlay {
                            self.toolbar_height = ctx.content_rect().height() - rect.height();
                        }
                        ui.allocate_rect(rect, egui::Sense::hover());

                        desired_px = Some((
                            (rect.width() * ppp).round().max(1.0) as u32,
                            (rect.height() * ppp).round().max(1.0) as u32,
                        ));

                        // In software mode the page is composited under the chrome
                        // by the window itself, and this rect stays empty.
                        if let Some(tex) = self.browser_tex_id {
                            // WebRender renders bottom-up into the FBO, so flip V.
                            let uv = egui::Rect::from_min_max(
                                egui::pos2(0.0, 1.0),
                                egui::pos2(1.0, 0.0),
                            );
                            ui.painter().image(tex, rect, uv, egui::Color32::WHITE);
                        }
                    });

                // 3) Floating overlay toolbar (auto-hide, either edge): over the
                //    full-height web view, so toggling it resizes nothing.
                if toolbar_overlay {
                    // Draw only while shown: a foreground `Area` costs a
                    // tessellation pass every frame even off-screen.
                    if toolbar_shown {
                        self.toolbar_rect = toolbar::add_toolbar_overlay(
                            ctx,
                            ctx.content_rect().width(),
                            &mut state,
                            commands,
                            &inputs,
                        );
                        self.toolbar_height = self.toolbar_rect.height();
                    } else {
                        self.toolbar_rect = egui::Rect::NOTHING;
                    }
                }

                // A backdrop over the blank web view, below the foreground
                // overlays; the dial editor covers it entirely.
                if self.home_active && !self.dial_edit.visible() {
                    home::add_home(
                        ctx,
                        &mut self.home,
                        self.menu.dial.urls(),
                        self.webview_rect,
                        caret_for(OskField::Home),
                        face,
                        commands,
                    );
                }

                // Above the start page; the OSK can still open on top to type a URL.
                if self.dial_edit.visible() {
                    dial_edit::add_dial_edit(
                        ctx,
                        &mut self.dial_edit,
                        self.menu.dial.urls(),
                        caret_for(OskField::DialEdit),
                        face,
                        commands,
                    );
                }

                // Before the menu/OSK chain, so the OSK can open on top of a field.
                if self.settings.visible() {
                    // The overlay owns its row selection; an egui-focused row
                    // would take Enter a second time and activate twice.
                    drop_egui_focus(ctx);
                    settings::add_settings(ctx, &self.settings, &update, face, commands);
                }

                // Its own block rather than the chain below, so the keyboard can
                // open over it to pick a key for a row.
                if self.map_edit.visible() {
                    drop_egui_focus(ctx);
                    game::map_edit::add_map_edit(ctx, &self.map_edit, commands);
                }

                // Same, for the keyboard that types a map name.
                if self.input_maps.visible() {
                    drop_egui_focus(ctx);
                    game::input_maps::add_input_maps(ctx, &self.input_maps, commands);
                }

                // Its layer order puts it above whatever else is up.
                if self.prompt.visible() {
                    // Last frame's height; the OSK is drawn after this.
                    let osk_lift = if self.osk.visible {
                        self.osk_height
                    } else {
                        0.0
                    };
                    prompt::add_prompt(
                        ctx,
                        &mut self.prompt,
                        caret_for(OskField::Prompt),
                        osk_lift,
                        face,
                        commands,
                    );
                }

                if self.menu.visible {
                    drop_egui_focus(ctx);
                    menu::add_menu(ctx, &self.menu, &tab_infos, face, commands);
                } else if self.game_menu.visible {
                    // Same as the menu's: a focused row would activate twice.
                    drop_egui_focus(ctx);
                    game::menu::add_game_menu(
                        ctx,
                        &self.game_menu,
                        &self.input_map_name,
                        self.game_mode,
                        commands,
                    );
                } else if self.osk.visible {
                    // Clear a bottom toolbar so its address bar stays visible
                    // below the keys.
                    let bottom_inset = match self.toolbar_position {
                        ToolbarPosition::Bottom => self.toolbar_height,
                        ToolbarPosition::Top => 0.0,
                    };
                    self.osk_height =
                        osk::add_osk(ctx, &self.osk, self.pad_layout, bottom_inset) + bottom_inset;
                } else if self.hints.visible {
                    // Rects the page has scrolled out from under are left
                    // undrawn: a badge would mark whatever took that place.
                    if !self.hints.is_stale() {
                        let badges = self.hint_badges.then_some(face);
                        hints::add_hints(ctx, &self.hints, self.webview_rect, badges);
                    }
                } else if cursor_visible.is_some() {
                    let pos = egui::pos2(self.cursor.0, self.cursor.1);
                    cursor::paint_cursor(ctx, pos, self.scroll_mode, self.edge_scroll);
                }

                if self.toast_visible_for().is_some() {
                    game_mode::add_game_mode_toast(ctx, &self.game_mode_toast_text);
                }

                // Drawn last so it sits above everything; non-interactive, so it
                // never blocks input.
                if self.memory_overlay {
                    if let Some(summary) = &self.memory_summary {
                        memory::add_memory(ctx, summary);
                    }
                }
            });
        }

        if let Some(size) = desired_px {
            if size != self.browser_viewport {
                self.browser_viewport = size;
                browser.resize(size.0, size.1);
            }
        }

        // Now that the keyboard's height is known, scroll the field clear of it.
        if self.osk_lift_pending && self.osk_height > 0.0 {
            let covered = (self.osk_height / self.webview_rect.height()).clamp(0.0, 0.9);
            browser.lift_focus_above(covered);
            self.osk_lift_pending = false;
        }

        // A freshly shown anchored `Area` sizes itself invisibly and asks for a
        // follow-up to paint positioned; without this it waits for a keypress.
        let egui_delay = window.repaint_delay();
        if egui_delay < Duration::MAX {
            self.repaint_delay = Some(self.repaint_delay.map_or(egui_delay, |d| d.min(egui_delay)));
        }
    }

    /// Paints the UI (toolbar over the page) and presents to the window.
    pub fn draw(
        &mut self,
        window: &mut AppWindow,
        page_painted: bool,
    ) -> Option<crate::platform::window::CompositeTiming> {
        // Where the software backend composites the page frame; the GL backend
        // draws it as a texture in the same rect and ignores this.
        let ppp = self.egui_ctx.pixels_per_point();
        let page_at = (
            (self.webview_rect.left() * ppp).round() as i32,
            (self.webview_rect.top() * ppp).round() as i32,
        );
        let timing = window.paint(page_at, page_painted);
        self.repaint_pending = false;
        timing
    }
}

/// A point in screen points as the page's own coordinate — only the web view's
/// origin comes off. Both devices end here: the cursor is kept in points and
/// SDL reports pixels, so one conversion too many aims one of them elsewhere.
fn browser_rel((x, y): (f32, f32), webview: egui::Rect) -> (f32, f32) {
    (x - webview.left(), y - webview.top())
}

#[cfg(test)]
mod tests {
    use super::{browser_rel, egui, ChromeHidden};

    /// The cursor is kept in points and SDL reports pixels; converting the
    /// cursor twice put a press at the corner into the middle of the page.
    #[test]
    fn the_pad_cursor_and_the_mouse_land_on_the_same_page_pixel() {
        let webview = egui::Rect::from_min_size(egui::pos2(0.0, 33.0), egui::vec2(640.0, 447.0));
        let ppp = 2.0;
        // A point near the bottom-right corner, where the two diverged most.
        let cursor = (636.0, 476.0);
        let mouse_px = (cursor.0 * ppp, cursor.1 * ppp);
        // What the mouse path does before it gets here, and the cursor must not.
        let to_points = |(x, y): (f32, f32)| (x / ppp, y / ppp);
        let from_mouse = browser_rel(to_points(mouse_px), webview);
        assert_eq!(browser_rel(cursor, webview), from_mouse);
        assert_eq!(from_mouse, (636.0, 443.0));
    }

    /// The reasons are separate so that a page dropping fullscreen inside Game
    /// Mode — which it does on every navigation — cannot flash the chrome back.
    #[test]
    fn one_reason_ending_does_not_reveal_the_chrome_while_the_other_holds() {
        let both = ChromeHidden {
            page_fullscreen: true,
            game_mode: true,
        };
        assert!(both.any());
        assert!(ChromeHidden {
            page_fullscreen: false,
            ..both
        }
        .any());
        assert!(ChromeHidden {
            game_mode: false,
            ..both
        }
        .any());
        assert!(!ChromeHidden::default().any());
    }
}
