//! Game Mode's slice of command dispatch: the mode itself, Quick Access, the map
//! screens and the map editor — the controller over [`crate::event::game`],
//! [`crate::overlay::quick_access`] and [`crate::overlay::game`]. Split from the
//! dispatcher for size alone.

use super::super::{App, AppCommand, GameInputMapsAction, GameMapEditAction, QuickAccessAction};
use crate::browser::{BrowserCommand, TabMode};
use crate::config::{Scaling, TextureFilter, TextureLimit};
use crate::event::bindings::{self, Action};
use crate::event::game::input_map::{
    Dir, KeyMods, RawTarget, Side, KEY_PREFIX, LAYER_PREFIX, PAD_PREFIX,
};
use crate::event::game::GameMode;
use crate::overlay::game::input_maps::{MapAction, MapRow, NameFor, Press, NEW_MAP_NAME};
use crate::overlay::game::map_edit::{
    self, CaptureFor, Device, EditPress, Kind, LayerName, LayerRow, Modifier, Row, Slot, Tail,
    Take, DIRECTIONS, NEW_LAYER_NAME, UNBOUND,
};
use crate::overlay::menu::Section;
use crate::overlay::osk::OskCommand;
use crate::overlay::quick_access::{Entry, Over, Strip};
use crate::overlay::settings::{self, Settings, SettingsSection};
use inputbind::Pad;

impl App {
    /// Game Mode, loaded on demand with the map the config asks for.
    fn game_mode(&mut self) -> &mut GameMode {
        self.event_handler
            .game_mut(&self.config.game_mode.input_map)
    }

    /// Open `strip`, or close it when it is up; with the other strip up, swap.
    fn toggle_strip(&mut self, strip: Strip) {
        let panel = &mut self.ui.quick_access;
        if panel.visible && panel.strip() == strip {
            panel.close();
            return;
        }
        // The lists are what Quick Menu opens, so its gesture also closes them.
        if strip == Strip::QuickMenu && self.ui.menu.visible {
            self.ui.menu.close();
            return;
        }
        // Over the start page the focused tile leads the strip.
        if strip == Strip::QuickAccess && self.browser.on_home_page() {
            self.open_home_access();
            return;
        }
        let launched = self.browser.launched().is_some();
        let over = match self.browser.on_home_page() {
            true => Over::Home,
            false => Over::Tab(self.browser.mode()),
        };
        self.ui
            .quick_access
            .open(strip, over, launched, &self.config);
        let url = self.browser.state().page_url().to_string();
        let saved = match !url.is_empty() && self.ui.menu.is_bookmarked(&url) {
            true => "Saved",
            false => "",
        };
        let tabs = self.browser.tab_count().to_string();
        let panel = &mut self.ui.quick_access;
        panel.set_value(Entry::Run(Action::Bookmark), saved.to_string());
        panel.set_value(Entry::List(Section::Tabs), tabs);
        if self.browser.in_game_mode() {
            let (_, name) = self.game_mode().live();
            self.ui.quick_access.set_value(Entry::InputMap, name);
        }
    }

    /// Switch to the map `dx` places along the list, wrapping at the ends, and
    /// run it at once, as the map screens' Use does.
    fn step_input_map(&mut self, dx: i32, out: &mut Vec<AppCommand>) {
        let (live, _) = self.game_mode().live();
        let ids: Vec<String> = self
            .game_mode()
            .maps
            .all()
            .iter()
            .map(|m| m.id.clone())
            .collect();
        let at = ids.iter().position(|id| *id == live).unwrap_or(0);
        let next = ids[crate::list::wrap(at, dx, ids.len())].clone();
        self.use_input_map(&next, out);
        let (_, name) = self.game_mode().live();
        self.ui.quick_access.set_value(Entry::InputMap, name);
    }

    /// Open the settings screen, on `section` when given. Reads the live map's
    /// name for the Gaming tab, which loads the maps.
    pub(super) fn open_settings(&mut self, section: Option<SettingsSection>) {
        let (_, name) = self.game_mode().live();
        self.ui.set_input_map_name(name);
        self.ui.settings_open(&self.config);
        if let Some(section) = section {
            self.ui.settings.set_section(section);
        }
    }

    /// The input-map screens, over whatever opened them; B at their top closes
    /// them back onto it.
    pub(super) fn open_input_maps(&mut self) {
        self.refresh_input_maps();
        self.ui.input_maps.open();
    }

    /// Enter Game Mode, closing whatever overlay is up: the point is that the
    /// page owns the input, and an overlay would still hold it.
    pub(in crate::app) fn enter_game_mode(&mut self, out: &mut Vec<AppCommand>) {
        // The mode has nothing to route through without its maps, and this is
        // the one path in.
        self.game_mode();
        // Read at entry, not at startup: a pad can be plugged in later, and the
        // gestures named have to be the ones the tables actually hold.
        let keys: Vec<(String, Strip)> = [
            (Action::QuickAccess, Strip::QuickAccess),
            (Action::Menu, Strip::QuickMenu),
        ]
        .into_iter()
        .filter_map(|(action, strip)| {
            let gesture = bindings::key_gestures(action).into_iter().next()?;
            Some((gesture, strip))
        })
        .collect();
        let hint = crate::ui::game_mode_hint(
            &self.event_handler.game_strip_gestures(),
            &keys,
            self.event_handler.hardware_input(),
        );
        self.browser.set_mode(TabMode::Game);
        self.ui.enter_game_mode(hint);
        self.browser
            .set_game_scaling(self.config.game_mode.view.scaling);
        let view = &self.config.game_mode.view;
        self.browser
            .set_game_textures(view.texture_limit, view.texture_filter);
        self.ui.osk(OskCommand::Hide, &self.browser, out);
        self.ui.menu.close();
        self.ui.hints.hide();
        if self.ui.settings.visible() {
            self.settings_close(out);
        }
        log::info!("game mode: true");
    }

    /// Leave it (Quick Access's Exit row), closing the panel.
    pub(super) fn leave_game_mode(&mut self) {
        self.ui.leave_game_mode();
        self.browser.set_mode(TabMode::Page);
        self.ui.quick_access.close();
        self.browser.set_game_scaling(Scaling::None);
        self.browser
            .set_game_textures(TextureLimit::Full, TextureFilter::default());
        // A pad the map asked for exists only while the map runs.
        self.browser.drop_mapped_pad();
        log::info!("game mode: false");
    }

    /// Apply an action on Quick Access (see [`crate::overlay::quick_access`]).
    pub(super) fn quick_access_action(
        &mut self,
        action: &QuickAccessAction,
        out: &mut Vec<AppCommand>,
    ) {
        match action {
            QuickAccessAction::Toggle(strip) => self.toggle_strip(*strip),
            QuickAccessAction::Close => self.ui.quick_access.close(),
            QuickAccessAction::Activate => self.quick_access_activate(out),
            // Rows with a value step; the rest act on A alone.
            QuickAccessAction::Adjust(dx) => match self.ui.quick_access.row() {
                Entry::Quick(field) => self.step_quick(field, *dx, out),
                Entry::InputMap => self.step_input_map(*dx, out),
                Entry::Run(Action::Reader) => self.quick_access_activate(out),
                Entry::Options(row) => self.step_options_row(row, *dx),
                _ => {}
            },
            QuickAccessAction::Click(index) => {
                self.ui.quick_access.select(*index);
                self.quick_access_activate(out);
            }
            QuickAccessAction::Text(text) => self.options_text(text),
        }
    }

    /// A / Enter on the focused Quick Access row.
    fn quick_access_activate(&mut self, out: &mut Vec<AppCommand>) {
        match self.ui.quick_access.row() {
            Entry::Quick(field) => self.step_quick(field, 1, out),
            Entry::InputMap => {
                self.ui.quick_access.close();
                self.open_input_maps();
            }
            // The keyboard types into the page and outranks this panel, so close
            // it first — the two would fight over the pad otherwise.
            Entry::Osk => {
                self.ui.quick_access.close();
                self.ui.osk(OskCommand::Show, &self.browser, out);
            }
            // Direct, not queued: the mode's command filter drops the browser's.
            Entry::Run(Action::Bookmark) => {
                self.ui.quick_access.close();
                self.toggle_current_bookmark();
            }
            // As its gesture would, once the panel is out of the way.
            Entry::Run(action) => {
                self.ui.quick_access.close();
                out.extend(action.command(true));
            }
            // Direct, not queued: the mode's command filter drops browser commands.
            Entry::Reload => {
                if self.ui.quick_access.confirm(Entry::Reload) {
                    self.ui.quick_access.close();
                    self.browser
                        .execute_command(&BrowserCommand::Reload, &self.config);
                }
            }
            Entry::List(section) => {
                self.ui.quick_access.close();
                self.ui.menu_open();
                self.ui.menu.set_section(section);
            }
            Entry::Quit => {
                if self.ui.quick_access.confirm(Entry::Quit) {
                    out.push(AppCommand::Shutdown);
                }
            }
            Entry::Settings => {
                self.ui.quick_access.close();
                let section = self
                    .browser
                    .in_game_mode()
                    .then_some(SettingsSection::Gaming);
                self.open_settings(section);
            }
            Entry::Enter => {
                self.ui.quick_access.close();
                self.enter_game_mode(out);
            }
            Entry::Exit => self.leave_game_mode(),
            Entry::Leave => self.exit_launch(out),
            Entry::Options(row) => self.options_row(row, out),
        }
    }

    /// Step a quick row's field by `dx` and apply it at once, the way the settings
    /// screen applies its draft on close.
    fn step_quick(&mut self, field: usize, dx: i32, out: &mut Vec<AppCommand>) {
        let mut config = self.config.clone();
        settings::step(&Settings::fields()[field].kind, &mut config, dx);
        self.apply_config(config, out);
        self.ui.quick_access.refresh(&self.config);
    }

    /// Apply an action on Game Mode's map screens (see
    /// [`crate::overlay::game::input_maps`]).
    pub(super) fn input_maps_action(
        &mut self,
        action: &GameInputMapsAction,
        out: &mut Vec<AppCommand>,
    ) {
        match action {
            // B pops one screen; past the list, what opened it is underneath.
            GameInputMapsAction::Close => {
                self.ui.input_maps.back();
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
            Some(Press::UseSelected) => {
                if let Some(id) = self.ui.input_maps.selected_row().map(|row| row.id.clone()) {
                    self.use_input_map(&id, out);
                }
            }
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
                let new_id = self
                    .event_handler
                    .game_mut(&self.config.game_mode.input_map)
                    .add(text);
                self.refresh_input_maps();
                self.ui.input_maps.open_id(&new_id);
            }
            (NameFor::Duplicate, Some(id)) => {
                let new_id = self
                    .event_handler
                    .game_mut(&self.config.game_mode.input_map)
                    .duplicate(&id, text);
                self.refresh_input_maps();
                self.ui.input_maps.open_id(&new_id);
            }
            (NameFor::Rename, Some(id)) => {
                let game = self
                    .event_handler
                    .game_mut(&self.config.game_mode.input_map);
                game.rename(&id, text, &self.browser, out);
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
        let game = self
            .event_handler
            .game_mut(&self.config.game_mode.input_map);
        let (live, _) = game.delete(&id, &self.browser, out);
        if self.config.game_mode.input_map != live {
            self.config.game_mode.input_map = live;
            self.save_config();
        }
        self.refresh_input_maps();
        self.ui.input_maps.close_confirm();
        self.ui.input_maps.open_id(&id);
    }

    /// Make a map the one Game Mode runs, and the one it starts with.
    fn use_input_map(&mut self, id: &str, out: &mut Vec<AppCommand>) {
        let game = self
            .event_handler
            .game_mut(&self.config.game_mode.input_map);
        let (id, _) = game.use_map(id, &self.browser, out);
        self.ui.settings.adopt_input_map(&id);
        self.config.game_mode.input_map = id;
        self.save_config();
        self.refresh_input_maps();
        log::info!("input map: {}", self.config.game_mode.input_map);
    }

    /// Re-snapshot the map list, and the live name the menu shows with it —
    /// every change to a map goes through here.
    fn refresh_input_maps(&mut self) {
        let (live, name) = self.game_mode().live();
        self.ui.set_input_map_name(name);
        let rows = self
            .game_mode()
            .maps
            .all()
            .iter()
            .map(|map| MapRow {
                id: map.id.clone(),
                name: map.name.clone(),
                in_use: map.id == live,
                // The row takes a file away, so there is none to offer where
                // the binary is all there is.
                fixed: map.fixed,
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
                // Leaving a layer changes which rows are the screen's.
                if self.ui.map_edit.back() {
                    return self.refresh_map_edit();
                }
                let id = self.ui.map_edit.map_id().to_string();
                if self.ui.map_edit.close() {
                    let game = self
                        .event_handler
                        .game_mut(&self.config.game_mode.input_map);
                    game.save(&id, &self.browser, out);
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
                if self.ui.map_edit.kind_open() {
                    return;
                }
                if let Some(slot) = self.ui.map_edit.slot() {
                    self.set_map_target(slot, None);
                } else if self.ui.map_edit.focused_tail() == Some(Tail::HeldBy) {
                    self.set_activator(None);
                }
            }
            GameMapEditAction::NameLayer(text) => self.name_layer(text),
            GameMapEditAction::Adjust(dx) => {
                if let Some((slot, speed)) = self.ui.map_edit.stepped_speed(*dx) {
                    if let Some(old) = self.map_raw(&slot) {
                        self.write_map_raw(slot, Some(old.with_speed(speed)));
                    }
                }
            }
        }
    }

    /// Name a new layer or rename the scoped one; a name another layer holds
    /// gets a suffix, since the name is the layer's key in the file.
    fn name_layer(&mut self, text: &str) {
        let Some(naming) = self.ui.map_edit.take_naming() else {
            return;
        };
        let id = self.ui.map_edit.map_id().to_string();
        let scope = self.ui.map_edit.layer().map(str::to_string);
        let Some(map) = self.game_mode().maps.get_mut(&id) else {
            return;
        };
        let taken: Vec<String> = map
            .raw_layers()
            .filter(|name| Some(*name) != scope.as_deref())
            .map(str::to_string)
            .collect();
        let name = unique_name(text, &taken);
        match (naming.what, scope) {
            (LayerName::New, _) => {
                map.add_raw_layer(&name);
                self.ui.map_edit.open_layer(name);
            }
            (LayerName::Rename, Some(old)) => {
                map.rename_raw_layer(&old, &name);
                self.ui.map_edit.rename_scope(name);
            }
            // A rename is only offered inside a layer.
            (LayerName::Rename, None) => return,
        }
        self.edited_input_map();
    }

    /// Delete the layer the screen is scoped to, and go back to the base.
    fn delete_layer(&mut self) {
        self.ui.map_edit.close_confirm();
        let Some(name) = self.ui.map_edit.layer().map(str::to_string) else {
            return;
        };
        let id = self.ui.map_edit.map_id().to_string();
        if let Some(map) = self.game_mode().maps.get_mut(&id) {
            map.remove_raw_layer(&name);
        }
        self.ui.map_edit.leave_layer();
        self.edited_input_map();
    }

    /// Hand the keyboard a layer name to edit.
    fn ask_layer_name(&mut self, what: LayerName, out: &mut Vec<AppCommand>) {
        let text = match what {
            LayerName::New => NEW_LAYER_NAME,
            LayerName::Rename => self.ui.map_edit.layer().unwrap_or(NEW_LAYER_NAME),
        };
        self.ui.map_edit.start_naming(what, text.to_string());
        self.ui.osk(OskCommand::Show, &self.browser, out);
    }

    /// A captured gesture becomes the source a row is made for; what no row can
    /// hold is refused where it was asked for (see [`map_edit::source_of`]).
    fn map_edit_capture(&mut self, gesture: &str, keyboard: bool) {
        let purpose = self.ui.map_edit.capturing();
        let slot = match map_edit::source_of(gesture, keyboard) {
            Ok(Slot::Stick(_)) if purpose == Some(CaptureFor::Activator) => {
                let note = "a stick cannot hold a layer open".to_string();
                return self.ui.map_edit.stop_capture(Some(note));
            }
            Ok(slot) => slot,
            Err(note) => return self.ui.map_edit.stop_capture(Some(note.to_string())),
        };
        self.ui.map_edit.stop_capture(None);
        if purpose == Some(CaptureFor::Activator) {
            return self.set_activator(Some(slot));
        }
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
            Some(EditPress::StartCapture) => self.ui.map_edit.start_capture(CaptureFor::Source),
            Some(EditPress::CaptureActivator) => {
                self.ui.map_edit.start_capture(CaptureFor::Activator)
            }
            // The mouse list is the one question a kind asks of its own; the
            // rows below it write like any other.
            Some(EditPress::Take(Kind::Mouse, _)) => self.ui.map_edit.open_device(Device::Mouse),
            Some(EditPress::Take(Kind::Gamepad, _)) => {
                self.ui.map_edit.open_device(Device::Gamepad)
            }
            Some(EditPress::Take(Kind::Layer, _)) => self.ui.map_edit.open_device(Device::Layer),
            Some(EditPress::TakeLayer(name, slot)) => {
                self.ui.map_edit.close_kinds();
                self.set_map_target(slot, Some(format!("{LAYER_PREFIX}{name}")));
            }
            Some(EditPress::OpenLayer(name)) => {
                self.ui.map_edit.open_layer(name);
                self.refresh_map_edit();
            }
            Some(EditPress::NewLayer) => self.ask_layer_name(LayerName::New, out),
            Some(EditPress::RenameLayer) => self.ask_layer_name(LayerName::Rename, out),
            Some(EditPress::AskDelete) => self.ui.map_edit.ask_confirm(),
            Some(EditPress::Delete(true)) => self.delete_layer(),
            Some(EditPress::Delete(false)) => self.ui.map_edit.close_confirm(),
            Some(EditPress::TakeMouse(kind, slot)) => {
                self.ui.map_edit.close_kinds();
                self.set_map_target(slot, Some(kind.target().to_string()));
            }
            Some(EditPress::TakePad(button, slot)) => {
                self.ui.map_edit.close_kinds();
                self.set_map_target(slot, Some(format!("{PAD_PREFIX}{}", button.name())));
            }
            Some(EditPress::Take(Kind::Modifiers, _)) => {
                self.ui.map_edit.open_device(Device::Modifiers)
            }
            Some(EditPress::ToggleModifier(modifier, slot)) => {
                self.toggle_map_modifier(slot, modifier)
            }
            Some(EditPress::Take(kind, slot)) => {
                self.ui.map_edit.close_kinds();
                match kind.take() {
                    Take::Text(text) => self.set_map_target(slot, Some(text.to_string())),
                    Take::Arrows => self.set_map_arrows(slot),
                    Take::Remove => self.set_map_target(slot, None),
                    // Reached by the arms above, which have the row they are for.
                    Take::Mouse | Take::Gamepad | Take::Layer | Take::Modifiers => {}
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

    /// Point one row at `text`, keeping what of its old target still applies.
    fn set_map_target(&mut self, slot: Slot, text: Option<String>) {
        let old = self.map_raw(&slot);
        let raw = text.map(|to| RawTarget::retarget(old.as_ref(), to));
        self.write_map_raw(slot, raw);
    }

    fn toggle_map_modifier(&mut self, slot: Slot, modifier: Modifier) {
        let Some(old) = self.map_raw(&slot) else {
            return;
        };
        let raw = old.with_mods(modifier.toggled(old.mods()));
        self.write_map_raw(slot, Some(raw));
    }

    /// The row's target in the edited map, in the editor's scope.
    fn map_raw(&mut self, slot: &Slot) -> Option<RawTarget> {
        let id = self.ui.map_edit.map_id().to_string();
        let scope = self.ui.map_edit.layer().map(str::to_string);
        let layer = scope.as_deref();
        let map = self.game_mode().maps.get(&id)?;
        match slot {
            Slot::Button(pad) => map.raw_pad(layer, *pad),
            Slot::Key(name) => map
                .raw_keys(layer)
                .find(|(key, _)| key == name)
                .map(|(_, raw)| raw),
            Slot::Stick(side) => map.raw_stick(layer, *side),
            Slot::Direction(side, dir) => map.raw_stick_dir(layer, *side, *dir),
        }
        .cloned()
    }

    /// Write one row into the edited map, and put the highlight where it landed
    /// — a source bound from the row that captures has no row until now.
    fn write_map_raw(&mut self, slot: Slot, raw: Option<RawTarget>) {
        let id = self.ui.map_edit.map_id().to_string();
        let scope = self.ui.map_edit.layer().map(str::to_string);
        let layer = scope.as_deref();
        if let Some(map) = self.game_mode().maps.get_mut(&id) {
            match &slot {
                Slot::Button(pad) => map.set_raw_pad(layer, *pad, raw),
                Slot::Key(name) => map.set_raw_key(layer, name, raw),
                Slot::Stick(side) => map.set_raw_stick(layer, *side, raw),
                Slot::Direction(side, dir) => map.set_raw_stick_dir(layer, *side, *dir, raw),
            }
        }
        self.edited_input_map();
        self.ui.map_edit.select_slot(&slot);
    }

    /// Make `slot` the one base source that holds the scoped layer open, or
    /// leave nothing holding it.
    fn set_activator(&mut self, slot: Option<Slot>) {
        let id = self.ui.map_edit.map_id().to_string();
        let Some(layer) = self.ui.map_edit.layer().map(str::to_string) else {
            return;
        };
        let Some(map) = self.game_mode().maps.get_mut(&id) else {
            return;
        };
        map.clear_raw_activators(&layer);
        let target = Some(RawTarget::Short(format!("{LAYER_PREFIX}{layer}")));
        match slot {
            Some(Slot::Button(pad)) => map.set_raw_pad(None, pad, target),
            Some(Slot::Key(name)) => map.set_raw_key(None, &name, target),
            Some(Slot::Direction(side, dir)) => map.set_raw_stick_dir(None, side, dir, target),
            // Refused at the capture: a whole stick sends a vector, not an edge.
            Some(Slot::Stick(_)) | None => {}
        }
        self.edited_input_map();
    }

    /// Hand a stick its four directions (see [`Take::Arrows`]).
    fn set_map_arrows(&mut self, slot: Slot) {
        let Slot::Stick(side) = slot else {
            return;
        };
        let id = self.ui.map_edit.map_id().to_string();
        let scope = self.ui.map_edit.layer().map(str::to_string);
        if let Some(map) = self.game_mode().maps.get_mut(&id) {
            map.set_raw_stick_arrows(scope.as_deref(), side);
        }
        self.edited_input_map();
    }

    fn edited_input_map(&mut self) {
        self.ui.map_edit.mark_dirty();
        self.refresh_map_edit();
    }

    /// Re-snapshot the editor's rows from the map it has open, in its scope. A
    /// stale id keeps the last snapshot: writes to it are already no-ops.
    fn refresh_map_edit(&mut self) {
        let id = self.ui.map_edit.map_id().to_string();
        let scope = self.ui.map_edit.layer().map(str::to_string);
        let layer = scope.as_deref();
        let Some(map) = self.game_mode().maps.get(&id) else {
            return;
        };
        let text = |raw: Option<&RawTarget>| match raw {
            Some(raw) => raw.text().to_string(),
            None => UNBOUND.to_string(),
        };
        let mods = |raw: Option<&RawTarget>| raw.map(RawTarget::mods).unwrap_or_default();
        let speed = |raw: Option<&RawTarget>| raw.and_then(RawTarget::speed);
        let mut rows = vec![];
        // The sticks lead, each a row only while the map binds it.
        for side in Side::ALL {
            let digital = map.raw_stick_is_digital(layer, side);
            if !digital && map.raw_stick(layer, side).is_none() {
                continue;
            }
            rows.push(Row {
                slot: Slot::Stick(side),
                // A stick read as directions has no whole-stick entry to show,
                // so the row says what it has become instead.
                target: match digital {
                    true => DIRECTIONS.to_string(),
                    false => text(map.raw_stick(layer, side)),
                },
                mods: KeyMods::default(),
                speed: speed(map.raw_stick(layer, side)),
            });
            if digital {
                rows.extend(Dir::ALL.map(|dir| Row {
                    slot: Slot::Direction(side, dir),
                    target: text(map.raw_stick_dir(layer, side, dir)),
                    mods: mods(map.raw_stick_dir(layer, side, dir)),
                    speed: None,
                }));
            }
        }
        // Then what the map binds: the pad in the pad's own order, the keys in
        // the file's. No button is held back, or one the file spells could not
        // be taken back out.
        rows.extend(Pad::ALL.into_iter().filter_map(|pad| {
            map.raw_pad(layer, pad).map(|raw| Row {
                slot: Slot::Button(pad),
                target: raw.text().to_string(),
                mods: raw.mods(),
                speed: None,
            })
        }));
        rows.extend(map.raw_keys(layer).map(|(name, raw)| Row {
            slot: Slot::Key(name.to_string()),
            target: raw.text().to_string(),
            mods: raw.mods(),
            speed: None,
        }));
        let layers = map
            .raw_layers()
            .map(|name| LayerRow {
                name: name.to_string(),
                held_by: map
                    .raw_activators(name)
                    .iter()
                    .filter_map(|path| Slot::from_path(path))
                    .collect(),
                has_sticks: map.raw_layer_has_sticks(name),
            })
            .collect();
        self.ui.map_edit.set_rows(rows, layers);
    }

    /// Adopt the map the config names, for a settings restore. The config
    /// is the source of truth here, so nothing is written back.
    pub(super) fn adopt_input_map(&mut self, out: &mut Vec<AppCommand>) {
        let id = self.config.game_mode.input_map.clone();
        // Every settings save comes through here, and a mode that has not loaded
        // takes the config's map when it does: this must not be what loads it.
        let Some(game) = self.event_handler.game() else {
            return;
        };
        game.use_map(&id, &self.browser, out);
        self.refresh_input_maps();
    }
}

/// `text` trimmed, with a number after it where another name has it already.
fn unique_name(text: &str, taken: &[String]) -> String {
    let base = match text.trim() {
        "" => NEW_LAYER_NAME,
        trimmed => trimmed,
    };
    let free = |name: &str| !taken.iter().any(|t| t == name);
    if free(base) {
        return base.to_string();
    }
    (2..)
        .map(|n| format!("{base}-{n}"))
        .find(|name| free(name))
        .expect("the integers outnumber the layers")
}
