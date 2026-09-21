//! Game Mode's input machinery: the [`input_map`] entity (what each source
//! sends to the page), the [`map_library`] repository of this run's maps, and
//! the [`mode`] translator that applies the active one. [`GameMode`] is the
//! three together, which is how the event handler holds them.

pub mod input_map;
pub mod map_library;
pub mod mode;

use crate::browser::AppBrowser;
use crate::command::AppCommand;
use crate::config::InputConfig;
use crate::event::key_names;
use map_library::MapLibrary;
use mode::GameInput;

/// Everything Game Mode owns. Loaded when a screen or the mode itself asks for
/// a map and dropped once none of them is up, so a run that stays in the browser
/// holds no map at all. The live map's id survives in `[game_mode] input_map`.
pub struct GameMode {
    /// The maps this run offers.
    pub maps: MapLibrary,
    /// Which of them the mode runs.
    live: String,
    /// The translator, for exactly as long as the pad goes to the game: this
    /// being `Some` *is* the mode routing, and dropping it is what guarantees
    /// the page is left holding nothing.
    input: Option<GameInput>,
}

impl GameMode {
    /// Load the library and take the map `id` names, or the first where nothing
    /// answers to it — an edited config is never a dead mode.
    pub fn load(id: &str) -> Self {
        let maps = MapLibrary::load(&key_names());
        let live = maps.pick(id).id.clone();
        if live != id {
            log::warn!("input map: no `{id}`; using `{live}`");
        }
        Self {
            maps,
            live,
            input: None,
        }
    }

    /// The live map's id and name, as the menu shows them.
    pub fn live(&self) -> (String, String) {
        let map = self.maps.pick(&self.live);
        (map.id.clone(), map.name.clone())
    }

    /// The translator while the pad goes to the game.
    pub fn routing(&mut self) -> Option<&mut GameInput> {
        self.input.as_mut()
    }

    pub fn is_routing(&self) -> bool {
        self.input.is_some()
    }

    /// Start routing: the live map becomes a translator.
    pub fn start_routing(&mut self, cfg: &InputConfig) {
        self.input = Some(GameInput::new(self.maps.pick(&self.live).clone(), cfg));
    }

    /// Stop, releasing what the page holds on the way out — a transition must
    /// never leave it with a stuck key or a stuck click.
    pub fn stop_routing(&mut self, browser: &AppBrowser, commands: &mut Vec<AppCommand>) {
        if let Some(mut input) = self.input.take() {
            input.release(browser, commands);
        }
    }

    /// Retune a running translator; one not running reads the config when it
    /// starts.
    pub fn set_config(&mut self, cfg: &InputConfig) {
        if let Some(input) = &mut self.input {
            input.set_config(cfg);
        }
    }

    /// Write an edited map to its file (see [`MapLibrary::save`]) and re-adopt
    /// it if it is the one running. Returns its name.
    pub fn save(
        &mut self,
        id: &str,
        browser: &AppBrowser,
        commands: &mut Vec<AppCommand>,
    ) -> String {
        let Some(name) = self.maps.save(id, &key_names()) else {
            return self.live().1;
        };
        if id == self.live {
            self.adopt(browser, commands);
        }
        name
    }

    /// Rename what the menu shows and write it.
    pub fn rename(
        &mut self,
        id: &str,
        name: String,
        browser: &AppBrowser,
        commands: &mut Vec<AppCommand>,
    ) {
        self.maps.set_name(id, name);
        self.save(id, browser, commands);
    }

    pub fn duplicate(&mut self, id: &str, name: String) -> String {
        self.maps.duplicate(id, name, &key_names())
    }

    pub fn add(&mut self, name: String) -> String {
        self.maps.add_passthrough(name, &key_names())
    }

    /// Delete a map (see [`MapLibrary::delete`]). The mode cannot run what is
    /// no longer there, so it takes the first map instead; returns what it
    /// runs now.
    pub fn delete(
        &mut self,
        id: &str,
        browser: &AppBrowser,
        commands: &mut Vec<AppCommand>,
    ) -> (String, String) {
        if self.maps.delete(id, &key_names()) && id == self.live {
            self.use_map(id, browser, commands);
        }
        self.live()
    }

    /// Run the map `id` names, or the first where nothing answers to it. What
    /// the page holds under the old one is released first.
    pub fn use_map(
        &mut self,
        id: &str,
        browser: &AppBrowser,
        commands: &mut Vec<AppCommand>,
    ) -> (String, String) {
        self.live = self.maps.pick(id).id.clone();
        self.adopt(browser, commands);
        self.live()
    }

    /// Hand a running translator the live map, so a switch or an edit takes
    /// effect without leaving the mode.
    fn adopt(&mut self, browser: &AppBrowser, commands: &mut Vec<AppCommand>) {
        let map = self.maps.pick(&self.live).clone();
        if let Some(input) = &mut self.input {
            input.set_map(map, browser, commands);
        }
    }
}
