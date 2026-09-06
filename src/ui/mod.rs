//! The egui layer: [`AppUi`] owns the gamepad cursor and the overlay state
//! holders, and lays the chrome out over the page. egui itself lives in
//! [`crate::platform::window::AppWindow`], which owns the renderer it belongs
//! to; the widgets are rendered by the submodules ([`toolbar`], [`menu`], [`osk`]).

mod cursor;
mod dial_edit;
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

pub use self::overlays::Focus;

use crate::{
    app::AppCommand,
    browser::AppBrowser,
    config::{
        DebugConfig, DisplayConfig, DownloadsConfig, HistoryConfig, InputConfig, OskConfig,
        ToolbarPosition, UpdateConfig,
    },
    overlay::dial_edit::DialEdit,
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

/// The text field the OSK currently types into, so its renderer can park egui's
/// caret at the buffer end — egui keys its caret by widget id and won't follow an
/// external edit. `None` when the OSK is hidden or types somewhere without an
/// egui caret (the page, or a settings row, whose value is painted text).
#[derive(PartialEq, Eq, Clone, Copy)]
pub(super) enum OskField {
    None,
    AddressBar,
    DialEdit,
    Prompt,
    Home,
}

/// Drop egui's own keyboard focus, so Enter reaches only the overlay's selected
/// row (see the callers).
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
    /// Open tab count, shown in the toolbar's tab chip.
    tab_count: usize,
    /// Page-zoom chip percentage (`None` at the default zoom).
    zoom_pct: Option<u16>,
    /// Tab snapshots for the menu's Tabs section (empty unless the menu is open).
    tab_infos: Vec<crate::browser::TabInfo>,
    /// The field the OSK types into this frame (if any).
    osk_field: OskField,
    /// Where the OSK's caret sits, mirrored into each `TextEdit`.
    osk_caret: usize,
}

pub struct AppUi {
    /// The window's egui context, cached so focus/scale queries don't each need
    /// the window. Refreshed every frame: the software backend builds a fresh
    /// context on a resize.
    egui_ctx: egui::Context,
    repaint_delay: Option<Duration>,
    /// The web view's rect (logical px), measured from the central panel each
    /// frame. Maps cursor/browser coordinates, keeps the pointer out of the
    /// toolbar, and anchors the home/hints overlays.
    webview_rect: egui::Rect,
    /// Toolbar thickness (logical px), measured each frame. Stays valid across
    /// window-size changes, so the SDL-resize fast path sizes the viewport with it.
    toolbar_height: f32,
    /// The toolbar's current on-screen rect (logical px) — the panel strip, or
    /// the auto-hide overlay's slid position (off-screen when hidden). Used by the
    /// hit-tests to tell "this points at the chrome" from "this points at the page".
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
    /// Gamepad cursor position (logical px). The UI owns it — it draws the
    /// overlay — and the gamepad moves it via `move_cursor` (see [`cursor`]).
    cursor: (f32, f32),
    /// When the cursor last moved, or `None` if it has never moved. Drives the
    /// auto-hide: the overlay shows only within `cursor_linger` of this.
    cursor_last_move: Option<Instant>,
    /// How long the cursor stays visible after a move (from the interface config).
    cursor_linger: Duration,
    /// `[display] scale`: the user's factor over the fit to the panel. Applied live.
    ui_scale: f32,
    /// `RETSURF_SCALE`, standing in for the panel's own fit where a launcher
    /// knows better (Android, which reports a density a resolution cannot).
    forced_scale: Option<f32>,
    /// Which edge the toolbar renders on (from the display config). Applied live.
    toolbar_position: ToolbarPosition,
    /// Whether the toolbar hides on scroll-down / reveals on scroll-up (config).
    toolbar_autohide: bool,
    /// Auto-hide target visibility: shown after a scroll up, hidden after a
    /// scroll down; forced shown while a field is typed into.
    toolbar_shown: bool,
    /// Signed scroll distance accumulated since the last direction flip; the
    /// toolbar flips visibility once it crosses a threshold (debounces jitter).
    scroll_accum: f32,
    /// On-screen keyboard: state, rendering, and input routing all live here.
    osk: Osk,
    /// The full-screen menu (Tabs / Bookmarks / History / Downloads). Public —
    /// driven directly; open via [`AppUi::menu_open`] so competing overlays close.
    pub menu: Menu,
    /// The full-screen settings overlay (edits a config draft). Public — driven
    /// directly; open/close/move go through the `settings_*` coordinators.
    pub settings: Settings,
    /// Self-update manager (About tab): in-place on PortMaster / desktop
    /// installs, "open the release page" elsewhere. See [`crate::update`].
    update: Updater,
    /// The built-in start page overlay's selection / search-field state.
    home: Home,
    /// The standalone speed-dial editor overlay (opened from the start page).
    dial_edit: DialEdit,
    /// Whether the active tab is on the start page (mirrored each frame from
    /// [`crate::browser::AppBrowser::on_home_page`]); drives [`Focus::Home`].
    home_active: bool,
    /// Link-hint navigation state (L3); rects come from the browser. Public —
    /// a round starts via `hints_begin_collect` / `hints_apply` (see [`overlays`]).
    pub hints: Hints,
    /// Modal page prompts: queued `<select>` pickers and JS dialogs. Public —
    /// the router and main loop drive [`Prompt`]'s own methods directly.
    pub prompt: Prompt,
    /// The gamepad's latched D-pad scroll mode, mirrored each frame by the
    /// router; drawn as an autoscroll-style indicator in place of the cursor.
    scroll_mode: bool,
    /// Whether hint mode draws combo badges (cached from the input config; the
    /// router gates symbol routing on the same flag). Off = plain spatial hops.
    hint_badges: bool,
    /// Whether the most recent input came from the keyboard (vs a gamepad).
    /// Read when hint mode opens to pick the badge alphabet.
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
            osk: Osk::new(osk),
            menu: Menu::new(history, downloads, user_agent),
            settings: Settings::new(),
            update: Updater::new(update),
            home: Home::new(),
            dial_edit: DialEdit::new(),
            home_active: false,
            hints: Hints::new(),
            prompt: Prompt::new(),
            scroll_mode: false,
            hint_badges: input.hint_badges,
            last_input_keyboard: false,
            memory_overlay: debug.memory_overlay,
            memory_log: debug.memory_log,
            memory_summary: None,
        }
    }

    /// Whether anything wants Servo's memory report — the overlay to draw it or
    /// the log to record it. Drives the main loop's periodic requests (see
    /// [`crate::browser::AppBrowser::request_memory_report`]).
    #[inline]
    pub fn memory_reports_wanted(&self) -> bool {
        self.memory_overlay || self.memory_log
    }

    /// Adopt edited diagnostics live (from a settings save). Clears the stale
    /// snapshot once nothing wants it, so it doesn't flash on the next enable.
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
    /// `compose_bytes` is the compositor's, from [`AppWindow::compose_bytes`].
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
    pub fn take_repain_delay(&mut self) -> Option<Duration> {
        self.repaint_delay.take()
    }

    /// Ask the loop to render one more frame without blocking on input: commands
    /// drain *after* this frame's [`AppUi::update`], so a UI change they make
    /// would otherwise wait for the next input. Two passes, not one — this pass
    /// draws output built before the command ran; the next holds the change.
    #[inline]
    pub fn request_repaint(&mut self) {
        self.repaint_delay = Some(Duration::ZERO);
        self.forced_passes = 2;
    }

    /// Whether the frame just built differs from the one on the panel. Composing
    /// and presenting is the whole frame cost on a GPU-less device (~90 ms at
    /// 752x560), so an identical frame is worth not drawing. egui reporting `MAX`
    /// means idle; a consumed event, an animation, or the cursor say otherwise.
    pub fn take_frame_dirty(&mut self, window: &AppWindow) -> bool {
        let forced = self.forced_passes > 0;
        self.forced_passes = self.forced_passes.saturating_sub(1);
        forced
            || self.repaint_pending
            || window.repaint_delay() < Duration::MAX
            || self.cursor_visible_for().is_some()
    }

    /// Move the toolbar to a window edge (live config change). The next frame
    /// re-lays the panel and the web view follows automatically.
    #[inline]
    pub fn set_toolbar_position(&mut self, pos: ToolbarPosition) {
        self.toolbar_position = pos;
    }

    /// Enable/disable scroll-driven auto-hide (live config change). Re-showing
    /// the toolbar avoids leaving it stuck hidden from a prior scroll.
    #[inline]
    pub fn set_toolbar_autohide(&mut self, on: bool) {
        self.toolbar_autohide = on;
        if !on {
            self.toolbar_shown = true;
        }
    }

    /// Feed a page-scroll delta (the same `dy` handed to [`AppBrowser::scroll`]:
    /// positive reveals lower content) so the toolbar can hide on scroll-down and
    /// reveal on scroll-up. Accumulates to a threshold; a no-op without auto-hide.
    pub fn notify_page_scroll(&mut self, dy: f32) {
        if !self.toolbar_autohide || dy == 0.0 {
            return;
        }
        // Reset the accumulator on a direction change so a flick the other way
        // responds immediately instead of cancelling out a long prior drag.
        if (dy > 0.0) != (self.scroll_accum > 0.0) {
            self.scroll_accum = 0.0;
        }
        self.scroll_accum += dy;
        const THRESHOLD: f32 = 48.0;
        if self.scroll_accum > THRESHOLD {
            self.toolbar_shown = false;
            self.scroll_accum = 0.0;
        } else if self.scroll_accum < -THRESHOLD {
            self.toolbar_shown = true;
            self.scroll_accum = 0.0;
        }
    }

    /// Height of the browser viewport (logical px) — for screen-relative scrolls.
    #[inline]
    pub fn browser_area_height(&self) -> f32 {
        self.webview_rect.height()
    }

    /// Width of the browser viewport (logical px) — to keep scroll hit-test
    /// points inside the visible area.
    #[inline]
    pub fn browser_area_width(&self) -> f32 {
        self.webview_rect.width()
    }

    /// A window pixel (as SDL reports events) in web-view points — the space the
    /// page's own rects come back in, and what [`AppBrowser`] is fed.
    #[inline]
    pub fn to_browser_rel_pos(&self, x: f32, y: f32) -> (f32, f32) {
        let ppp = self.egui_ctx.pixels_per_point();
        (
            x / ppp - self.webview_rect.left(),
            y / ppp - self.webview_rect.top(),
        )
    }

    /// A window-pixel distance in points, for the deltas SDL reports in pixels.
    #[inline]
    pub fn to_points(&self, dx: f32, dy: f32) -> (f32, f32) {
        let ppp = self.egui_ctx.pixels_per_point();
        (dx / ppp, dy / ppp)
    }

    /// Resize the browser to the web-view area on SDL window-resize events:
    /// egui's reactive sizing reads the central rect a frame later and lags the
    /// window. Uses the toolbar height measured in [`AppUi::update`] and shares
    /// `browser_viewport` with it, so the two never double-resize.
    pub fn resize_browser(&mut self, window: &AppWindow, browser: &AppBrowser) {
        let (dw, dh) = window.drawable_size();
        if dw == 0 || dh == 0 {
            return;
        }
        // An auto-hide bar floats as an overlay on either edge, so the web view
        // stays full-height; without auto-hide the bar reserves a strip.
        let overlay = self.toolbar_autohide;
        let toolbar_px = if overlay {
            0
        } else {
            (self.toolbar_height * self.egui_ctx.pixels_per_point()).round() as u32
        };
        let size = (dw, dh.saturating_sub(toolbar_px).max(1));
        if size != self.browser_viewport {
            self.browser_viewport = size;
            self.forced_passes = self.forced_passes.max(1);
            browser.resize(size.0, size.1);
        }
    }

    /// Refresh egui's cached window size from the live window, once per frame:
    /// SDL doesn't deliver a size-changed event on Android rotation, and the UI
    /// would keep laying out for the previous orientation.
    #[cfg(target_os = "android")]
    pub fn sync_window_size(&mut self, window: &mut AppWindow) {
        window.sync_egui_window_size();
    }

    /// Handles the event and returns whether it is consumed
    pub fn handle_event(&mut self, window: &mut AppWindow, event: &sdl2::event::Event) -> bool {
        let resp = window.on_event(event);
        self.repaint_pending |= resp.repaint;
        // don't consume when pointer over browser area
        resp.consumed & self.is_pointer_over_toolbar(window)
    }

    /// Fold the idle-repaint sources into `repaint_delay` so the blocking wait
    /// wakes on its own when something time-based comes due. Each source only
    /// ever shortens the wait; `None` leaves it untouched.
    fn schedule_idle_repaints(&mut self, cursor_visible: Option<Duration>) {
        self.repaint_delay = cursor_visible;
        // A pending post-scroll hint refresh also needs the loop to wake by
        // itself — without this the wait blocks on input and it never fires.
        if let Some(refresh) = self.hints.refresh_in() {
            self.repaint_delay = Some(self.repaint_delay.map_or(refresh, |d| d.min(refresh)));
        }
        // Keep the loop ticking ~1 Hz while a memory report is wanted, so its
        // periodic request fires and the figures stay fresh when idle.
        if self.memory_reports_wanted() {
            let tick = Duration::from_secs(1);
            self.repaint_delay = Some(self.repaint_delay.map_or(tick, |d| d.min(tick)));
        }
    }

    /// Snapshot the per-frame browser/overlay inputs as owned copies, so the
    /// browser borrow doesn't leak into `egui.run` (which borrows `self`). The
    /// tab reads happen *before* the long `get_state_mut` borrow taken just
    /// before `run`: both touch the browser's tab list and can't overlap.
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
        }
    }

    /// Keep the start-page / speed-dial-editor selections in range before they
    /// render. The pin list isn't snapshotted — the overlays borrow it straight
    /// from the live store at their call sites.
    fn clamp_overlay_selections(&mut self) {
        let pin_count = self.menu.dial.urls().len();
        if self.home_active {
            // +1 for the trailing "Edit" tile, so its selection isn't clamped off.
            self.home.clamp(pin_count + 1);
        }
        if self.dial_edit.visible() {
            // The editor's grid is the pins plus, while the ⚙ shortcut is off the
            // dial, the trailing "Pin settings" tile.
            let slots = self.dial_edit_slots();
            self.dial_edit.clamp(slots);
        }
    }

    /// Decide the toolbar layout before the egui closure (these reads — esp.
    /// `focus()` — borrow all of `self`, which can't overlap `egui.run`).
    /// Auto-hide forces the bar visible while typing and floats it on either
    /// edge: a strip that came and went would resize the web view, and that is
    /// a full Servo reflow mid-scroll. Otherwise the bar is a panel.
    fn toolbar_layout(&self) -> ToolbarLayout {
        let typing = self.focus() != Focus::Page;
        ToolbarLayout {
            position: self.toolbar_position,
            shown: !self.toolbar_autohide || self.toolbar_shown || typing,
            overlay: self.toolbar_autohide,
        }
    }

    pub fn update(
        &mut self,
        window: &mut AppWindow,
        browser: &mut AppBrowser,
        commands: &mut Vec<AppCommand>,
    ) {
        let mut desired_px: Option<(u32, u32)> = None;
        // A software resize (handled in the previous frame's paint) leaves a
        // fresh context behind; adopt it before anything queries it.
        if self.egui_ctx != *window.egui_ctx() {
            self.egui_ctx = window.egui_ctx().clone();
        }
        // Before the layout: every rect below is measured in the zoom in force.
        self.sync_scale(browser);

        // The cursor draws only while it lingers after a move; ask the loop to
        // wake when the linger ends so it gets erased without another event.
        let cursor_visible = if self.focus() == Focus::Page {
            self.cursor_visible_for()
        } else {
            None
        };
        self.schedule_idle_repaints(cursor_visible);

        let snapshot = self.frame_snapshot(browser);
        self.clamp_overlay_selections();

        {
            let mut state = browser.get_state_mut();
            // Owned copies (see `FrameInputs`); `caret_for` parks each
            // `TextEdit`'s cursor at the OSK caret for the field it types into.
            let FrameInputs {
                tab_count,
                zoom_pct,
                tab_infos,
                osk_field,
                osk_caret,
            } = snapshot;
            let caret_for = |f| (osk_field == f).then_some(osk_caret);
            let ToolbarLayout {
                position,
                shown: toolbar_shown,
                overlay: toolbar_overlay,
            } = self.toolbar_layout();
            // Snapshot the self-update state here (the About tab reads it): the
            // `self.update` borrow can't overlap the `self`-borrowing closure below.
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

                let bookmarked = self.menu.is_bookmarked(&state.location);
                let active_downloads = self.menu.downloads.active_count();

                // 1) Reserved-space toolbar (auto-hide off): the panel reserves
                //    its strip and the page reflows below it.
                if !toolbar_overlay && toolbar_shown {
                    self.toolbar_rect = toolbar::add_toolbar(
                        &mut root,
                        &mut state,
                        commands,
                        bookmarked,
                        tab_count,
                        active_downloads,
                        update_available,
                        zoom_pct,
                        caret_for(OskField::AddressBar),
                        position,
                    );
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
                            bookmarked,
                            tab_count,
                            active_downloads,
                            update_available,
                            zoom_pct,
                            caret_for(OskField::AddressBar),
                            position,
                        );
                        self.toolbar_height = self.toolbar_rect.height();
                    } else {
                        self.toolbar_rect = egui::Rect::NOTHING;
                    }
                }

                // The start page is a backdrop over the (blank) web view, drawn
                // below the foreground overlays so they can open on top. The dial
                // editor fully covers it, so skip it underneath.
                if self.home_active && !self.dial_edit.visible() {
                    home::add_home(
                        ctx,
                        &mut self.home,
                        self.menu.dial.urls(),
                        self.webview_rect,
                        caret_for(OskField::Home),
                        commands,
                    );
                }

                // The speed-dial editor: a full-screen overlay above the start
                // page; the OSK (below) can still open on top to type a URL.
                if self.dial_edit.visible() {
                    dial_edit::add_dial_edit(
                        ctx,
                        &mut self.dial_edit,
                        self.menu.dial.urls(),
                        caret_for(OskField::DialEdit),
                        commands,
                    );
                }

                // Settings: drawn before the menu/OSK chain so the OSK can open
                // on top to type into a text field.
                if self.settings.visible() {
                    // The overlay owns its row selection; an egui-focused row
                    // would take Enter a second time and activate twice.
                    drop_egui_focus(ctx);
                    settings::add_settings(ctx, &self.settings, &update, commands);
                }

                // The modal prompt draws on top of whatever else is up (its
                // egui layer order puts it above the other overlays).
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
                        commands,
                    );
                }

                if self.menu.visible {
                    drop_egui_focus(ctx);
                    menu::add_menu(ctx, &self.menu, &tab_infos, commands);
                } else if self.osk.visible {
                    // Clear a bottom toolbar so its address bar stays visible
                    // below the keys; a top toolbar needs no inset.
                    let bottom_inset = match self.toolbar_position {
                        ToolbarPosition::Bottom => self.toolbar_height,
                        ToolbarPosition::Top => 0.0,
                    };
                    self.osk_height = osk::add_osk(ctx, &self.osk, bottom_inset) + bottom_inset;
                } else if self.hints.visible {
                    hints::add_hints(ctx, &self.hints, self.webview_rect, self.hint_badges);
                } else if cursor_visible.is_some() {
                    let pos = egui::pos2(self.cursor.0, self.cursor.1);
                    cursor::paint_cursor(ctx, pos, self.scroll_mode);
                }

                // Debug memory overlay (opt-in), drawn last so it sits above
                // everything; non-interactive, so it never blocks input.
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

        // The keyboard just opened over a page field: now that it has been drawn
        // (so its height is known), ask the page to scroll the field clear of it.
        if self.osk_lift_pending && self.osk_height > 0.0 {
            let covered = (self.osk_height / self.webview_rect.height()).clamp(0.0, 0.9);
            browser.lift_focus_above(covered);
            self.osk_lift_pending = false;
        }

        // Fold in egui's own repaint timing: a freshly shown anchored `Area`
        // sizes itself invisibly and asks for an immediate follow-up to paint
        // positioned — without this the overlay appears only after the next
        // keypress. `MAX` means egui is idle, so it never shortens our wait.
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

    #[inline]
    fn is_pointer_over_toolbar(&self, window: &AppWindow) -> bool {
        let Some(pos) = window.pointer_pos_in_points() else {
            return false;
        };
        self.toolbar_rect.contains(pos)
    }

    /// Whether a *pixel*-space y (raw SDL finger events) lands in the web view,
    /// below the toolbar: touches over the toolbar are egui's, so only web-view
    /// touches should start a page scroll/tap gesture.
    #[inline]
    pub fn point_over_webview(&self, y_px: f32) -> bool {
        let y = y_px / self.egui_ctx.pixels_per_point();
        !self.toolbar_rect.y_range().contains(y)
    }
}
