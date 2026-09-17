//! Game Mode's input maps, as two screens reached from its menu: the list of
//! what this run offers, and what one map can be told to do — use it, edit
//! its bindings, rename it, copy it, or throw the file away. The device this
//! is for has no file manager, so a map it cannot add here is one it cannot
//! add at all.
//!
//! The central router ([`crate::app`]) drives them; [`crate::ui`] renders them.

/// One map as the list shows it — a snapshot, since the maps themselves live
/// in the event handler that resolved them.
pub struct MapRow {
    pub id: String,
    pub name: String,
    /// Whether the mode runs this one.
    pub in_use: bool,
    /// The row that takes the file away, if it has one: a built-in no file
    /// shadows has nothing to delete.
    pub remove: Option<MapAction>,
}

/// What one map's screen offers, top to bottom.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MapAction {
    /// Hand this map to the mode, and make it the one it starts with.
    Use,
    /// Open the editor ([`super::map_edit`]) on it.
    Edit,
    Rename,
    /// Copy it under a new name, bindings and all — the list's New row starts
    /// from nothing instead.
    Duplicate,
    /// Delete the file: the map goes with it.
    Delete,
    /// Delete the file of a built-in: the binary's own text comes back.
    Reset,
}

impl MapAction {
    /// A trailing ellipsis means the row asks for something before it does
    /// anything — the two that summon the keyboard for a name. A row that only
    /// opens another screen does not get one.
    pub fn label(self) -> &'static str {
        match self {
            MapAction::Use => "Use this map",
            MapAction::Edit => "Edit",
            MapAction::Rename => "Rename...",
            MapAction::Duplicate => "Duplicate...",
            MapAction::Delete => "Delete",
            MapAction::Reset => "Reset to default",
        }
    }
}

/// What a name being typed is for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum NameFor {
    /// Rename the map whose screen is open.
    Rename,
    /// Copy that map under the typed name.
    Duplicate,
    /// Make a map, from the list rather than from any open one.
    New,
}

/// A name being typed, and what it is for.
pub struct Naming {
    pub what: NameFor,
    pub text: String,
}

/// The list's first row, which makes a map. The ellipsis promises the keyboard,
/// like the two [`MapAction`] rows that ask for a name.
pub const NEW_MAP_LABEL: &str = "New map...";

/// What that keyboard starts from, so a press through it still names something.
pub const NEW_MAP_NAME: &str = "New map";

/// What **A** does, given which of the three lists the highlight is in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Press {
    /// Make a map: the list's first row.
    New,
    /// Open the highlighted map's own screen.
    Open,
    /// Take the highlighted action on the map that is open.
    Take(MapAction),
    /// Answer the confirmation: `true` removes.
    Confirm(bool),
}

/// The confirmation's rows, in order — Cancel second, and where the highlight
/// starts.
const CONFIRM_ROWS: usize = 2;
const CANCEL_ROW: usize = 1;

/// The list shows [`NEW_MAP_LABEL`] first, so the maps start one row below
/// their index in `rows`.
const MAPS_START: usize = 1;

pub struct InputMaps {
    visible: bool,
    rows: Vec<MapRow>,
    /// Where the highlight is in the list as drawn, the New row included.
    selected: usize,
    /// The map whose own screen is open, as an index into `rows`.
    open: Option<usize>,
    /// Where the highlight is on that screen.
    at: usize,
    /// The confirmation over a removal, once one is asked for.
    confirm: Option<usize>,
    /// The name being typed, while the keyboard is up to type it.
    naming: Option<Naming>,
}

impl InputMaps {
    pub fn new() -> Self {
        Self {
            visible: false,
            rows: Vec::new(),
            selected: 0,
            open: None,
            at: 0,
            confirm: None,
            naming: None,
        }
    }

    pub fn visible(&self) -> bool {
        self.visible
    }

    /// Show the list.
    pub fn open(&mut self) {
        self.visible = true;
        self.open = None;
        self.confirm = None;
        self.naming = None;
        self.selected = self.selected.min(self.last_row());
    }

    /// The last index the list offers — the New row when there are no maps.
    fn last_row(&self) -> usize {
        self.rows.len()
    }

    /// Which map the list's highlight is on, or `None` on the New row.
    fn selected_map(&self) -> Option<usize> {
        self.selected.checked_sub(MAPS_START)
    }

    /// Show one map's own screen, if it is still there — after a deletion
    /// it is not, and the list is what is left to show.
    pub fn open_id(&mut self, id: &str) -> bool {
        let Some(index) = self.rows.iter().position(|row| row.id == id) else {
            self.open();
            return false;
        };
        self.visible = true;
        self.selected = index + MAPS_START;
        self.open = Some(index);
        self.at = 0;
        self.confirm = None;
        self.naming = None;
        true
    }

    pub fn close(&mut self) {
        self.visible = false;
        self.confirm = None;
        self.naming = None;
    }

    /// Back out one level — the confirmation, then the map, then the list,
    /// which closes. `false` once nothing is left, so the caller can show
    /// whatever opened this.
    pub fn back(&mut self) -> bool {
        if self.confirm.take().is_some() {
            return true;
        }
        if self.open.take().is_some() {
            self.at = 0;
            return true;
        }
        self.close();
        false
    }

    /// Adopt a fresh snapshot of the maps (after anything changed one).
    pub fn set_rows(&mut self, rows: Vec<MapRow>) {
        self.rows = rows;
        self.selected = self.selected.min(self.last_row());
        self.open = self.open.filter(|at| *at < self.rows.len());
        self.at = self.at.min(self.actions().len().saturating_sub(1));
    }

    pub fn rows(&self) -> &[MapRow] {
        &self.rows
    }

    /// The map whose screen is open.
    pub fn open_row(&self) -> Option<&MapRow> {
        self.open.and_then(|at| self.rows.get(at))
    }

    pub fn open_id_str(&self) -> Option<&str> {
        self.open_row().map(|row| row.id.as_str())
    }

    /// What the open map offers. `Use` is absent on the one already
    /// running: a row that would do nothing is a row that is not there.
    pub fn actions(&self) -> Vec<MapAction> {
        let Some(row) = self.open_row() else {
            return Vec::new();
        };
        let mut actions = Vec::new();
        if !row.in_use {
            actions.push(MapAction::Use);
        }
        actions.extend([MapAction::Edit, MapAction::Rename, MapAction::Duplicate]);
        actions.extend(row.remove);
        actions
    }

    /// Where the highlight is on whichever list is up.
    pub fn selected(&self) -> usize {
        match (self.confirm, self.open) {
            (Some(at), _) => at,
            (None, Some(_)) => self.at,
            (None, None) => self.selected,
        }
    }

    /// Move it, clamped to the ends of that list.
    pub fn move_sel(&mut self, dy: i32) {
        let last = self.level_len() as i32 - 1;
        let at = (self.selected() as i32 + dy).clamp(0, last.max(0)) as usize;
        match (&mut self.confirm, self.open) {
            (Some(slot), _) => *slot = at,
            (None, Some(_)) => self.at = at,
            (None, None) => self.selected = at,
        }
    }

    /// Focus a row by index (clicking it).
    pub fn select(&mut self, index: usize) {
        if index < self.level_len() {
            self.move_sel(index as i32 - self.selected() as i32);
        }
    }

    fn level_len(&self) -> usize {
        match (self.confirm, self.open) {
            (Some(_), _) => CONFIRM_ROWS,
            (None, Some(_)) => self.actions().len(),
            (None, None) => self.last_row() + 1,
        }
    }

    /// What **A** takes. The list always has the New row, so only a map's own
    /// screen can come up empty.
    pub fn press(&self) -> Option<Press> {
        if let Some(at) = self.confirm {
            return Some(Press::Confirm(at != CANCEL_ROW));
        }
        if self.open.is_some() {
            return self.actions().get(self.at).copied().map(Press::Take);
        }
        let Some(at) = self.selected_map() else {
            return Some(Press::New);
        };
        self.rows.get(at).map(|_| Press::Open)
    }

    /// Open the highlighted map's own screen.
    pub fn open_selected(&mut self) {
        if let Some(at) = self.selected_map().filter(|at| *at < self.rows.len()) {
            self.open = Some(at);
            self.at = 0;
        }
    }

    /// Ask before removing, with the highlight on Cancel — a press that came
    /// one row too far must not throw a map away.
    pub fn ask_confirm(&mut self) {
        self.confirm = Some(CANCEL_ROW);
    }

    pub fn close_confirm(&mut self) {
        self.confirm = None;
    }

    /// Whether the confirmation is up, for the screen that draws it.
    pub fn confirming(&self) -> bool {
        self.confirm.is_some()
    }

    /// Hand the keyboard a name to edit (a rename, a copy's, or a new map's).
    pub fn start_naming(&mut self, what: NameFor, text: String) {
        self.naming = Some(Naming { what, text });
    }

    pub fn naming(&self) -> Option<&Naming> {
        self.naming.as_ref()
    }

    /// The buffer the on-screen keyboard types into.
    pub fn naming_text_mut(&mut self) -> Option<&mut String> {
        self.naming.as_mut().map(|naming| &mut naming.text)
    }

    /// Take what was typed, if the keyboard was submitted rather than dropped.
    pub fn take_naming(&mut self) -> Option<Naming> {
        self.naming.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows() -> Vec<MapRow> {
        vec![
            MapRow {
                id: "keys".to_string(),
                name: "Keyboard keys".to_string(),
                in_use: true,
                remove: None,
            },
            MapRow {
                id: "pad".to_string(),
                name: "Gamepad".to_string(),
                in_use: false,
                remove: Some(MapAction::Reset),
            },
            MapRow {
                id: "my-game".to_string(),
                name: "My game".to_string(),
                in_use: false,
                remove: Some(MapAction::Delete),
            },
        ]
    }

    fn opened(id: &str) -> InputMaps {
        let mut screens = InputMaps::new();
        screens.set_rows(rows());
        screens.open();
        assert!(screens.open_id(id));
        screens
    }

    /// The rows a map offers follow what it is: nothing to switch to on the
    /// one running, and nothing to delete where there is no file.
    #[test]
    fn a_map_offers_what_it_can_actually_do() {
        assert_eq!(
            opened("keys").actions(),
            [MapAction::Edit, MapAction::Rename, MapAction::Duplicate]
        );
        assert_eq!(
            opened("pad").actions(),
            [
                MapAction::Use,
                MapAction::Edit,
                MapAction::Rename,
                MapAction::Duplicate,
                MapAction::Reset
            ]
        );
        assert_eq!(
            *opened("my-game").actions().last().unwrap(),
            MapAction::Delete
        );
    }

    /// The ellipsis is a promise that the row will ask for something before it
    /// acts, so only the two that summon the keyboard may make it.
    #[test]
    fn only_a_row_that_asks_for_a_name_trails_off() {
        let all = [
            MapAction::Use,
            MapAction::Edit,
            MapAction::Rename,
            MapAction::Duplicate,
            MapAction::Delete,
            MapAction::Reset,
        ];
        for action in all {
            let asks = matches!(action, MapAction::Rename | MapAction::Duplicate);
            assert_eq!(action.label().ends_with("..."), asks, "{action:?}");
        }
        // The list's own row makes the same promise, and keeps it.
        assert!(NEW_MAP_LABEL.ends_with("..."));
    }

    /// The list leads with the row that makes a map, so it is what a press
    /// takes before the highlight has moved — and with no maps at all, the
    /// only thing there is to press.
    #[test]
    fn the_list_leads_with_the_row_that_makes_a_map() {
        let mut screens = InputMaps::new();
        screens.set_rows(rows());
        screens.open();
        assert_eq!(screens.press(), Some(Press::New));
        // It is a row of its own: pressing it opens no map's screen.
        screens.open_selected();
        assert!(screens.open_row().is_none());
        screens.move_sel(1);
        assert_eq!(screens.press(), Some(Press::Open));
        screens.open_selected();
        assert_eq!(screens.open_row().map(|row| row.id.as_str()), Some("keys"));

        screens.back();
        screens.set_rows(Vec::new());
        assert_eq!(screens.press(), Some(Press::New));
    }

    /// B walks back out one screen at a time, and says when there is nothing
    /// left — that is what hands the menu back.
    #[test]
    fn back_pops_one_level_at_a_time() {
        let mut screens = opened("my-game");
        screens.ask_confirm();
        assert!(screens.back() && screens.confirm.is_none());
        assert!(screens.back() && screens.open_row().is_none());
        assert!(!screens.back());
        assert!(!screens.visible());
    }

    /// Neither removal can be undone here, so the press that answers has to
    /// land on Cancel.
    #[test]
    fn the_confirmation_starts_on_cancel() {
        let mut screens = opened("my-game");
        screens.ask_confirm();
        assert_eq!(screens.press(), Some(Press::Confirm(false)));
        screens.move_sel(-1);
        assert_eq!(screens.press(), Some(Press::Confirm(true)));
        // It is its own list: the map's rows stay where they were.
        screens.move_sel(9);
        assert_eq!(screens.press(), Some(Press::Confirm(false)));
        screens.back();
        assert_eq!(screens.press(), Some(Press::Take(MapAction::Use)));
    }

    /// The list is what the highlight moves through until a map is opened.
    #[test]
    fn the_highlight_stops_at_the_ends_of_whichever_list_is_up() {
        let mut screens = InputMaps::new();
        screens.set_rows(rows());
        screens.open();
        screens.move_sel(-1);
        assert_eq!(screens.selected(), 0);
        screens.move_sel(99);
        // The New row sits above the maps, so the last index is their count.
        assert_eq!(screens.selected(), rows().len());
        screens.open_selected();
        screens.move_sel(99);
        assert_eq!(screens.press(), Some(Press::Take(MapAction::Delete)));
    }

    /// A deleted map has no screen to go back to, so asking for one lands
    /// on the list rather than on a stale row.
    #[test]
    fn a_map_that_is_gone_falls_back_to_the_list() {
        let mut screens = opened("my-game");
        let left: Vec<MapRow> = rows().into_iter().filter(|r| r.id != "my-game").collect();
        screens.set_rows(left);
        assert!(!screens.open_id("my-game"));
        assert_eq!(screens.press(), Some(Press::Open));
    }
}
