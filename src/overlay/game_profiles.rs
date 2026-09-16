//! Game Mode's profiles, as two screens reached from its menu: the list of
//! what this run offers, and what one profile can be told to do — use it, edit
//! its buttons, rename it, copy it, or throw the file away. The device this is
//! for has no file manager, so a profile it cannot add here is one it cannot
//! add at all.
//!
//! The central router ([`crate::app`]) drives them; [`crate::ui`] renders them.
//!
//! On screen they are *input* profiles — one maps both devices — while the code
//! says `game_`, which is the namespace every Game Mode module shares and the
//! one `input` cannot be: that is already the config section for the pad.

/// One profile as the list shows it — a snapshot, since the profiles
/// themselves live in the event handler that resolved them.
pub struct ProfileRow {
    pub id: String,
    pub name: String,
    /// Whether the mode runs this one.
    pub in_use: bool,
    /// The row that takes the file away, if it has one: a built-in no file
    /// shadows has nothing to delete.
    pub remove: Option<ProfileAction>,
}

/// What one profile's screen offers, top to bottom.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ProfileAction {
    /// Hand this profile to the mode, and make it the one it starts with.
    Use,
    /// Open the button editor ([`super::game_edit`]) on it.
    Buttons,
    Rename,
    /// Copy it under a new name — the only way to add a profile, since an
    /// empty one would reach the page with no cursor and no click.
    Duplicate,
    /// Delete the file: the profile goes with it.
    Delete,
    /// Delete the file of a built-in: the binary's own text comes back.
    Reset,
}

impl ProfileAction {
    /// A trailing ellipsis means the row asks for something before it does
    /// anything — the two that summon the keyboard for a name. A row that only
    /// opens another screen does not get one.
    pub fn label(self) -> &'static str {
        match self {
            ProfileAction::Use => "Use this profile",
            ProfileAction::Buttons => "Buttons",
            ProfileAction::Rename => "Rename...",
            ProfileAction::Duplicate => "Duplicate...",
            ProfileAction::Delete => "Delete",
            ProfileAction::Reset => "Reset to default",
        }
    }
}

/// A name being typed, and what it is for.
pub struct Naming {
    /// Whether the name makes a copy rather than renaming what is open.
    pub copy: bool,
    pub text: String,
}

/// What **A** does, given which of the three lists the highlight is in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Press {
    /// Open the highlighted profile's own screen.
    Open,
    /// Take the highlighted action on the profile that is open.
    Take(ProfileAction),
    /// Answer the confirmation: `true` removes.
    Confirm(bool),
}

/// The confirmation's rows, in order — Cancel second, and where the highlight
/// starts.
const CONFIRM_ROWS: usize = 2;
const CANCEL_ROW: usize = 1;

pub struct GameProfiles {
    visible: bool,
    rows: Vec<ProfileRow>,
    selected: usize,
    /// The profile whose own screen is open, as an index into `rows`.
    open: Option<usize>,
    /// Where the highlight is on that screen.
    at: usize,
    /// The confirmation over a removal, once one is asked for.
    confirm: Option<usize>,
    /// The name being typed, while the keyboard is up to type it.
    naming: Option<Naming>,
}

impl GameProfiles {
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
        self.selected = self.selected.min(self.rows.len().saturating_sub(1));
    }

    /// Show one profile's own screen, if it is still there — after a deletion
    /// it is not, and the list is what is left to show.
    pub fn open_id(&mut self, id: &str) -> bool {
        let Some(index) = self.rows.iter().position(|row| row.id == id) else {
            self.open();
            return false;
        };
        self.visible = true;
        self.selected = index;
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

    /// Back out one level — the confirmation, then the profile, then the list,
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

    /// Adopt a fresh snapshot of the profiles (after anything changed one).
    pub fn set_rows(&mut self, rows: Vec<ProfileRow>) {
        self.rows = rows;
        let last = self.rows.len().saturating_sub(1);
        self.selected = self.selected.min(last);
        self.open = self.open.filter(|at| *at < self.rows.len());
        self.at = self.at.min(self.actions().len().saturating_sub(1));
    }

    pub fn rows(&self) -> &[ProfileRow] {
        &self.rows
    }

    /// The profile whose screen is open.
    pub fn open_row(&self) -> Option<&ProfileRow> {
        self.open.and_then(|at| self.rows.get(at))
    }

    pub fn open_id_str(&self) -> Option<&str> {
        self.open_row().map(|row| row.id.as_str())
    }

    /// What the open profile offers. `Use` is absent on the one already
    /// running: a row that would do nothing is a row that is not there.
    pub fn actions(&self) -> Vec<ProfileAction> {
        let Some(row) = self.open_row() else {
            return Vec::new();
        };
        let mut actions = Vec::new();
        if !row.in_use {
            actions.push(ProfileAction::Use);
        }
        actions.extend([
            ProfileAction::Buttons,
            ProfileAction::Rename,
            ProfileAction::Duplicate,
        ]);
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
            (None, None) => self.rows.len(),
        }
    }

    /// What **A** takes, or `None` on a list with nothing in it.
    pub fn press(&self) -> Option<Press> {
        if let Some(at) = self.confirm {
            return Some(Press::Confirm(at != CANCEL_ROW));
        }
        if self.open.is_some() {
            return self.actions().get(self.at).copied().map(Press::Take);
        }
        self.rows.get(self.selected).map(|_| Press::Open)
    }

    /// Open the highlighted profile's own screen.
    pub fn open_selected(&mut self) {
        if self.selected < self.rows.len() {
            self.open = Some(self.selected);
            self.at = 0;
        }
    }

    /// Ask before removing, with the highlight on Cancel — a press that came
    /// one row too far must not throw a profile away.
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

    /// Hand the keyboard a name to edit (a rename, or a copy's).
    pub fn start_naming(&mut self, copy: bool, text: String) {
        self.naming = Some(Naming { copy, text });
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

    fn rows() -> Vec<ProfileRow> {
        vec![
            ProfileRow {
                id: "keys".to_string(),
                name: "Keyboard keys".to_string(),
                in_use: true,
                remove: None,
            },
            ProfileRow {
                id: "pad".to_string(),
                name: "Gamepad".to_string(),
                in_use: false,
                remove: Some(ProfileAction::Reset),
            },
            ProfileRow {
                id: "my-game".to_string(),
                name: "My game".to_string(),
                in_use: false,
                remove: Some(ProfileAction::Delete),
            },
        ]
    }

    fn opened(id: &str) -> GameProfiles {
        let mut screens = GameProfiles::new();
        screens.set_rows(rows());
        screens.open();
        assert!(screens.open_id(id));
        screens
    }

    /// The rows a profile offers follow what it is: nothing to switch to on the
    /// one running, and nothing to delete where there is no file.
    #[test]
    fn a_profile_offers_what_it_can_actually_do() {
        assert_eq!(
            opened("keys").actions(),
            [
                ProfileAction::Buttons,
                ProfileAction::Rename,
                ProfileAction::Duplicate
            ]
        );
        assert_eq!(
            opened("pad").actions(),
            [
                ProfileAction::Use,
                ProfileAction::Buttons,
                ProfileAction::Rename,
                ProfileAction::Duplicate,
                ProfileAction::Reset
            ]
        );
        assert_eq!(
            *opened("my-game").actions().last().unwrap(),
            ProfileAction::Delete
        );
    }

    /// The ellipsis is a promise that the row will ask for something before it
    /// acts, so only the two that summon the keyboard may make it.
    #[test]
    fn only_a_row_that_asks_for_a_name_trails_off() {
        let all = [
            ProfileAction::Use,
            ProfileAction::Buttons,
            ProfileAction::Rename,
            ProfileAction::Duplicate,
            ProfileAction::Delete,
            ProfileAction::Reset,
        ];
        for action in all {
            let asks = matches!(action, ProfileAction::Rename | ProfileAction::Duplicate);
            assert_eq!(action.label().ends_with("..."), asks, "{action:?}");
        }
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
        // It is its own list: the profile's rows stay where they were.
        screens.move_sel(9);
        assert_eq!(screens.press(), Some(Press::Confirm(false)));
        screens.back();
        assert_eq!(screens.press(), Some(Press::Take(ProfileAction::Use)));
    }

    /// The list is what the highlight moves through until a profile is opened.
    #[test]
    fn the_highlight_stops_at_the_ends_of_whichever_list_is_up() {
        let mut screens = GameProfiles::new();
        screens.set_rows(rows());
        screens.open();
        screens.move_sel(-1);
        assert_eq!(screens.selected(), 0);
        screens.move_sel(99);
        assert_eq!(screens.selected(), rows().len() - 1);
        screens.open_selected();
        screens.move_sel(99);
        assert_eq!(screens.press(), Some(Press::Take(ProfileAction::Delete)));
    }

    /// A deleted profile has no screen to go back to, so asking for one lands
    /// on the list rather than on a stale row.
    #[test]
    fn a_profile_that_is_gone_falls_back_to_the_list() {
        let mut screens = opened("my-game");
        let left: Vec<ProfileRow> = rows().into_iter().filter(|r| r.id != "my-game").collect();
        screens.set_rows(left);
        assert!(!screens.open_id("my-game"));
        assert_eq!(screens.press(), Some(Press::Open));
    }
}
