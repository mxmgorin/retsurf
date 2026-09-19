//! Command dispatch: turning an [`AppCommand`] into effects on the browser, UI,
//! and config. The main loop ([`super::App::run`]) drains its command queue
//! through [`App::execute_command`]; the per-overlay action helpers it fans out
//! to (menu / settings / speed-dial / bookmarks) live here too. Input intents are
//! mapped earlier, in [`super::router`].

use super::{
    App, AppCommand, GameInputMapsAction, GameMapEditAction, GameMenuAction, InputCommand,
    MenuAction, PromptAction, SettingsAction,
};
use crate::browser::BrowserCommand;
use crate::config::AppConfig;
use crate::event::bindings::Action;
use crate::event::game::input_map::{Dir, RawTarget, Side};
use crate::overlay::dial_edit::EditItem;
use crate::overlay::game::input_maps::{MapAction, MapRow, NameFor, Press, NEW_MAP_NAME};
use crate::overlay::game::map_edit::{EditPress, Slot, StickTargets, Take, Targets, UNBOUND};
use crate::overlay::game::menu::GameRow;
use crate::overlay::menu::Section;
use crate::overlay::osk::OskCommand;
use crate::overlay::settings::Task;
use inputbind::Pad;

/// What a stick's row reads as once it is four directions.
const DIRECTIONS: &str = "directions";

impl App {
    pub(super) fn execute_command(&mut self, command: &AppCommand, out: &mut Vec<AppCommand>) {
        // Game Mode shrinks the browser's vocabulary to what the mode itself
        // needs, so a shortcut resolved under one of its overlays cannot act on
        // the browser behind it (see [`AppCommand::in_game_mode`]). Its menu
        // counts either way: it owns the input wherever it was opened.
        if (self.ui.game_mode() || self.ui.game_screen()) && !command.in_game_mode() {
            return;
        }
        match command {
            AppCommand::Shutdown => self.shutdown(),
            // On a window resize, size the browser to the new central area straight
            // away from the actual window (egui's reactive sizing can lag a frame).
            AppCommand::Resize => {
                self.sync_screen_geometry();
                self.ui.resize_browser(&self.window, &self.browser);
            }
            AppCommand::Browser(command) => {
                self.browser.execute_command(command, &self.config.browser)
            }
            AppCommand::Input(command) => self.route_input(command, out),
            AppCommand::Menu(action) => self.menu_action(action),
            AppCommand::ToggleBookmark => self.toggle_current_bookmark(),
            AppCommand::GameMode => self.game_mode_gesture(),
            AppCommand::GameMenu(action) => self.game_menu_action(action, out),
            AppCommand::GameInputMaps(action) => self.input_maps_action(action, out),
            AppCommand::GameMapEdit(action) => self.map_edit_action(action, out),
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
    fn game_menu_action(&mut self, action: &GameMenuAction, out: &mut Vec<AppCommand>) {
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
    fn input_maps_action(&mut self, action: &GameInputMapsAction, out: &mut Vec<AppCommand>) {
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
    fn map_edit_action(&mut self, action: &GameMapEditAction, out: &mut Vec<AppCommand>) {
        match action {
            // B backs out of the lists first, then out of the editor — saving
            // on the way, and only if something changed, so an untouched visit
            // never rewrites a file the user hand-edited.
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
        }
    }

    /// A in the editor: open a stick's rows, open the focused row's list of
    /// kinds, or take the one it is on — a key defers to the on-screen
    /// keyboard, the rest are written straight away.
    fn map_edit_activate(&mut self, out: &mut Vec<AppCommand>) {
        match self.ui.map_edit.press() {
            Some(EditPress::OpenStick(side)) => self.ui.map_edit.open_stick(side),
            Some(EditPress::OpenKinds) => self.ui.map_edit.open_kinds(),
            Some(EditPress::Take(kind, slot)) => {
                self.ui.map_edit.close_kinds();
                match kind.take() {
                    Take::Text(text) => self.set_map_target(slot, Some(text.to_string())),
                    Take::Arrows => self.set_map_arrows(slot),
                    // The keyboard becomes a key picker; the pick lands in the
                    // editor's slot, which the loop drains (see
                    // [`App::drain_map_pick`]).
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
    pub(super) fn drain_map_pick(&mut self, out: &mut Vec<AppCommand>) {
        let (Some(text), Some(slot)) = (self.ui.map_edit.take_picked(), self.ui.map_edit.picking())
        else {
            return;
        };
        self.ui.map_edit.set_picking(None);
        self.ui.osk(OskCommand::Hide, &self.browser, out);
        self.set_map_target(slot, Some(text));
    }

    /// Write one row into the edited map.
    fn set_map_target(&mut self, slot: Slot, text: Option<String>) {
        let id = self.ui.map_edit.map_id().to_string();
        let raw = text.map(RawTarget::Short);
        if let Some(map) = self.event_handler.maps.get_mut(&id) {
            match slot {
                Slot::Button(pad) => map.set_raw_pad(pad, raw),
                Slot::Stick(side) => map.set_raw_stick(side, raw),
                Slot::Direction(side, dir) => map.set_raw_stick_dir(side, dir, raw),
            }
        }
        self.edited_input_map();
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
        let pads = Pad::ALL
            .into_iter()
            .map(|pad| text(map.raw_pad(pad)))
            .collect();
        let sticks = Side::ALL.map(|side| {
            let digital = map.raw_stick_is_digital(side);
            StickTargets {
                digital,
                // A stick read as directions has no whole-stick entry to show,
                // so the row says what it has become instead.
                role: match digital {
                    true => DIRECTIONS.to_string(),
                    false => text(map.raw_stick(side)),
                },
                dirs: Dir::ALL.map(|dir| text(map.raw_stick_dir(side, dir))),
            }
        });
        self.ui.map_edit.set_targets(Targets { pads, sticks });
    }

    /// Adopt the map the config names, for a settings restore. The config
    /// is the source of truth here, so nothing is written back.
    fn adopt_input_map(&mut self, out: &mut Vec<AppCommand>) {
        let id = self.config.game_mode.input_map.clone();
        self.event_handler.use_input_map(&id, &self.browser, out);
        self.refresh_input_maps();
    }

    /// Apply a menu action (Tabs / Bookmarks / History / Downloads overlay).
    fn menu_action(&mut self, action: &MenuAction) {
        match action {
            // Select toggles the menu; the menu button only ever opens it (it's hidden
            // behind the menu once shown).
            MenuAction::Open => {
                if self.ui.menu.visible {
                    self.ui.menu.close();
                } else {
                    self.ui.menu_open();
                }
            }
            MenuAction::Close => self.ui.menu.close(),
            MenuAction::SetSection(section) => self.ui.menu.set_section(*section),
            MenuAction::OpenSelected => self.menu_open_selected(),
            MenuAction::RemoveSelected => self.delete_menu_selection(),
            MenuAction::Clear => self.ui.menu.clear_or_arm(),
            MenuAction::OpenUrl(url) => self.open_url(url.clone()),
            MenuAction::ToggleBookmark(url) => self.ui.menu.toggle_bookmark(url),
            MenuAction::DialEdit => self.ui.open_pins_editor(),
            MenuAction::DialClose => self.ui.close_pins_editor(),
            MenuAction::DialAdd(url) => self.dial_add(url),
            MenuAction::DialRemoveAt(index) => self.ui.menu.dial.remove(*index),
            MenuAction::DialPinSettings => self.ui.menu.dial.pin(crate::data::dial::SETTINGS_PIN),
            MenuAction::RemoveAt(index) => self.ui.menu.remove_at(*index),
            MenuAction::OpenTab(index) => {
                self.browser.switch_to(*index);
                self.ui.menu.close();
            }
            MenuAction::CloseTab(index) => {
                self.browser
                    .close_tab(*index, &self.config.browser.home_page);
                self.ui.menu.set_tab_count(self.browser.tab_count());
                self.schedule_heap_trim();
            }
            MenuAction::NewTab => self.new_tab(),
        }
    }

    /// Open a new tab at the home page and close the menu.
    fn new_tab(&mut self) {
        let home = self.config.browser.home_page.clone();
        self.browser.open_tab(&home);
        self.ui.menu.close();
    }

    /// Toggle the current page in saved bookmarks (the bookmark button / Start).
    fn toggle_current_bookmark(&mut self) {
        let url = self.browser.get_state_mut().page_url().to_string();
        if !url.is_empty() {
            self.ui.menu.toggle_bookmark(&url);
        }
    }

    /// Open the highlighted menu entry (the **A** button / Enter). In Tabs this
    /// switches to the tab (or opens a new one on the "+ New tab" row); in the URL
    /// lists it loads the entry. Closes the menu either way.
    pub(super) fn menu_open_selected(&mut self) {
        if self.ui.menu.section() == Section::Tabs {
            let sel = self.ui.menu.tab_selected();
            if sel == 0 {
                self.new_tab(); // the "+ New tab" button (index 0)
            } else {
                self.browser.switch_to(sel - 1);
                self.ui.menu.close();
            }
        } else if self.ui.menu.clear_selected() {
            // The section's clear row (index 0): arms, then wipes; stays open.
            self.ui.menu.clear_or_arm();
        } else if let Some(url) = self.ui.menu.selected_url() {
            self.open_url(url);
        } else if self.ui.menu.section() != Section::Downloads {
            // A on an active/failed download has nothing to open — keep the menu
            // up so the user can watch the progress; other sections close.
            self.ui.menu.close();
        }
    }

    /// Delete the highlighted menu entry (the **X** button / Delete). In Tabs this
    /// closes the tab; in the URL lists it removes the bookmark / history entry.
    pub(super) fn delete_menu_selection(&mut self) {
        if self.ui.menu.section() == Section::Tabs {
            // Index 0 is the "+ New tab" button (nothing to delete); tabs are 1.. .
            let sel = self.ui.menu.tab_selected();
            if sel > 0 {
                self.browser
                    .close_tab(sel - 1, &self.config.browser.home_page);
                self.ui.menu.set_tab_count(self.browser.tab_count());
            }
        } else {
            self.ui.menu.remove_selected();
        }
    }

    /// Y in the menu (link-hint toggle elsewhere): the action depends on the
    /// section. Bookmarks pins/unpins the selected entry on the speed dial;
    /// History bookmarks (or un-bookmarks) the selected entry; Tabs bookmarks
    /// the selected tab's URL. Downloads has no Y action.
    pub(super) fn menu_y_action(&mut self) {
        match self.ui.menu.section() {
            Section::Bookmarks => {
                if let Some(url) = self.ui.menu.selected_url() {
                    self.ui.menu.dial.toggle(&url);
                }
            }
            Section::History => {
                if let Some(url) = self.ui.menu.selected_url() {
                    self.ui.menu.toggle_bookmark(&url);
                }
            }
            Section::Tabs => {
                // Index 0 is the "+ New tab" button; the tabs follow at 1..=N.
                let sel = self.ui.menu.tab_selected();
                if sel > 0 {
                    if let Some(info) = self.browser.tabs().get(sel - 1) {
                        if !info.url.is_empty() {
                            self.ui.menu.toggle_bookmark(&info.url);
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
                if self.ui.settings.visible() {
                    self.settings_close(out);
                    self.shutdown();
                } else {
                    self.ui.settings_open(&self.config);
                }
            }
            SettingsAction::Close => self.settings_close(out),
            SettingsAction::SetSection(section) => self.ui.settings.set_section(*section),
            SettingsAction::Select(index) => self.ui.settings.set_selected(*index),
            SettingsAction::Activate => self.settings_confirm(out),
            SettingsAction::Adjust(dx) => self.ui.settings.adjust(*dx),
            // A link on the About tab: save & close like a normal exit, then load
            // it in the focused tab (open_url also tidies the menu, harmless here).
            SettingsAction::OpenLink(url) => {
                self.settings_close(out);
                self.open_url(url.clone());
            }
            // Binding capture (Controls section): the gesture the user performed
            // (gamepad gesture or key combo), bound to the listening action. The
            // raw input comes from the event loop / pad while capturing (see
            // [`crate::event::handler`] / [`crate::event::gamepad`]).
            SettingsAction::CaptureBinding { gesture, keyboard } => {
                self.ui.settings.apply_capture(gesture.clone(), *keyboard);
            }
            SettingsAction::CaptureCancel => self.ui.settings.cancel_capture(),
            // Self-update (About tab, PortMaster only). Check/Install forward to the
            // background Updater; Quit reuses the audited two-step-quit path so the
            // launcher's pm_finish runs and re-execs the freshly swapped binary.
            SettingsAction::CheckUpdate => {
                // A channel edited in this visit is still only in the overlay draft
                // (`apply_config` runs on close), so adopt it before checking.
                if let Some(update) = self.ui.settings.pending_update().cloned() {
                    self.ui.set_update_config(&update);
                }
                self.ui.update_check(&self.event_sender);
            }
            SettingsAction::InstallUpdate => self.ui.update_install(&self.event_sender),
            SettingsAction::QuitForUpdate => {
                self.settings_close(out);
                self.shutdown();
            }
        }
    }

    /// A / Enter on the focused settings row: add/remove a binding in the Controls
    /// section, run a confirmed action row, open the on-screen keyboard on a text
    /// field, or step every other kind forward (Left/Right does the rest).
    pub(super) fn settings_confirm(&mut self, out: &mut Vec<AppCommand>) {
        if self.ui.settings.is_info_section() {
            // About tab: A activates the focused row (update action or a link);
            // the resulting SettingsAction routes back through settings_action.
            if let Some(action) = self.ui.about_activate() {
                out.push(AppCommand::Settings(action));
            }
        } else if self.ui.settings.is_controls_section() {
            self.ui.settings.controls_activate();
        } else if let Some(task) = self.ui.settings.confirm_action() {
            match task {
                Task::ClearData => self.clear_browsing_data(),
                Task::RestoreDefaults => self.restore_defaults(),
            }
        } else if self.ui.settings.selected_is_text() {
            self.ui.osk(OskCommand::Show, &self.browser, out);
        } else {
            self.ui.settings.adjust(1);
        }
    }

    /// Wipe the browsing data: history, the finished downloads, the saved
    /// session and the open tabs, plus Servo's cookies, web storage and HTTP
    /// cache. Bookmarks, pins and the settings stay.
    fn clear_browsing_data(&mut self) {
        self.ui.menu.history_mut().clear();
        self.ui.menu.downloads.clear_finished();
        self.session.discard();
        self.browser.clear_site_data();
        self.browser.reset_tabs(&self.config.browser.home_page);
        log::info!("cleared browsing data");
    }

    /// Settings and bindings (overlay drafts, saved on close) plus the pins (their
    /// own file, written now) back to how they ship. Bookmarks, history and tabs
    /// are [`Self::clear_browsing_data`]'s business.
    fn restore_defaults(&mut self) {
        self.ui.settings.restore_defaults();
        self.ui.menu.dial.reset();
        log::info!("restored default settings, bindings and pins");
    }

    /// Close the settings overlay (B / close button): adopt its edited drafts
    /// — the config and the gamepad bindings, each saved and re-applied live.
    pub(super) fn settings_close(&mut self, out: &mut Vec<AppCommand>) {
        let (config, bindings) = self.ui.settings_close();
        self.apply_config(config, out);
        if let Some(store) = bindings {
            self.apply_bindings(store, out);
        }
    }

    /// Adopt edited bindings from the settings overlay: persist them, then rebuild
    /// both devices' tables in the running handler (no restart). Only called when
    /// the controls changed, so a config-only edit leaves `bindings.toml` — and any
    /// hand-written comments in it — alone.
    fn apply_bindings(&mut self, store: inputbind::Store, out: &mut Vec<AppCommand>) {
        crate::event::bindings::save(&store);
        // The pad drops what it holds, so a click still open closes here.
        self.event_handler.set_bindings(&store, out);
    }

    /// Adopt an edited config from the settings overlay: persist it to disk, then
    /// re-apply the parts the running app can change without a restart. The rest
    /// (window size, GL backend, engine threads, ad-block lists, persisted site
    /// data) take effect on the next launch — those rows are flagged with `*`.
    fn apply_config(&mut self, config: AppConfig, out: &mut Vec<AppCommand>) {
        self.config = config;
        self.config.save();
        // Restoring the defaults can move the pad map under a live Game
        // Mode, so push it the same way the menu does.
        self.adopt_input_map(out);
        // The router reads cursor/scroll speeds from the config each frame, but
        // the gamepad state machine and the UI cache a few values to push in.
        self.event_handler
            .set_gamepad_config(self.config.input.clone());
        self.ui
            .set_cursor_linger(self.config.display.cursor_linger_ms);
        self.ui.set_ui_scale(self.config.display.scale);
        self.ui
            .set_toolbar_position(self.config.display.toolbar_position);
        self.ui
            .set_toolbar_autohide(self.config.display.toolbar_autohide);
        self.ui.set_hint_badges(self.config.input.hint_badges);
        self.browser.set_haptics(self.config.input.haptics);
        self.ui.menu.history_mut().set_config(&self.config.history);
        self.ui.set_memory_debug(
            self.config.debug.memory_overlay,
            self.config.debug.memory_log,
        );
        self.ui.set_update_config(&self.config.update);
        // Lightweight-mode block flags take effect on the next subresource load,
        // no restart needed (unlike the engine-thread counts beside them).
        self.browser.set_content_filter(
            crate::browser::content_filter::ContentFilter::from_config(&self.config.data_saving),
        );
        // Experimental features apply live too — effective on the next page load.
        self.browser
            .set_experimental_prefs(&self.config.experimental);
        // The page theme needs no reload at all: open tabs restyle in place.
        self.browser.set_page_theme(self.config.browser.page_theme);
        // Off drops the stored session now, not on the next launch.
        if !self.config.browser.restore_tabs {
            self.session.discard();
        }
        // Binds later opens; the tabs already open stay.
        self.browser.set_max_tabs(self.config.browser.max_tabs);
        self.cpu_boost
            .set_enabled(self.config.performance.cpu_boost_on_load);
        // The frame cap takes effect on the very next frame, which is what makes
        // it worth tuning by hand on a device.
        self.window.set_max_fps(self.config.display.max_fps);
    }

    /// A on the start page: open the focused speed-dial tile, open the speed-dial
    /// editor on the "Edit" tile, or — when the search field is focused — open
    /// the OSK to type into it.
    pub(super) fn home_confirm(&mut self, out: &mut Vec<AppCommand>) {
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
    pub(super) fn dial_edit_confirm(&mut self, out: &mut Vec<AppCommand>) {
        match self.ui.dial_edit_item() {
            EditItem::Field => {
                self.ui.dial_edit_focus_field();
                self.ui.osk(OskCommand::Show, &self.browser, out);
            }
            // A on the trailing tile pins the settings shortcut; pin tiles are
            // edit-only (delete with X, move with L1/R1).
            EditItem::Tile(_) => {
                if self.ui.dial_edit_pin_settings_selected() {
                    self.ui.menu.dial.pin(crate::data::dial::SETTINGS_PIN);
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
            self.ui.menu.dial.pin(url.as_str());
        }
        self.ui.dial_edit_clear_input();
    }

    /// Load `url` in the focused tab and close the menu. The settings pin is a
    /// sentinel, not a real address: it opens the settings overlay instead of
    /// navigating (so a settings speed-dial tile / menu row behaves like the toolbar's).
    fn open_url(&mut self, url: String) {
        if url == crate::data::dial::SETTINGS_PIN {
            self.ui.menu.close();
            self.ui.settings_open(&self.config);
            return;
        }
        self.browser.get_state_mut().location = url;
        self.browser
            .execute_command(&BrowserCommand::Load, &self.config.browser);
        self.ui.menu.close();
    }
}
