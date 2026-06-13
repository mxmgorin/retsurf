//! The egui layer: [`AppUi`] owns the egui context, the gamepad cursor, and the
//! overlay state holders ([`crate::overlay::menu`], [`crate::overlay::osk`]), and composites
//! Servo's FBO texture under the chrome. The actual widgets are rendered by the
//! submodules: [`toolbar`], [`menu`] (the full-screen overlay), and [`osk`].

mod dial_edit;
mod hints;
mod home;
mod menu;
mod osk;
mod prompt;
mod settings;
mod theme;
mod toolbar;

use crate::{
    app::AppCommand,
    browser::AppBrowser,
    config::{AppConfig, DownloadsConfig, HistoryConfig, InterfaceConfig, OskConfig},
    event::user::UserEventSender,
    overlay::dial_edit::{DialEdit, EditItem},
    overlay::hints::{Hint, Hints},
    overlay::home::Home,
    overlay::menu::{Menu, Section},
    overlay::osk::{Osk, OskCommand, OskTarget},
    overlay::prompt::Prompt,
    overlay::settings::{Settings, SettingsSection},
    platform::window::AppWindow,
};
use egui_sdl2::egui;
use egui_sdl2::EguiGlow;
use std::time::{Duration, Instant};

/// The text field the OSK currently types into, so its renderer can park egui's
/// caret at the buffer end. The OSK only appends / backspaces, but egui keeps its
/// own caret keyed by widget id and won't follow an external edit — left alone it
/// sticks at the start and typed text scrolls out of view. `None` when the OSK is
/// hidden or types somewhere without an egui caret (the page, or a settings row,
/// whose value is painted text not a `TextEdit`).
#[derive(PartialEq, Eq, Clone, Copy)]
pub(super) enum OskField {
    None,
    AddressBar,
    DialEdit,
    Prompt,
    Home,
}

/// Park egui's caret at the end of an externally-edited single-line `TextEdit`
/// (see [`OskField`]). Call it *before* the field renders so the caret lands
/// this frame; egui reloads the state we store here when it draws.
pub(super) fn park_caret_end(ctx: &egui::Context, id: egui::Id, char_count: usize) {
    let mut state = egui::TextEdit::load_state(ctx, id).unwrap_or_default();
    let end = egui::text::CCursor::new(char_count);
    state
        .cursor
        .set_char_range(Some(egui::text::CCursorRange::one(end)));
    egui::TextEdit::store_state(ctx, id, state);
}

/// Gamepad cursor overlay: circle radius and outline width (logical px).
const CURSOR_RADIUS: f32 = 5.0;
const CURSOR_STROKE: f32 = 1.5;
/// The cursor's full painted half-extent — how far it reaches from its center,
/// used to keep the whole glyph (not just the center) inside the web view.
const CURSOR_EXTENT: f32 = CURSOR_RADIUS + CURSOR_STROKE / 2.0;

/// Which surface owns contextual input (Confirm, Cancel, overlay `Nav` steps)
/// right now: one precedence order derived from the overlay visibility flags,
/// so the router and event handlers match on this instead of re-combining
/// `*_visible()` checks at every site. The on-screen keyboard outranks the
/// modal prompt (it's how a gamepad types into one), the prompt outranks the
/// user overlays; menu / keyboard / hints never coexist (opening one closes
/// the others).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Focus {
    /// The on-screen keyboard — above everything, including the modal prompt.
    Osk,
    /// A modal page prompt (select picker / JS dialog) with no keyboard over it.
    Prompt,
    /// The full-screen menu (Tabs / Bookmarks / History / Downloads).
    Menu,
    /// The full-screen settings overlay (the on-screen keyboard can open over it
    /// to type into a text field, hence it ranks below `Osk`).
    Settings,
    /// Link-hint navigation.
    Hints,
    /// The standalone speed-dial editor (opened from the start page).
    DialEdit,
    /// The built-in start page overlay (active tab is on `retsurf:home`).
    Home,
    /// No overlay: input goes to the page or the toolbar.
    Page,
}

pub struct AppUi {
    egui: EguiGlow,
    repaint_delay: Option<Duration>,
    /// Y of the web view's top edge (logical px) = the real toolbar bottom,
    /// measured from the central panel each frame. Used to map cursor↔browser
    /// coordinates and to keep the cursor out of the toolbar.
    webview_top: f32,
    repaint_pending: bool,
    /// egui handle to Servo's FBO color texture (rendered directly by WebRender).
    browser_tex_id: egui::TextureId,
    /// Last browser viewport size (physical px) we requested, to avoid churn.
    browser_viewport: (u32, u32),
    /// Gamepad cursor position (logical px). The UI owns it — it draws the
    /// overlay — and the gamepad moves it via [`AppUi::move_cursor`].
    cursor: (f32, f32),
    /// When the cursor last moved, or `None` if it has never moved. Drives the
    /// auto-hide: the overlay shows only within `cursor_linger` of this.
    cursor_last_move: Option<Instant>,
    /// How long the cursor stays visible after a move (from the interface config).
    cursor_linger: Duration,
    /// On-screen keyboard: state, rendering, and input routing all live here.
    osk: Osk,
    /// The full-screen menu (Tabs / Bookmarks / History / Downloads) and its state.
    menu: Menu,
    /// The full-screen settings overlay (edits a draft of the config).
    settings: Settings,
    /// The built-in start page overlay's selection / search-field state.
    home: Home,
    /// The standalone speed-dial editor overlay (opened from the start page).
    dial_edit: DialEdit,
    /// Whether the active tab is on the start page (mirrored each frame from
    /// [`crate::browser::AppBrowser::on_home_page`]); drives [`Focus::Home`].
    home_active: bool,
    /// Link-hint navigation state (L3); the rects come from the browser.
    hints: Hints,
    /// Modal page prompts: queued `<select>` pickers and JS dialogs. Public —
    /// the router and main loop drive [`Prompt`]'s own methods directly.
    pub prompt: Prompt,
    /// The gamepad's latched D-pad scroll mode, mirrored each frame by the
    /// router; drawn as an autoscroll-style indicator in place of the cursor.
    scroll_mode: bool,
}

impl AppUi {
    pub fn new(
        window: &AppWindow,
        interface: &InterfaceConfig,
        history: &HistoryConfig,
        downloads: &DownloadsConfig,
        osk: &OskConfig,
    ) -> Self {
        let mut egui = EguiGlow::new(window.get_sdl2_window(), window.get_glow_ctx(), None, false);
        // Install the shared accent theme so every selectable widget, text
        // selection, and link picks up the brand green (see [`theme`]).
        theme::apply(&egui.ctx);
        // Register the FBO color texture once; its GL name is stable across
        // resizes, so this TextureId stays valid for the program's lifetime.
        let browser_tex_id = egui
            .painter
            .register_native_texture(window.rendering_color_texture());

        Self {
            egui,
            repaint_delay: None,
            webview_top: 0.0,
            repaint_pending: false,
            browser_tex_id,
            browser_viewport: (0, 0),
            cursor: {
                let (w, h) = window.size();
                (w as f32 / 2.0, h as f32 / 2.0)
            },
            cursor_last_move: None,
            cursor_linger: Duration::from_millis(interface.cursor_linger_ms),
            osk: Osk::new(osk),
            menu: Menu::new(history, downloads),
            settings: Settings::new(),
            home: Home::new(),
            dial_edit: DialEdit::new(),
            home_active: false,
            hints: Hints::new(),
            prompt: Prompt::new(),
            scroll_mode: false,
        }
    }

    #[inline]
    pub fn take_repain_delay(&mut self) -> Option<Duration> {
        self.repaint_delay.take()
    }

    /// Ask the main loop to render one more frame immediately, without blocking on
    /// input. Commands are drained *after* this frame's [`AppUi::update`] builds the
    /// egui output, so a command that changes UI state (open a menu, switch section,
    /// type on the keyboard) wouldn't show until the next input wakes the loop. This
    /// schedules that follow-up frame so the change appears at once.
    #[inline]
    pub fn request_repaint(&mut self) {
        self.repaint_delay = Some(Duration::ZERO);
    }

    /// Move the gamepad cursor by a logical-px delta and mark it visible. Clamped
    /// to the window (inset by the cursor's painted extent so the whole circle
    /// stays on screen); it may roam over the toolbar so its buttons are clickable.
    #[inline]
    pub fn move_cursor(&mut self, dx: f32, dy: f32, window: &AppWindow) {
        let (w, h) = window.size();
        self.cursor.0 = (self.cursor.0 + dx).clamp(CURSOR_EXTENT, w as f32 - CURSOR_EXTENT);
        self.cursor.1 = (self.cursor.1 + dy).clamp(CURSOR_EXTENT, h as f32 - CURSOR_EXTENT);
        self.cursor_last_move = Some(Instant::now());
    }

    /// Whether the cursor is over the web view (below the toolbar). Clicks there
    /// go to the page; clicks above go to the egui toolbar via [`AppUi::click_ui`].
    #[inline]
    pub fn cursor_over_browser(&self) -> bool {
        self.cursor.1 >= self.webview_top
    }

    /// Click the egui UI element under the cursor by feeding the backend a
    /// synthetic mouse button event (egui never sees the gamepad otherwise).
    /// `pressed` mirrors the A button's press/release so egui registers a click.
    pub fn click_ui(&mut self, pressed: bool, window: &AppWindow) {
        let ppp = self.egui.ctx.pixels_per_point();
        let (x, y) = ((self.cursor.0 * ppp) as i32, (self.cursor.1 * ppp) as i32);
        let win = window.get_sdl2_window();
        let window_id = win.id();
        let event = if pressed {
            sdl2::event::Event::MouseButtonDown {
                timestamp: 0,
                window_id,
                which: 0,
                mouse_btn: sdl2::mouse::MouseButton::Left,
                clicks: 1,
                x,
                y,
            }
        } else {
            sdl2::event::Event::MouseButtonUp {
                timestamp: 0,
                window_id,
                which: 0,
                mouse_btn: sdl2::mouse::MouseButton::Left,
                clicks: 1,
                x,
                y,
            }
        };
        let _ = self.egui.state.on_event(win, &event);
        self.repaint_pending = true;
    }

    /// Time left before the cursor auto-hides, or `None` if it's already hidden
    /// (never moved or idle past `cursor_linger`).
    #[inline]
    fn cursor_visible_for(&self) -> Option<Duration> {
        self.cursor_last_move
            .and_then(|t| self.cursor_linger.checked_sub(t.elapsed()))
    }

    /// The gamepad cursor in browser-relative coordinates (below the toolbar),
    /// ready to feed to Servo as a mouse position.
    #[inline]
    pub fn cursor_browser_rel(&self) -> (f32, f32) {
        self.to_browser_rel_pos(self.cursor.0, self.cursor.1)
    }

    /// The current input owner — see [`Focus`] for the precedence.
    #[inline]
    pub fn focus(&self) -> Focus {
        if self.osk.visible {
            Focus::Osk
        } else if self.prompt.visible() {
            Focus::Prompt
        } else if self.menu.visible {
            Focus::Menu
        } else if self.settings.visible() {
            Focus::Settings
        } else if self.hints.visible {
            Focus::Hints
        } else if self.dial_edit.visible() {
            Focus::DialEdit
        } else if self.home_active {
            Focus::Home
        } else {
            Focus::Page
        }
    }

    /// Apply an [`OskCommand`] to the on-screen keyboard, routing typed input
    /// to a modal `prompt()` dialog's field when one is up, else the address
    /// bar when it holds focus, otherwise the focused page element.
    pub fn osk(&mut self, cmd: OskCommand, browser: &AppBrowser, commands: &mut Vec<AppCommand>) {
        let to_address_bar = self.address_bar_focused();
        let target = if self.prompt.visible() && self.prompt.has_text_field() {
            OskTarget::Prompt(self.prompt.input_mut())
        } else if self.settings.visible() && self.settings.selected_is_text() {
            // The settings overlay's focused text row: typing lands in the draft
            // (the OSK only opens over a text row — see `App::settings_confirm`).
            OskTarget::Settings(self.settings.selected_text_mut().expect("text row"))
        } else if self.dial_edit.visible() {
            // The speed-dial editor's URL field (its own buffer); Enter pins it.
            OskTarget::DialEdit(self.dial_edit.input_mut())
        } else if self.home_active {
            // On the start page, typed text goes to its own search field, not
            // the address bar (which only ever shows `retsurf:home` there).
            OskTarget::Home(self.home.input_mut())
        } else if to_address_bar {
            OskTarget::AddressBar
        } else {
            OskTarget::Page
        };
        self.osk.handle(cmd, target, browser, commands);
    }

    /// Whether the full-screen menu is shown.
    #[inline]
    pub fn menu_visible(&self) -> bool {
        self.menu.visible
    }

    /// Open the menu. It takes over the stick and A, so the other user
    /// overlays close — input focus and draw order can never disagree.
    #[inline]
    pub fn menu_open(&mut self) {
        self.osk.visible = false;
        self.hints.hide();
        self.menu.open();
    }

    #[inline]
    pub fn menu_close(&mut self) {
        self.menu.close();
    }

    /// Whether the settings overlay is shown.
    #[inline]
    pub fn settings_visible(&self) -> bool {
        self.settings.visible()
    }

    /// Open the settings overlay, seeding its draft from the live config. Like
    /// the menu it takes over the stick and A, so the other user overlays close.
    #[inline]
    pub fn settings_open(&mut self, config: &AppConfig) {
        self.osk.visible = false;
        self.hints.hide();
        self.menu.close();
        self.settings.open(config);
    }

    /// Close the settings overlay, handing back its edited draft so the app can
    /// save it and re-apply what changes live.
    #[inline]
    pub fn settings_close(&mut self) -> AppConfig {
        let draft = self.settings.draft();
        self.settings.close();
        draft
    }

    /// Focus settings row `index` (clicking it).
    #[inline]
    pub fn settings_select(&mut self, index: usize) {
        self.settings.set_selected(index);
    }

    /// Switch the active settings section by `delta` (L1/R1 / Tab / Ctrl+◀▶).
    #[inline]
    pub fn settings_switch(&mut self, delta: i32) {
        self.settings.switch_section(delta);
    }

    /// Jump to a settings section (clicking its tab).
    #[inline]
    pub fn settings_set_section(&mut self, section: SettingsSection) {
        self.settings.set_section(section);
    }

    /// Move the settings selection by `dy` rows.
    #[inline]
    pub fn settings_move(&mut self, dy: i32) {
        self.settings.move_sel(dy);
    }

    /// Step the focused settings field by `dx` (◀ -1 / ▶ +1): toggle / cycle /
    /// nudge a number.
    #[inline]
    pub fn settings_adjust(&mut self, dx: i32) {
        self.settings.adjust(dx);
    }

    /// Whether the focused settings row is a text field (A opens the OSK on it
    /// rather than toggling/stepping).
    #[inline]
    pub fn settings_selected_is_text(&self) -> bool {
        self.settings.selected_is_text()
    }

    /// Set how long the gamepad cursor lingers after a move (the app calls this
    /// when the interface config changes live via the settings overlay).
    #[inline]
    pub fn set_cursor_linger(&mut self, ms: u64) {
        self.cursor_linger = Duration::from_millis(ms);
    }

    /// Switch the active section by `delta` (◀▶).
    #[inline]
    pub fn menu_switch(&mut self, delta: i32) {
        self.menu.switch_section(delta);
    }

    /// Jump to a specific section (clicking its tab).
    #[inline]
    pub fn menu_set_section(&mut self, section: Section) {
        self.menu.set_section(section);
    }

    /// The active menu section.
    #[inline]
    pub fn menu_section(&self) -> Section {
        self.menu.section()
    }

    /// Highlighted row in the Tabs section (== tab count means the "+ New tab" row).
    #[inline]
    pub fn menu_tab_selected(&self) -> usize {
        self.menu.tab_selected()
    }

    /// Whether the History section's "Clear all" top row is highlighted.
    #[inline]
    pub fn menu_history_clear_selected(&self) -> bool {
        self.menu.history_clear_selected()
    }

    /// Refresh the Tabs section's known tab count (keeps its selection in range).
    #[inline]
    pub fn menu_set_tab_count(&mut self, count: usize) {
        self.menu.set_tab_count(count);
    }

    /// Move the active section's selection by `dy` rows.
    #[inline]
    pub fn menu_move(&mut self, dy: i32) {
        self.menu.move_sel(dy);
    }

    /// The highlighted entry's URL in the active section, if any.
    #[inline]
    pub fn menu_selected_url(&self) -> Option<String> {
        self.menu.selected_url()
    }

    /// Remove the highlighted entry in the active section.
    #[inline]
    pub fn menu_remove_selected(&mut self) {
        self.menu.remove_selected();
    }

    /// Remove the entry at `index` in the active section (clicking its ✖ button).
    #[inline]
    pub fn menu_remove_at(&mut self, index: usize) {
        self.menu.remove_at(index);
    }

    /// Clear all entries in the active section (History's "Clear all").
    #[inline]
    pub fn menu_clear(&mut self) {
        self.menu.clear();
    }

    /// Record a visited URL in history (no-op if history is disabled).
    #[inline]
    pub fn menu_record_history(&mut self, url: &str) {
        self.menu.record_history(url);
    }

    /// Pull progress from download worker threads into the menu's Downloads list
    /// (records finishes; cheap when nothing is downloading).
    #[inline]
    pub fn downloads_poll(&mut self) {
        self.menu.downloads.poll();
    }

    /// Start downloading `url` in the background (see [`crate::data::downloads`]).
    #[inline]
    pub fn start_download(&mut self, url: &str, sender: &UserEventSender) {
        self.menu.downloads.start(url, sender);
    }

    /// Add or remove `url` from the saved bookmarks (★ button / Start).
    #[inline]
    pub fn toggle_bookmark(&mut self, url: &str) {
        self.menu.toggle_bookmark(url);
    }

    /// Mirror whether the active tab is on the start page (called each frame
    /// from the main loop). Resets the overlay's state on entry so it always
    /// opens focused on an empty search field.
    #[inline]
    pub fn set_home_active(&mut self, active: bool) {
        if active && !self.home_active {
            self.home.reset();
        }
        self.home_active = active;
    }

    /// Focus the start page's search field (when the OSK opens to type).
    #[inline]
    pub fn home_focus_search(&mut self) {
        self.home.focus_search();
    }

    /// The start-page search field's current text (for submitting it from the
    /// keyboard's Enter).
    #[inline]
    pub fn home_search_text(&self) -> String {
        self.home.input().to_string()
    }

    /// Move the start-page selection by one dominant-axis step. The grid holds
    /// one tile per pin plus a trailing "+ Add" tile, hence `len() + 1`.
    #[inline]
    pub fn home_move(&mut self, dx: i32, dy: i32) {
        let count = self.menu.dial.urls().len() + 1;
        self.home.move_sel(dx, dy, count);
    }

    /// The focused tile's pinned URL, if a *pin* tile is selected (the trailing
    /// "Edit" tile has no URL — see [`Self::home_tile_is_edit`]).
    #[inline]
    pub fn home_selected_url(&self) -> Option<String> {
        self.home
            .tile()
            .and_then(|i| self.menu.dial.urls().get(i).cloned())
    }

    /// Whether the trailing "Edit" tile (index == pin count) is focused.
    #[inline]
    pub fn home_tile_is_edit(&self) -> bool {
        self.home.tile() == Some(self.menu.dial.urls().len())
    }

    /// Pin `url` to the speed dial if absent (the editor's Add); unlike
    /// [`Self::dial_toggle`] it never unpins.
    #[inline]
    pub fn dial_pin(&mut self, url: &str) {
        self.menu.dial.pin(url);
    }

    // --- Speed-dial editor (the standalone overlay opened from the start page) ---

    /// Open the speed-dial editor overlay.
    #[inline]
    pub fn open_pins_editor(&mut self) {
        self.dial_edit.open();
    }

    /// Close the speed-dial editor (back to the start page).
    #[inline]
    pub fn close_pins_editor(&mut self) {
        self.dial_edit.close();
    }

    /// The editor's focused item (drives the **A** action in the router).
    #[inline]
    pub fn dial_edit_item(&self) -> EditItem {
        self.dial_edit.item()
    }

    /// Focus the editor's URL field (e.g. before opening the OSK to type).
    #[inline]
    pub fn dial_edit_focus_field(&mut self) {
        self.dial_edit.focus_field();
    }

    /// The editor's URL field text (trimmed submission lives in the app).
    #[inline]
    pub fn dial_edit_input(&self) -> String {
        self.dial_edit.input().to_string()
    }

    /// Clear the editor's URL field (after pinning its contents).
    #[inline]
    pub fn dial_edit_clear_input(&mut self) {
        self.dial_edit.clear_input();
    }

    /// Move the editor's selection by one dominant-axis step.
    #[inline]
    pub fn dial_edit_move(&mut self, dx: i32, dy: i32) {
        self.dial_edit.move_sel(dx, dy, self.dial_edit_slots());
    }

    /// The editor's focused pin index, if a tile (not the field) is focused.
    #[inline]
    pub fn dial_edit_tile(&self) -> Option<usize> {
        self.dial_edit.tile()
    }

    /// Dial indices of the editor's regular (non-settings) pin tiles, in order.
    /// The editor hides the ⚙ settings sentinel from the normal pins and shows
    /// it as a dedicated trailing toggle tile, so its grid slots are these pins
    /// followed by that one tile.
    fn dial_edit_pin_indices(&self) -> Vec<usize> {
        self.menu
            .dial
            .urls()
            .iter()
            .enumerate()
            .filter(|(_, u)| u.as_str() != crate::data::dial::SETTINGS_PIN)
            .map(|(i, _)| i)
            .collect()
    }

    /// Number of editor grid slots: the regular pins plus the always-present ⚙
    /// settings toggle tile at the end.
    #[inline]
    fn dial_edit_slots(&self) -> usize {
        self.dial_edit_pin_indices().len() + 1
    }

    /// Whether the focused grid tile is the trailing ⚙ settings toggle (its slot
    /// is the one past the regular pins) — drives the **A** action in the editor.
    pub fn dial_edit_settings_selected(&self) -> bool {
        self.dial_edit.tile() == Some(self.dial_edit_pin_indices().len())
    }

    /// Remove the pin at `index` from the speed dial (editor ✖ click, which
    /// carries the real dial index).
    #[inline]
    pub fn dial_remove_at(&mut self, index: usize) {
        self.menu.dial.remove(index);
    }

    /// Delete the editor's focused tile (gamepad/keyboard X): a regular pin is
    /// removed by its mapped dial index; the ⚙ settings toggle tile is left
    /// alone (it pins/unpins with A, not delete).
    pub fn dial_edit_remove_selected(&mut self) {
        if let Some(slot) = self.dial_edit.tile() {
            let indices = self.dial_edit_pin_indices();
            if let Some(&dial_index) = indices.get(slot) {
                self.menu.dial.remove(dial_index);
            }
        }
    }

    /// Whether a start-page tile (not the search field) is focused.
    #[inline]
    pub fn home_tile_selected(&self) -> bool {
        self.home.tile().is_some()
    }

    /// Pin `url` to the speed dial, or unpin it if already pinned (Y on a menu
    /// Bookmarks / History row, or on a focused start-page tile).
    #[inline]
    pub fn dial_toggle(&mut self, url: &str) {
        self.menu.dial.toggle(url);
    }

    /// Whether link-hint navigation is currently shown.
    #[inline]
    pub fn hints_visible(&self) -> bool {
        self.hints.visible
    }

    /// L3 pressed: a hint-collection round was started in the browser.
    #[inline]
    pub fn hints_begin_collect(&mut self) {
        self.hints.begin_collect();
    }

    /// Fresh clickable rects from the page. Selection lands near the previous
    /// one (a post-scroll refresh) or near the gamepad cursor (mode entry).
    pub fn hints_apply(&mut self, rects: Vec<Hint>) {
        let near = self
            .hints
            .selected_center()
            .unwrap_or_else(|| self.cursor_browser_rel());
        self.hints.show(rects, near);
    }

    #[inline]
    pub fn hints_hide(&mut self) {
        self.hints.hide();
    }

    /// Hop the hint selection in `dir` (a dominant-axis step from the router).
    #[inline]
    pub fn hints_move(&mut self, dir: (i32, i32)) {
        self.hints.move_sel(dir);
    }

    /// Center of the selected hint in browser-relative coordinates.
    #[inline]
    pub fn hints_selected_center(&self) -> Option<(f32, f32)> {
        self.hints.selected_center()
    }

    /// The selected hint's link URL (owned), if it is a link.
    #[inline]
    pub fn hints_selected_url(&self) -> Option<String> {
        self.hints.selected_url().map(str::to_owned)
    }

    /// The page scrolled under the badges: schedule a re-collect.
    #[inline]
    pub fn hints_mark_stale(&mut self) {
        self.hints.mark_stale();
    }

    /// Whether the post-scroll re-collect is due (cleared on read).
    #[inline]
    pub fn hints_refresh_due(&mut self) -> bool {
        self.hints.take_refresh_due()
    }

    /// Mirror the gamepad's latched D-pad scroll mode (called by the router
    /// every analog frame) so the indicator tracks it.
    #[inline]
    pub fn set_scroll_mode(&mut self, on: bool) {
        self.scroll_mode = on;
    }

    /// Whether the address-bar text field currently holds keyboard focus (also
    /// guards plain-key keyboard shortcuts in the event handler).
    pub fn address_bar_focused(&self) -> bool {
        self.egui
            .ctx
            .memory(|m| m.has_focus(egui::Id::new("location")))
    }

    /// Which egui text field the OSK is currently typing into — mirrors the
    /// target priority in [`AppUi::osk`], collapsing the no-egui-caret cases
    /// (Page, settings rows) to `None`. Used to park that field's caret at the
    /// buffer end while the OSK is up (see [`OskField`]).
    fn osk_target_field(&self) -> OskField {
        if !self.osk.visible {
            OskField::None
        } else if self.prompt.visible() && self.prompt.has_text_field() {
            OskField::Prompt
        } else if self.dial_edit.visible() {
            OskField::DialEdit
        } else if self.home_active {
            OskField::Home
        } else if self.address_bar_focused() {
            OskField::AddressBar
        } else {
            OskField::None
        }
    }

    /// Whether the start page's search field holds egui keyboard focus (a desktop
    /// click into it). While it does, arrow keys edit text rather than moving the
    /// start-page selection, and plain-key shortcuts are muted.
    pub fn home_field_editing(&self) -> bool {
        self.egui
            .ctx
            .memory(|m| m.has_focus(egui::Id::new("home_search")))
    }

    /// Whether the speed-dial editor's URL field holds egui keyboard focus —
    /// like [`Self::home_field_editing`], but for the editor's `dial_edit_url`
    /// field.
    pub fn dial_edit_field_editing(&self) -> bool {
        self.egui
            .ctx
            .memory(|m| m.has_focus(egui::Id::new("dial_edit_url")))
    }

    #[inline]
    pub fn to_browser_rel_pos(&self, x: f32, y: f32) -> (f32, f32) {
        (x, y - self.webview_top)
    }

    /// Resize the browser to the central web-view area (the window minus the
    /// toolbar) for the current window size. Driven by SDL window-resize events:
    /// egui's reactive sizing reads the central rect a frame later, so it can lag
    /// behind the actual window. Uses the toolbar height measured during `update`,
    /// and shares `browser_viewport` with that path so the two never double-resize.
    pub fn resize_browser(&mut self, window: &AppWindow, browser: &AppBrowser) {
        let (dw, dh) = window.drawable_size();
        if dw == 0 || dh == 0 {
            return;
        }
        let toolbar_px = (self.webview_top * self.egui.ctx.pixels_per_point()).round() as u32;
        let size = (dw, dh.saturating_sub(toolbar_px).max(1));
        if size != self.browser_viewport {
            self.browser_viewport = size;
            browser.resize(size.0, size.1);
        }
    }

    /// Handles the event and returns whether it is consumed
    pub fn handle_event(&mut self, window: &AppWindow, event: &sdl2::event::Event) -> bool {
        let resp = self.egui.state.on_event(window.get_sdl2_window(), event);
        self.repaint_pending = resp.repaint;
        // don't consume when pointer over browser area
        resp.consumed & self.is_pointer_over_toolbar()
    }

    pub fn update(&mut self, browser: &mut AppBrowser, commands: &mut Vec<AppCommand>) {
        let mut desired_px: Option<(u32, u32)> = None;

        // The cursor draws only while it lingers after a move. When it does, ask
        // the loop to wake when the linger ends so it gets erased even if no other
        // event arrives; otherwise leave the idle wait untouched.
        let cursor_visible = if self.focus() == Focus::Page {
            self.cursor_visible_for()
        } else {
            None
        };
        self.repaint_delay = cursor_visible;
        // A pending post-scroll hint refresh also needs the loop to wake by
        // itself — without this the wait blocks on input and it never fires.
        if let Some(refresh) = self.hints.refresh_in() {
            self.repaint_delay = Some(self.repaint_delay.map_or(refresh, |d| d.min(refresh)));
        }

        // Read tab info *before* borrowing the active tab's state below — both read
        // the browser's tab list, so they can't overlap. `tab_pos` is the 1-based
        // active index and count, shown in the toolbar; `tab_infos` feeds the menu.
        let tab_pos = (browser.active_tab() + 1, browser.tab_count());
        let zoom_pct = browser.zoom_chip();
        let tab_infos = if self.menu.visible {
            self.menu.set_tab_count(browser.tab_count());
            browser.tabs()
        } else {
            Vec::new()
        };
        // Snapshot the pinned speed-dial list for the start page / editor (kept
        // in sync with the menu's live store), and keep their selections in range.
        let pin_count = self.menu.dial.urls().len();
        let pins = if self.home_active || self.dial_edit.visible() {
            if self.home_active {
                // +1 for the trailing "Edit" tile, so its selection isn't clamped off.
                self.home.clamp(pin_count + 1);
            }
            if self.dial_edit.visible() {
                // The editor's grid is its non-settings pins plus the trailing
                // ⚙ toggle tile, so a selection up to that tile stays valid.
                let slots = self.dial_edit_slots();
                self.dial_edit.clamp(slots);
            }
            self.menu.dial.urls().to_vec()
        } else {
            Vec::new()
        };

        {
            let mut state = browser.get_state_mut();
            // Which field (if any) the OSK types into — computed here, before the
            // closure borrows `self.egui` via `run`, since the lookup borrows all
            // of `self` (the closure itself only captures disjoint fields).
            let osk_field = self.osk_target_field();
            self.egui.run(|ctx| {
                let ppp = ctx.pixels_per_point();
                let mut root = egui::Ui::new(
                    ctx.clone(),
                    egui::Id::new("root_ui"),
                    egui::UiBuilder::new().max_rect(ctx.content_rect()),
                );
                root.set_clip_rect(ctx.content_rect());

                let bookmarked = self.menu.is_bookmarked(state.get_location());
                let active_downloads = self.menu.downloads.active_count();
                toolbar::add_toolbar(
                    &mut root,
                    &mut state,
                    commands,
                    bookmarked,
                    tab_pos,
                    active_downloads,
                    zoom_pct,
                    osk_field == OskField::AddressBar,
                );

                let frame = egui::Frame::default().inner_margin(0.0);
                egui::CentralPanel::default()
                    .frame(frame)
                    .show_inside(&mut root, |ui| {
                        let rect = ui.max_rect();
                        // The panel's top edge is the real toolbar bottom (incl.
                        // frame margins), so map cursor/clicks against it.
                        self.webview_top = rect.min.y;
                        ui.allocate_rect(rect, egui::Sense::hover());

                        desired_px = Some((
                            (rect.width() * ppp).round().max(1.0) as u32,
                            (rect.height() * ppp).round().max(1.0) as u32,
                        ));

                        // WebRender renders bottom-up into the FBO, so flip V.
                        let uv =
                            egui::Rect::from_min_max(egui::pos2(0.0, 1.0), egui::pos2(1.0, 0.0));
                        ui.painter()
                            .image(self.browser_tex_id, rect, uv, egui::Color32::WHITE);
                    });

                // The start-page overlay is a backdrop over the (blank) web
                // view — drawn below the foreground overlays (menu / OSK / etc.)
                // so they can still open on top of it. The dial editor (also
                // reached from the start page) fully covers it, so skip the start
                // page underneath while the editor is up.
                if self.home_active && !self.dial_edit.visible() {
                    let home_caret = osk_field == OskField::Home;
                    home::add_home(
                        ctx,
                        &mut self.home,
                        &pins,
                        self.webview_top,
                        home_caret,
                        commands,
                    );
                }

                // The speed-dial editor: a full-screen overlay above the start
                // page; the OSK (below) can still open on top to type a URL.
                if self.dial_edit.visible() {
                    let dial_caret = osk_field == OskField::DialEdit;
                    dial_edit::add_dial_edit(ctx, &mut self.dial_edit, &pins, dial_caret, commands);
                }

                // The settings overlay: a full-screen panel like the menu. Drawn
                // before the menu/OSK chain below so the OSK can open on top to
                // type into a text field (focus is `Osk` then, painting it here
                // would put it under the keyboard).
                if self.settings.visible() {
                    settings::add_settings(ctx, &self.settings, commands);
                }

                // The modal prompt draws on top of whatever else is up (its
                // egui layer order puts it above the other overlays).
                if self.prompt.visible() {
                    let prompt_caret = osk_field == OskField::Prompt;
                    prompt::add_prompt(ctx, &mut self.prompt, prompt_caret, commands);
                }

                if self.menu.visible {
                    menu::add_menu(ctx, &self.menu, &tab_infos, commands);
                } else if self.osk.visible {
                    osk::add_osk(ctx, &self.osk);
                } else if self.hints.visible {
                    hints::add_hints(ctx, &self.hints, self.webview_top);
                } else if self.scroll_mode || cursor_visible.is_some() {
                    // Gamepad cursor overlay, always on top. `cursor` is in logical
                    // px which equals egui points at the handheld's 1.0 scale factor.
                    let painter = ctx.layer_painter(egui::LayerId::new(
                        egui::Order::Foreground,
                        egui::Id::new("gamepad_cursor"),
                    ));
                    let pos = egui::pos2(self.cursor.0, self.cursor.1);
                    if self.scroll_mode {
                        // Autoscroll-style indicator (dot + up/down arrowheads),
                        // persistent while the mode is latched so it's never
                        // ambiguous what the D-pad currently does.
                        add_scroll_indicator(&painter, pos);
                    } else {
                        painter.circle_filled(
                            pos,
                            CURSOR_RADIUS,
                            egui::Color32::from_white_alpha(235),
                        );
                        painter.circle_stroke(
                            pos,
                            CURSOR_RADIUS,
                            egui::Stroke::new(CURSOR_STROKE, egui::Color32::BLACK),
                        );
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

        // Fold in egui's own repaint timing. A freshly shown anchored `Area`
        // (the menu / OSK) sizes itself invisibly on its first frame and asks
        // egui for an immediate follow-up to paint it positioned — without this
        // the loop blocks on input and the overlay only appears after the next
        // keypress. `MAX` means egui is idle, so it never shortens our wait.
        let egui_delay = self.egui.repaint_delay();
        if egui_delay < Duration::MAX {
            self.repaint_delay = Some(self.repaint_delay.map_or(egui_delay, |d| d.min(egui_delay)));
        }
    }

    /// Paints the UI (toolbar + browser texture) and presents to the window.
    pub fn draw(&mut self, window: &AppWindow) {
        // Servo's software context made its own GL context current while rendering;
        // restore SDL2's context before egui issues any GL calls.
        window.make_current();
        window.bind_default_framebuffer();
        self.egui.paint();
        window.present();
        self.repaint_pending = false;
    }

    pub fn destroy(&mut self) {
        self.egui.destroy();
    }

    #[inline]
    fn is_pointer_over_toolbar(&self) -> bool {
        let Some(pos) = self.egui.state.get_pointer_pos_in_points() else {
            return false;
        };

        pos.y < self.webview_top
    }
}

/// The D-pad scroll-mode indicator at the parked cursor position: a center dot
/// with up/down arrowheads, like a browser's middle-click autoscroll marker.
fn add_scroll_indicator(painter: &egui::Painter, pos: egui::Pos2) {
    let fill = egui::Color32::from_white_alpha(235);
    let stroke = egui::Stroke::new(CURSOR_STROKE, egui::Color32::BLACK);
    painter.circle_filled(pos, 2.5, fill);
    painter.circle_stroke(pos, 2.5, stroke);
    for dir in [-1.0f32, 1.0] {
        let tip = egui::pos2(pos.x, pos.y + dir * 12.0);
        let base = pos.y + dir * 5.5;
        let points = vec![
            tip,
            egui::pos2(pos.x - 4.5, base),
            egui::pos2(pos.x + 4.5, base),
        ];
        painter.add(egui::Shape::convex_polygon(points, fill, stroke));
    }
}
