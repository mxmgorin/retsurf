//! The input maps this run offers: the repository the mode's menu lists, the
//! editor writes, and the running mode adopts from. Files and built-ins are
//! [`input_map`]'s; which map runs live stays the event handler's.

use super::input_map::{self, InputMap};
use inputbind::sdl::KeyNames;

pub struct MapLibrary {
    maps: Vec<InputMap>,
}

impl MapLibrary {
    pub fn load(keys: &KeyNames) -> Self {
        Self {
            maps: input_map::load_all(keys),
        }
    }

    /// Every map, in the order the mode's menu cycles them.
    pub fn all(&self) -> &[InputMap] {
        &self.maps
    }

    /// The map `id` names. `None` for a stale id, matching [`Self::get_mut`].
    pub fn get(&self, id: &str) -> Option<&InputMap> {
        self.maps.iter().find(|m| m.id == id)
    }

    /// The same, to write one row of it (the editor).
    pub fn get_mut(&mut self, id: &str) -> Option<&mut InputMap> {
        self.maps.iter_mut().find(|m| m.id == id)
    }

    /// The lenient read for the runtime path: an id nothing answers to falls
    /// back to the first map, so an edited config is never a dead mode.
    pub fn pick(&self, id: &str) -> &InputMap {
        input_map::pick(&self.maps, id)
    }

    /// Write `id`'s edits to its file; returns its name, `None` for a stale id.
    pub fn save(&mut self, id: &str, keys: &KeyNames) -> Option<String> {
        let at = self.maps.iter().position(|m| m.id == id)?;
        self.maps[at] = self.maps[at].save(keys);
        Some(self.maps[at].name.clone())
    }

    /// Rename what the menu shows (the write is [`Self::save`]'s). The id stays
    /// what it was: it is the file's stem, and `[game_mode] input_map` names it.
    pub fn set_name(&mut self, id: &str, name: String) {
        if let Some(map) = self.get_mut(id) {
            map.set_name(name);
        }
    }

    /// Copy `id` under a new name, as a file of its own. Returns the id it
    /// landed under — the name decides it, so a collision cannot shadow one.
    pub fn duplicate(&mut self, id: &str, name: String, keys: &KeyNames) -> String {
        let new_id = self.free_id(&name);
        let copy = self.pick(id).copy(&new_id, name, keys);
        self.maps.push(copy.save(keys));
        new_id
    }

    /// Add a map that passes the whole pad through, for the editor to bind from
    /// there. Returns the id it landed under, like a duplicate.
    pub fn add_passthrough(&mut self, name: String, keys: &KeyNames) -> String {
        let new_id = self.free_id(&name);
        let map = InputMap::passthrough(&new_id, name, keys);
        self.maps.push(map.save(keys));
        new_id
    }

    /// Delete `id`'s file: a built-in comes back as the binary carries it,
    /// anything else is gone. `false` for a stale id.
    pub fn delete(&mut self, id: &str, keys: &KeyNames) -> bool {
        let Some(at) = self.maps.iter().position(|m| m.id == id) else {
            return false;
        };
        self.maps[at].delete();
        match input_map::built_in(id, keys) {
            Some(original) => self.maps[at] = original,
            None => {
                self.maps.remove(at);
            }
        }
        true
    }

    fn free_id(&self, name: &str) -> String {
        let taken: Vec<String> = self.maps.iter().map(|m| m.id.clone()).collect();
        input_map::new_id(name, &taken)
    }
}
