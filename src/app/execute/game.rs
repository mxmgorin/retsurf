//! Game Mode's slice of command dispatch: the mode itself, its menu, the map
//! screens and the map editor — the controller over [`crate::event::game`] and
//! [`crate::overlay::game`]. Split from the dispatcher for size alone.

use super::super::{App, AppCommand, GameInputMapsAction, GameMapEditAction, GameMenuAction};
use crate::event::bindings::Action;
use crate::event::game::input_map::{Dir, RawTarget, Side, KEY_PREFIX};
use crate::overlay::game::input_maps::{MapAction, MapRow, NameFor, Press, NEW_MAP_NAME};
use crate::overlay::game::map_edit::{self, EditPress, Kind, Row, Slot, Take, UNBOUND};
use crate::overlay::game::menu::GameRow;
use crate::overlay::osk::OskCommand;
use inputbind::Pad;

/// What a stick's row reads as once it is four directions.
const DIRECTIONS: &str = "directions";

impl App {
    /// The Game Mode gesture: the menu, always. One gesture means one screen in
    /// either state, entering and leaving are its one row, and the map can
    /// be set before a game rather than only under a running one.
    pub(super) fn game_mode_gesture(&mut self) {
        match self.ui.game_menu.visible {
            true => self.ui.game_menu.close(),
            false => self.ui.game_menu.open(self.ui.game_mode()),
        }
    }

    /// Enter Game Mode, closing whatever overlay is up: the point is that the
    /// page owns the input, and an overlay would still hold it.
    fn enter_game_mode(&mut self, out: &mut Vec<AppCommand>) {
        // Read at entry, not at startup: a pad can be plugged in later, and the
        // gestures named have to be the ones the tables actually hold.
        let handler = &self.event_handler;
        let toast = crate::ui::game_mode_toast_text(
            handler.has_pad(),
            &handler.key_gestures(Action::GameMode),
        );
        self.ui.enter_game_mode(toast);
        self.ui.osk(OskCommand::Hide, &self.browser, out);
        self.ui.menu.close();
        self.ui.hints.hide();
        if self.ui.settings.visible() {
            self.settings_close(out);
        }
        log::info!("game mode: true");
    }

    /// Leave it (the menu's Exit row), taking the menu with it.
    fn leave_game_mode(&mut self) {
        self.ui.leave_game_mode();
        self.ui.game_menu.close();
        log::info!("game mode: false");
    }

    /// Apply an action on Game Mode's menu (see [`crate::overlay::game::menu`]).
    /// It is the only screen reachable while the mode is on, so every row either
    /// returns to the game or leaves the mode.
    pub(super) fn game_menu_action(&mut self, action: &GameMenuAction, out: &mut Vec<AppCommand>) {
        match action {
            GameMenuAction::Activate => self.game_menu_activate(out),
            GameMenuAction::Click(index) => {
                self.ui.game_menu.select(*index);
                self.game_menu_activate(out);
            }
        }
    }

    /// A / Enter on the focused Game Mode row.
    fn game_menu_activate(&mut self, out: &mut Vec<AppCommand>) {
        match self.ui.game_menu.row() {
            GameRow::Resume => self.ui.game_menu.close(),
            // The maps are their own screens; the menu is what B returns to.
            GameRow::InputMap => {
                self.ui.game_menu.close();
                self.refresh_input_maps();
                self.ui.input_maps.open();
            }
            // The keyboard types into the page and outranks this menu, so close
            // it first — the two would fight over the pad otherwise.
            GameRow::Osk => {
                self.ui.game_menu.close();
                self.ui.osk(OskCommand::Show, &self.browser, out);
            }
            // The menu is the only way in and the only way out.
            GameRow::Toggle => match self.ui.game_mode() {
                true => self.leave_game_mode(),
                false => {
                    self.ui.game_menu.close();
                    self.enter_game_mode(out);
                }
            },
        }
    }

    /// Apply an action on Game Mode's map screens (see
    /// [`crate::overlay::game::input_maps`]).
    pub(super) fn input_maps_action(
        &mut self,
        action: &GameInputMapsAction,
        out: &mut Vec<AppCommand>,
    ) {
        match action {
            // B pops one screen; past the list there is the menu that opened it.
            GameInputMapsAction::Close => {
                if !self.ui.input_maps.back() {
                    self.ui.game_menu.open(self.ui.game_mode());
                }
            }
            GameInputMapsAction::Activate => self.input_maps_activate(out),
            GameInputMapsAction::Click(index) => {
                self.ui.input_maps.select(*index);
                self.input_maps_activate(out);
            }
            GameInputMapsAction::Name(text) => self.name_input_map(text.clone(), out),
        }
    }

    /// A on whichever of the three lists is up.
    fn input_maps_activate(&mut self, out: &mut Vec<AppCommand>) {
        match self.ui.input_maps.press() {
            Some(Press::New) => self.ask_new_map_name(out),
            Some(Press::Open) => self.ui.input_maps.open_selected(),
            Some(Press::Take(action)) => self.take_map_action(action, out),
            Some(Press::Confirm(true)) => self.remove_input_map(out),
            Some(Press::Confirm(false)) => self.ui.input_maps.close_confirm(),
            None => {}
        }
    }

    /// One row of a map's own screen.
    fn take_map_action(&mut self, action: MapAction, out: &mut Vec<AppCommand>) {
        let Some(id) = self.ui.input_maps.open_id_str().map(str::to_string) else {
            return;
        };
        match action {
            // Which map runs is the list's business, so it goes back there.
            MapAction::Use => {
                self.use_input_map(&id, out);
                self.ui.input_maps.back();
            }
            MapAction::Edit => {
                let name = self.ui.input_maps.open_row().map(|row| row.name.clone());
                self.ui.input_maps.close();
                self.ui.map_edit.open(id, name.unwrap_or_default());
                self.refresh_map_edit();
            }
            MapAction::Rename => self.ask_map_name(NameFor::Rename, out),
            MapAction::Duplicate => self.ask_map_name(NameFor::Duplicate, out),
            MapAction::Delete | MapAction::Reset => self.ui.input_maps.ask_confirm(),
        }
    }

    /// Hand the keyboard a name to edit: a rename starts from the current one,
    /// a copy from `<name> copy`.
    fn ask_map_name(&mut self, what: NameFor, out: &mut Vec<AppCommand>) {
        let Some(row) = self.ui.input_maps.open_row() else {
            return;
        };
        let text = match what {
            NameFor::Duplicate => format!("{} copy", row.name),
            _ => row.name.clone(),
        };
        self.ui.input_maps.start_naming(what, text);
        self.ui.osk(OskCommand::Show, &self.browser, out);
    }

    /// The list's New row: a name first, since the map is a file under it.
    fn ask_new_map_name(&mut self, out: &mut Vec<AppCommand>) {
        self.ui
            .input_maps
            .start_naming(NameFor::New, NEW_MAP_NAME.to_string());
        self.ui.osk(OskCommand::Show, &self.browser, out);
    }

    /// The keyboard submitted a name. A new map and a copy both open their own
    /// screen: they were made to be set up.
    fn name_input_map(&mut self, text: String, out: &mut Vec<AppCommand>) {
        let Some(naming) = self.ui.input_maps.take_naming() else {
            return;
        };
        let open = self.ui.input_maps.open_id_str().map(str::to_string);
        match (naming.what, open) {
            (NameFor::New, _) => {
                let new_id = self.event_handler.new_input_map(text);
                self.refresh_input_maps();
                self.ui.input_maps.open_id(&new_id);
            }
            (NameFor::Duplicate, Some(id)) => {
                let new_id = self.event_handler.duplicate_input_map(&id, text);
                self.refresh_input_maps();
                self.ui.input_maps.open_id(&new_id);
            }
            (NameFor::Rename, Some(id)) => {
                self.event_handler
                    .rename_input_map(&id, text, &self.browser, out);
                self.refresh_input_maps();
            }
            // The map went away while the keyboard was up.
            (_, None) => {}
        }
    }

    /// The confirmation said yes. A built-in comes back from the binary, so its
    /// screen stays; anything else is gone.
    fn remove_input_map(&mut self, out: &mut Vec<AppCommand>) {
        let Some(id) = self.ui.input_maps.open_id_str().map(str::to_string) else {
            return;
        };
        let (live, _) = self.event_handler.delete_input_map(&id, &self.browser, out);
        if self.config.game_mode.input_map != live {
            self.config.game_mode.input_map = live;
            self.config.save();
        }
        self.refresh_input_maps();
        self.ui.input_maps.close_confirm();
        self.ui.input_maps.open_id(&id);
    }

    /// Make a map the one Game Mode runs, and the one it starts with.
    fn use_input_map(&mut self, id: &str, out: &mut Vec<AppCommand>) {
        let (id, _) = self.event_handler.use_input_map(id, &self.browser, out);
        self.config.game_mode.input_map = id;
        self.config.save();
        self.refresh_input_maps();
        log::info!("input map: {}", self.config.game_mode.input_map);
    }

    /// Re-snapshot the map list, and the live name the menu shows with it —
    /// every change to a map goes through here.
    fn refresh_input_maps(&mut self) {
        let name = self.event_handler.input_map_name().to_string();
        self.ui.set_input_map_name(name);
        let live = self.event_handler.input_map_id().to_string();
        let rows = self
            .event_handler
            .maps
            .all()
            .iter()
            .map(|map| MapRow {
                id: map.id.clone(),
                name: map.name.clone(),
                in_use: map.id == live,
                // The row takes a file away, so there is none to offer where
                // the binary is all there is.
                remove: match (map.builtin, map.file) {
                    (_, false) => None,
                    (true, true) => Some(MapAction::Reset),
                    (false, true) => Some(MapAction::Delete),
                },
            })
            .collect();
        self.ui.input_maps.set_rows(rows);
    }

    /// Apply an action on the map editor (see [`crate::overlay::game::map_edit`]).
    pub(super) fn map_edit_action(
        &mut self,
        action: &GameMapEditAction,
        out: &mut Vec<AppCommand>,
    ) {
        match action {
            // B backs out of the lists first, then the editor, saving only if
            // something changed — an untouched visit rewrites no file.
            GameMapEditAction::Close => {
                if self.ui.map_edit.back() {
                    return;
                }
                let id = self.ui.map_edit.map_id().to_string();
                if self.ui.map_edit.close() {
                    self.event_handler.save_input_map(&id, &self.browser, out);
                }
                self.refresh_input_maps();
                self.ui.input_maps.open_id(&id);
            }
            GameMapEditAction::Activate => self.map_edit_activate(out),
            GameMapEditAction::Click(index) => {
                self.ui.map_edit.select(*index);
                self.map_edit_activate(out);
            }
            GameMapEditAction::Capture { gesture, keyboard } => {
                self.map_edit_capture(gesture, *keyboard);
            }
            GameMapEditAction::CaptureCancel => self.ui.map_edit.stop_capture(None),
            // Only over the rows: inside a kind list the same press is about
            // to take one, not to undo it.
            GameMapEditAction::Remove => {
                if let (false, Some(slot)) = (self.ui.map_edit.kind_open(), self.ui.map_edit.slot())
                {
                    self.set_map_target(slot, None);
                }
            }
        }
    }

    /// A captured gesture becomes the source a row is made for; what no row can
    /// hold is refused where it was asked for (see [`map_edit::source_of`]).
    fn map_edit_capture(&mut self, gesture: &str, keyboard: bool) {
        let slot = match map_edit::source_of(gesture, keyboard) {
            Ok(slot) => slot,
            Err(note) => return self.ui.map_edit.stop_capture(Some(note.to_string())),
        };
        self.ui.map_edit.stop_capture(None);
        // A source the map already binds has a row; the capture takes you to it
        // rather than adding a second one.
        if self.ui.map_edit.rows().iter().any(|row| row.slot == slot) {
            self.ui.map_edit.select_slot(&slot);
            self.ui.map_edit.open_kinds();
        } else {
            self.ui.map_edit.open_kinds_for(slot);
        }
    }

    /// A in the editor: open the focused row's list of kinds, or take the one it
    /// is on — a key defers to the on-screen keyboard, the rest are written
    /// straight away.
    fn map_edit_activate(&mut self, out: &mut Vec<AppCommand>) {
        match self.ui.map_edit.press() {
            Some(EditPress::OpenKinds) => self.ui.map_edit.open_kinds(),
            Some(EditPress::StartCapture) => self.ui.map_edit.start_capture(),
            // The mouse list is the one question a kind asks of its own; the
            // rows below it write like any other.
            Some(EditPress::Take(Kind::Mouse, _)) => self.ui.map_edit.open_mouse(),
            Some(EditPress::TakeMouse(kind, slot)) => {
                self.ui.map_edit.close_kinds();
                self.set_map_target(slot, Some(kind.target().to_string()));
            }
            Some(EditPress::Take(kind, slot)) => {
                self.ui.map_edit.close_kinds();
                match kind.take() {
                    Take::Text(text) => self.set_map_target(slot, Some(text.to_string())),
                    Take::Arrows => self.set_map_arrows(slot),
                    // Reached by the arm above, which has the row it is for.
                    Take::Mouse => {}
                    // The keyboard becomes a key picker; the pick lands in the
                    // editor's slot, which the loop drains.
                    Take::Key => {
                        self.ui.map_edit.set_picking(Some(slot));
                        self.ui.osk(OskCommand::Show, &self.browser, out);
                    }
                }
            }
            None => {}
        }
    }

    /// A key the picker took becomes the row's target.
    pub(in crate::app) fn drain_map_pick(&mut self, out: &mut Vec<AppCommand>) {
        let (Some(text), Some(slot)) = (self.ui.map_edit.take_picked(), self.ui.map_edit.picking())
        else {
            return;
        };
        self.ui.map_edit.set_picking(None);
        self.ui.osk(OskCommand::Hide, &self.browser, out);
        // The picker names the key; the file wants it as a target.
        self.set_map_target(slot, Some(format!("{KEY_PREFIX}{text}")));
    }

    /// Write one row into the edited map, and put the highlight where it landed
    /// — a source bound from the row that captures has no row until now.
    fn set_map_target(&mut self, slot: Slot, text: Option<String>) {
        let id = self.ui.map_edit.map_id().to_string();
        let raw = text.map(RawTarget::Short);
        if let Some(map) = self.event_handler.maps.get_mut(&id) {
            match &slot {
                Slot::Button(pad) => map.set_raw_pad(*pad, raw),
                Slot::Key(name) => map.set_raw_key(name, raw),
                Slot::Stick(side) => map.set_raw_stick(*side, raw),
                Slot::Direction(side, dir) => map.set_raw_stick_dir(*side, *dir, raw),
            }
        }
        self.edited_input_map();
        self.ui.map_edit.select_slot(&slot);
    }

    /// Hand a stick its four directions (see [`Take::Arrows`]).
    fn set_map_arrows(&mut self, slot: Slot) {
        let Slot::Stick(side) = slot else {
            return;
        };
        let id = self.ui.map_edit.map_id().to_string();
        if let Some(map) = self.event_handler.maps.get_mut(&id) {
            map.set_raw_stick_arrows(side);
        }
        self.edited_input_map();
    }

    fn edited_input_map(&mut self) {
        self.ui.map_edit.mark_dirty();
        self.refresh_map_edit();
    }

    /// Re-snapshot the editor's rows from the map it has open. A stale id keeps
    /// the last snapshot: writes to it are already no-ops (see `set_map_target`).
    fn refresh_map_edit(&mut self) {
        let Some(map) = self.event_handler.maps.get(self.ui.map_edit.map_id()) else {
            return;
        };
        let text = |raw: Option<&RawTarget>| match raw {
            Some(raw) => raw.text().to_string(),
            None => UNBOUND.to_string(),
        };
        let mut rows = vec![];
        // The sticks lead, each a row only while the map binds it.
        for side in Side::ALL {
            let digital = map.raw_stick_is_digital(side);
            if !digital && map.raw_stick(side).is_none() {
                continue;
            }
            rows.push(Row {
                slot: Slot::Stick(side),
                // A stick read as directions has no whole-stick entry to show,
                // so the row says what it has become instead.
                target: match digital {
                    true => DIRECTIONS.to_string(),
                    false => text(map.raw_stick(side)),
                },
            });
            if digital {
                rows.extend(Dir::ALL.map(|dir| Row {
                    slot: Slot::Direction(side, dir),
                    target: text(map.raw_stick_dir(side, dir)),
                }));
            }
        }
        // Then what the map binds: the pad in the pad's own order, the keys in
        // the file's. Select is not among them — the menu keeps it.
        rows.extend(
            Pad::ALL
                .into_iter()
                .filter(|pad| *pad != Pad::Select)
                .filter_map(|pad| {
                    map.raw_pad(pad).map(|raw| Row {
                        slot: Slot::Button(pad),
                        target: raw.text().to_string(),
                    })
                }),
        );
        rows.extend(map.raw_keys().map(|(name, raw)| Row {
            slot: Slot::Key(name.to_string()),
            target: raw.text().to_string(),
        }));
        self.ui.map_edit.set_rows(rows);
    }

    /// Adopt the map the config names, for a settings restore. The config
    /// is the source of truth here, so nothing is written back.
    pub(super) fn adopt_input_map(&mut self, out: &mut Vec<AppCommand>) {
        let id = self.config.game_mode.input_map.clone();
        self.event_handler.use_input_map(&id, &self.browser, out);
        self.refresh_input_maps();
    }
}
