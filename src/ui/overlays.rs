//! [`AppUi`]'s overlay coordinators: the [`Focus`] precedence the router matches
//! on, OSK input routing, and the per-overlay passthroughs (menu, settings,
//! start page, speed-dial editor, hints) the router and app drive.

use super::{dial_edit, home, settings, AppUi, OskField};
use crate::{
    browser::AppBrowser,
    command::{AppCommand, SettingsAction},
    config::AppConfig,
    overlay::hints::{Hint, HintInput, HintLabels, Label, Sym},
    overlay::osk::{OskCommand, OskTarget},
};
use egui_sdl2::egui;

/// Which surface owns contextual input (Confirm, Cancel, overlay `Nav`): one
/// precedence order over the visibility flags, so routing never re-combines
/// `*_visible()` checks. The OSK outranks the modal prompt, being typed through.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Focus {
    /// The on-screen keyboard — above everything, including the modal prompt.
    Osk,
    /// A modal page prompt (select picker / JS dialog) with no keyboard over it.
    Prompt,
    /// The full-screen menu (Tabs / Bookmarks / History / Downloads).
    Menu,
    /// Game Mode's own menu, over the still-running game.
    GameMenu,
    /// Its map list and one map's rows, opened from that menu.
    GameInputMaps,
    /// Its map editor, opened from a map.
    GameMapEdit,
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

impl Focus {
    /// One of Game Mode's own screens: the menu, the map list, the map editor.
    /// Exhaustive so a new variant must place itself.
    pub fn is_game_screen(self) -> bool {
        match self {
            Focus::GameMenu | Focus::GameInputMaps | Focus::GameMapEdit => true,
            Focus::Osk
            | Focus::Prompt
            | Focus::Menu
            | Focus::Settings
            | Focus::Hints
            | Focus::DialEdit
            | Focus::Home
            | Focus::Page => false,
        }
    }

    /// Overlays that own the device outright, so a browser-level shortcut — tab
    /// switching, reload, back/forward — must not fire underneath them.
    pub fn takes_over(self) -> bool {
        match self {
            Focus::Settings | Focus::GameMenu | Focus::GameInputMaps | Focus::GameMapEdit => true,
            Focus::Osk
            | Focus::Prompt
            | Focus::Menu
            | Focus::Hints
            | Focus::DialEdit
            | Focus::Home
            | Focus::Page => false,
        }
    }
}

/// Where typed input lands while the keyboard is up — one ladder (see
/// [`AppUi::osk_destination`]), so the routing and the caret parking that both
/// hang off it cannot drift.
#[derive(Clone, Copy, PartialEq, Eq)]
enum OskDest {
    Prompt,
    Settings,
    Capture,
    GameName,
    DialEdit,
    Home,
    AddressBar,
    Page,
}

impl AppUi {
    /// The current input owner — see [`Focus`] for the precedence.
    #[inline]
    pub fn focus(&self) -> Focus {
        if self.osk.visible {
            Focus::Osk
        } else {
            self.focus_below_osk()
        }
    }

    /// The precedence below the keyboard — what the OSK would type into, and
    /// what [`Self::focus`] returns once it is down.
    fn focus_below_osk(&self) -> Focus {
        if self.prompt.visible() {
            Focus::Prompt
        } else if self.menu.visible {
            Focus::Menu
        } else if self.game_menu.visible {
            Focus::GameMenu
        } else if self.input_maps.visible() {
            Focus::GameInputMaps
        } else if self.map_edit.visible() {
            Focus::GameMapEdit
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

    /// Where the keyboard types, derived from the focus underneath it. The
    /// sub-branches are what the plain focus cannot say: a prompt without a
    /// text field, a settings row that is not text, a picker or a rename.
    fn osk_destination(&self) -> OskDest {
        match self.focus_below_osk() {
            Focus::Prompt if self.prompt.has_text_field() => OskDest::Prompt,
            // The map editor turned the keyboard into a key picker.
            Focus::GameMapEdit if self.map_edit.picking().is_some() => OskDest::Capture,
            // A map being renamed or copied: the keyboard types its name.
            Focus::GameInputMaps if self.input_maps.naming().is_some() => OskDest::GameName,
            Focus::Settings if self.settings.selected_is_text() => OskDest::Settings,
            // The speed-dial editor's URL field (its own buffer); Enter pins it.
            Focus::DialEdit => OskDest::DialEdit,
            // On the start page, typed text goes to its own search field, not
            // the address bar (which only ever shows `retsurf:home` there).
            Focus::Home => OskDest::Home,
            _ if self.address_bar_focused() => OskDest::AddressBar,
            _ => OskDest::Page,
        }
    }

    /// Apply an [`OskCommand`] to the on-screen keyboard, routing typed input
    /// by [`Self::osk_destination`].
    pub fn osk(&mut self, cmd: OskCommand, browser: &AppBrowser, commands: &mut Vec<AppCommand>) {
        self.osk.set_picking(self.map_edit.picking().is_some());
        let target = match self.osk_destination() {
            OskDest::Prompt => OskTarget::Prompt(self.prompt.input_mut()),
            // Typing lands in the draft (the OSK only opens over a text row —
            // see `App::settings_confirm`).
            OskDest::Settings => {
                OskTarget::Settings(self.settings.selected_text_mut().expect("text row"))
            }
            OskDest::Capture => OskTarget::Capture(self.map_edit.picked_mut()),
            OskDest::GameName => {
                OskTarget::GameName(self.input_maps.naming_text_mut().expect("naming"))
            }
            OskDest::DialEdit => OskTarget::DialEdit(self.dial_edit.input_mut()),
            OskDest::Home => OskTarget::Home(self.home.input_mut()),
            OskDest::AddressBar => OskTarget::AddressBar,
            OskDest::Page => OskTarget::Page,
        };
        let to_page = matches!(target, OskTarget::Page);
        self.osk.handle(cmd, target, browser, commands);
        // Only the page can scroll a covered field out; the lift is applied once
        // the keyboard's height is known (see `update`).
        if to_page && matches!(cmd, OskCommand::Show) {
            self.osk_lift_pending = true;
        }
        // A map's name is committed by Enter, which writes a file; putting
        // the keyboard away is how that is called off.
        if matches!(cmd, OskCommand::Hide) {
            self.input_maps.take_naming();
        }
    }

    /// The egui text field the OSK types into — [`Self::osk_destination`] with
    /// the no-egui-caret cases collapsed to `None` (the page, a settings row's
    /// painted text, the picker and the rename the OSK draws itself).
    pub(super) fn osk_target_field(&self) -> OskField {
        if !self.osk.visible {
            return OskField::None;
        }
        match self.osk_destination() {
            OskDest::Prompt => OskField::Prompt,
            OskDest::DialEdit => OskField::DialEdit,
            OskDest::Home => OskField::Home,
            OskDest::AddressBar => OskField::AddressBar,
            OskDest::Settings | OskDest::Capture | OskDest::GameName | OskDest::Page => {
                OskField::None
            }
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

    /// Mirror whether the active tab is on the start page (each frame); entry
    /// resets the overlay to an empty search field. Returns whether it changed —
    /// without a follow-up repaint the idle loop never paints the fresh overlay.
    #[inline]
    pub fn set_home_active(&mut self, active: bool) -> bool {
        let changed = active != self.home_active;
        if active && !self.home_active {
            self.home.reset();
        }
        self.home_active = active;
        changed
    }

    /// Move the start-page selection by one dominant-axis step across the pin
    /// grid (see [`home::slot_count`]).
    #[inline]
    pub fn home_move(&mut self, dx: i32, dy: i32) {
        let count = home::slot_count(self.menu.dial.urls());
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

    // --- Speed-dial editor (the standalone overlay opened from the start page) ---

    /// Move the editor's selection by one dominant-axis step.
    #[inline]
    pub fn dial_edit_move(&mut self, dx: i32, dy: i32) {
        self.dial_edit.move_sel(dx, dy, self.dial_edit_slots());
    }

    #[inline]
    pub(super) fn dial_edit_slots(&self) -> usize {
        dial_edit::slot_count(self.menu.dial.urls())
    }

    /// Whether the trailing "Pin settings" slot is focused — drives **A** in the
    /// editor. The slot exists only while `slot_count` runs past the pins.
    pub fn dial_edit_pin_settings_selected(&self) -> bool {
        let pins = self.menu.dial.urls().len();
        self.dial_edit.tile() == Some(pins) && self.dial_edit_slots() > pins
    }

    /// Delete the editor's focused pin. The trailing tile's slot is out of
    /// range, so it's a no-op there.
    pub fn dial_edit_remove_selected(&mut self) {
        if let Some(slot) = self.dial_edit.tile() {
            self.menu.dial.remove(slot);
        }
    }

    /// Move the editor's focused pin by `delta` slots, taking the
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
        self.field_focused(super::ids::LOCATION)
    }

    /// Whether the start page's search field holds egui keyboard focus (a desktop
    /// click into it). While it does, arrow keys edit text rather than moving the
    /// start-page selection, and plain-key shortcuts are muted.
    pub fn home_field_editing(&self) -> bool {
        self.field_focused(super::ids::HOME_SEARCH)
    }

    /// Whether the speed-dial editor's URL field holds egui keyboard focus —
    /// [`Self::home_field_editing`] for the editor's `dial_edit_url` field.
    pub fn dial_edit_field_editing(&self) -> bool {
        self.field_focused(super::ids::DIAL_EDIT_URL)
    }

    /// Whether the chrome text field `id` holds egui keyboard focus.
    fn field_focused(&self, id: &str) -> bool {
        self.egui_ctx.memory(|m| m.has_focus(egui::Id::new(id)))
    }
}
