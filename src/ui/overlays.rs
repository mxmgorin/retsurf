//! [`AppUi`]'s overlay coordinators: the [`Focus`] precedence the router matches
//! on, OSK input routing, and the per-overlay passthroughs (menu, settings,
//! start page, speed-dial editor, hints) the router and app drive.

use super::{dial_edit, settings, AppUi, OskField};
use crate::{
    app::{AppCommand, SettingsAction},
    browser::AppBrowser,
    config::AppConfig,
    event::user::UserEventSender,
    overlay::dial_edit::EditItem,
    overlay::hints::{Hint, HintInput, HintLabels, Label, Sym},
    overlay::osk::{OskCommand, OskTarget},
};
use egui_sdl2::egui;

/// Which surface owns contextual input (Confirm, Cancel, overlay `Nav`): one
/// precedence order derived from the visibility flags, so routing never
/// re-combines `*_visible()` checks. The OSK outranks the modal prompt (a
/// gamepad types into one through it); menu / keyboard / hints never coexist.
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

impl AppUi {
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
        let to_page = matches!(target, OskTarget::Page);
        self.osk.handle(cmd, target, browser, commands);
        // Only the page can scroll a covered field out; the lift is applied once
        // the keyboard's height is known (see `update`).
        if to_page && matches!(cmd, OskCommand::Show) {
            self.osk_lift_pending = true;
        }
    }

    /// The egui text field the OSK types into — the target priority of
    /// [`AppUi::osk`], with the no-egui-caret cases (Page, settings rows)
    /// collapsed to `None`. Parks that field's caret (see [`OskField`]).
    pub(super) fn osk_target_field(&self) -> OskField {
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

    /// Open the menu. It takes over the stick and A, so the other user
    /// overlays close — input focus and draw order can never disagree.
    #[inline]
    pub fn menu_open(&mut self) {
        self.osk.visible = false;
        self.hints.hide();
        self.menu.open();
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

    /// Close the settings overlay, handing back its edited config and bindings
    /// drafts so the app can save them and re-apply what changes live.
    #[inline]
    pub fn settings_close(&mut self) -> (AppConfig, Option<inputbind::Store>) {
        let drafts = (self.settings.draft(), self.settings.changed_bindings());
        self.settings.close();
        drafts
    }

    /// Move the settings selection by `dy` rows. On the About tab the update block's
    /// row count depends on the live update state (a release-notes link appears when
    /// an update is available), so it's resolved here and handed to the nav.
    #[inline]
    pub fn settings_move(&mut self, dy: i32) {
        let update_rows = settings::update_row_count(&self.update.snapshot());
        self.settings.move_sel(dy, update_rows);
    }

    /// A on the focused About-tab row: in the update block, the primary action
    /// (row 0) or the release-notes link (row 1); past it, the static link at
    /// `sel - update_rows`. `None` when the row has nothing to do (work running).
    pub fn about_activate(&self) -> Option<SettingsAction> {
        let sel = self.settings.selected();
        let state = self.update.snapshot();
        let update_rows = settings::update_row_count(&state);
        if sel < update_rows {
            if sel == 0 {
                settings::update_command(&state)
            } else {
                settings::release_link(&state).map(SettingsAction::OpenLink)
            }
        } else {
            crate::overlay::settings::about_info()
                .links
                .get(sel - update_rows)
                .map(|(_, url)| SettingsAction::OpenLink(url.to_string()))
        }
    }

    /// Whether hint mode draws combo badges (the app calls this on a live config
    /// change). When off, hint mode is plain spatial hopping.
    #[inline]
    pub fn set_hint_badges(&mut self, on: bool) {
        self.hint_badges = on;
    }

    /// Whether hint-mode combo badges are enabled (the router gates typed input
    /// on the same flag).
    #[inline]
    pub fn hint_badges(&self) -> bool {
        self.hint_badges
    }

    /// The event handler notes which device produced the latest input, so hint
    /// mode can pick its badge alphabet (typed letters vs gamepad combos) when it
    /// opens. `true` = keyboard.
    #[inline]
    pub fn note_input_keyboard(&mut self, keyboard: bool) {
        self.last_input_keyboard = keyboard;
    }

    /// Kick off a self-update check in the background (About tab; a no-op off a
    /// PortMaster install). See [`crate::update`].
    #[inline]
    pub fn update_check(&self, sender: &UserEventSender) {
        self.update.check(sender);
    }

    /// Startup: run a throttled background update check if `[update] auto_check` is
    /// on and one is due (see [`crate::update::Updater::auto_check`]).
    #[inline]
    pub fn update_auto_check(&self, sender: &UserEventSender) {
        self.update.auto_check(sender);
    }

    /// Download + install the available update in the background (About tab).
    #[inline]
    pub fn update_install(&self, sender: &UserEventSender) {
        self.update.install(sender);
    }

    /// Adopt edited `[update]` settings live (settings overlay), for the next check.
    #[inline]
    pub fn set_update_config(&mut self, cfg: &crate::config::UpdateConfig) {
        self.update.set_config(cfg);
    }

    /// Mirror whether the active tab is on the start page (each frame); entry
    /// resets the overlay to an empty search field. Returns whether it changed:
    /// activation comes from an async navigation, and without a follow-up
    /// repaint the idle loop sizes the fresh overlay invisibly and never paints it.
    #[inline]
    pub fn set_home_active(&mut self, active: bool) -> bool {
        let changed = active != self.home_active;
        if active && !self.home_active {
            self.home.reset();
        }
        self.home_active = active;
        changed
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

    /// Whether a start-page tile (not the search field) is focused.
    #[inline]
    pub fn home_tile_selected(&self) -> bool {
        self.home.tile().is_some()
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

    fn dial_settings_pinned(&self) -> bool {
        self.menu.dial.contains(crate::data::dial::SETTINGS_PIN)
    }

    #[inline]
    pub(super) fn dial_edit_slots(&self) -> usize {
        dial_edit::slot_count(self.menu.dial.urls())
    }

    /// Whether the trailing "Pin settings" slot is focused — drives **A** in the
    /// editor.
    pub fn dial_edit_pin_settings_selected(&self) -> bool {
        !self.dial_settings_pinned() && self.dial_edit.tile() == Some(self.menu.dial.urls().len())
    }

    /// Delete the editor's focused pin (X). The trailing tile's slot is out of
    /// range, so it's a no-op there.
    pub fn dial_edit_remove_selected(&mut self) {
        if let Some(slot) = self.dial_edit.tile() {
            self.menu.dial.remove(slot);
        }
    }

    /// Move the editor's focused pin by `delta` slots (L1/R1), taking the
    /// selection with it.
    pub fn dial_edit_move_selected(&mut self, delta: i32) {
        let Some(slot) = self.dial_edit.tile() else {
            return;
        };
        let pins = self.menu.dial.urls().len();
        let target = slot as i32 + delta;
        if slot >= pins || target < 0 || target as usize >= pins {
            return;
        }
        let target = target as usize;
        self.menu.dial.swap(slot, target);
        self.dial_edit.select_tile(target);
    }

    /// Hint mode opened: a collection round was started in the browser. The badge
    /// alphabet follows the device that triggered it (typed letters from the
    /// keyboard, button combos from the pad) — fixed for the whole session.
    #[inline]
    pub fn hints_begin_collect(&mut self) {
        let labels = if self.last_input_keyboard {
            HintLabels::Keyboard
        } else {
            HintLabels::Gamepad
        };
        self.hints.begin_collect(labels);
    }

    /// Fresh clickable rects from the page. Selection lands near the previous
    /// one (a post-scroll refresh) or near the gamepad cursor (mode entry).
    pub fn hints_apply(&mut self, rects: Vec<Hint>) {
        let near = self
            .hints
            .selected_center()
            .unwrap_or_else(|| self.cursor_browser_rel());
        // Combo codes are ordered from the viewport top-center (browser-relative),
        // so the nearest-to-top hints get the shortest codes.
        let top_center = (self.webview_rect.width() / 2.0, 0.0);
        self.hints.show(rects, near, top_center);
    }

    /// Feed one gamepad combo symbol into hint mode (see [`HintInput`]).
    #[inline]
    pub fn hints_push_sym(&mut self, s: Sym) -> HintInput {
        self.hints.push_label(Label::Sym(s))
    }

    /// Feed one typed keyboard letter into hint mode (see [`HintInput`]).
    #[inline]
    pub fn hints_push_key(&mut self, c: char) -> HintInput {
        self.hints.push_label(Label::Key(c))
    }

    /// Whether the address-bar text field currently holds keyboard focus (also
    /// guards plain-key keyboard shortcuts in the event handler).
    pub fn address_bar_focused(&self) -> bool {
        self.egui_ctx
            .memory(|m| m.has_focus(egui::Id::new("location")))
    }

    /// Whether the start page's search field holds egui keyboard focus (a desktop
    /// click into it). While it does, arrow keys edit text rather than moving the
    /// start-page selection, and plain-key shortcuts are muted.
    pub fn home_field_editing(&self) -> bool {
        self.egui_ctx
            .memory(|m| m.has_focus(egui::Id::new("home_search")))
    }

    /// Whether the speed-dial editor's URL field holds egui keyboard focus —
    /// [`Self::home_field_editing`] for the editor's `dial_edit_url` field.
    pub fn dial_edit_field_editing(&self) -> bool {
        self.egui_ctx
            .memory(|m| m.has_focus(egui::Id::new("dial_edit_url")))
    }
}
