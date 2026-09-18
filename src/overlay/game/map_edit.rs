//! The Game Mode map editor: one row per source, showing what it sends and
//! letting the pad itself change it. The device this is for has no keyboard and
//! no file manager, so a map it cannot edit here is a map it cannot edit
//! at all; the file stays the fuller interface ([`crate::event::game::input_map`]).
//!
//! Physical keys and layers are the file's — they need names this screen has no
//! room to pick, which is also why it is titled for the two kinds it does list.

use crate::event::game::input_map::{Dir, Side};
use inputbind::Pad;

/// A row of the top list: every button, then the two sticks.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Source {
    Button(Pad),
    Stick(Side),
}

impl Source {
    /// How the file spells it, which is also the row's label. A stick carries
    /// its table's name because the D-pad already holds `left` and `right`.
    pub fn name(self) -> String {
        match self {
            Source::Button(pad) => pad.name().to_string(),
            Source::Stick(side) => format!("stick.{}", side.name()),
        }
    }
}

/// What one row writes to.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Slot {
    Button(Pad),
    /// The stick as one vector.
    Stick(Side),
    /// One of its four directions.
    Direction(Side, Dir),
}

impl Slot {
    /// How the file spells it, for the title of the list opened over it.
    pub fn name(self) -> String {
        match self {
            Slot::Button(pad) => pad.name().to_string(),
            Slot::Stick(side) => format!("stick.{}", side.name()),
            Slot::Direction(side, dir) => format!("stick.{}.{}", side.name(), dir.name()),
        }
    }
}

/// A row of one stick's own screen.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StickRow {
    /// What the whole stick does — the row that decides whether the rest exist.
    Sends,
    Direction(Dir),
}

/// What a row can be set to. One list per slot, so nothing about a row is
/// hidden behind a verb the screen cannot show.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// Hands over to the on-screen keyboard, which picks the key itself.
    Key,
    Click,
    /// The page reads the source through the Gamepad API. An unbound one does
    /// the same thing; this is the file saying so out loud, and the two are one
    /// entry here because they are one behaviour.
    Passthrough,
    /// Consumed and dropped: the source does nothing at all.
    Ignore,
    /// Read the whole stick as four directions instead of one vector.
    Directions,
    Cursor,
    Scroll,
}

impl Kind {
    /// What this slot can be told to send. A direction is offered no
    /// Passthrough: a stick read as directions withholds the whole axis, so
    /// the page would see nothing either way.
    pub fn all(slot: Slot) -> &'static [Kind] {
        match slot {
            Slot::Button(_) => &[Kind::Key, Kind::Click, Kind::Passthrough, Kind::Ignore],
            Slot::Direction(..) => &[Kind::Key, Kind::Click, Kind::Ignore],
            Slot::Stick(_) => &[
                Kind::Directions,
                Kind::Cursor,
                Kind::Scroll,
                Kind::Passthrough,
                Kind::Ignore,
            ],
        }
    }

    /// The ellipsis is the promise of a further question, which only the key
    /// picker makes (see [`super::input_maps::MapAction::label`]).
    pub fn label(self) -> &'static str {
        match self {
            Kind::Key => "Key...",
            Kind::Click => "Left mouse button",
            Kind::Passthrough => "Passthrough",
            Kind::Ignore => "Ignore",
            Kind::Directions => "Four directions",
            Kind::Cursor => "Cursor",
            Kind::Scroll => "Scroll",
        }
    }

    /// What taking it does to the map.
    pub fn take(self) -> Take {
        match self {
            Kind::Key => Take::Key,
            Kind::Directions => Take::Arrows,
            Kind::Click => Take::Text("mouse.left"),
            Kind::Passthrough => Take::Text("passthrough"),
            Kind::Ignore => Take::Text("none"),
            Kind::Cursor => Take::Text("mouse.cursor"),
            Kind::Scroll => Take::Text("mouse.scroll"),
        }
    }
}

/// What taking a kind does — most write a target, two do something else.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Take {
    /// Write this as the target, the way the file spells it.
    Text(&'static str),
    /// Summon the on-screen keyboard to pick the key.
    Key,
    /// Turn the stick into four directions, seeded with the arrows — a stick
    /// with none set reaches the page instead, which the row would not say.
    Arrows,
}

/// What **A** does, given which of the three lists is up. Qualified because
/// the map screens have one of these too.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EditPress {
    /// Open the focused stick's own rows.
    OpenStick(Side),
    /// Open the list of what the focused row can send.
    OpenKinds,
    /// Take the kind the list is on, for the row it was opened over.
    Take(Kind, Slot),
}

/// What an unbound source reads as in the editor's rows — the one spelling,
/// shared by the snapshot builder and the renderer's fallback.
pub const UNBOUND: &str = "-";

/// What the map says, as the rows show it — a snapshot, since the map
/// lives in the event handler that resolved it.
#[derive(Default)]
pub struct Targets {
    /// By [`Pad`] index; an unbound button reads as the dash its row shows.
    pub pads: Vec<String>,
    /// Left, then right.
    pub sticks: [StickTargets; 2],
}

#[derive(Default)]
pub struct StickTargets {
    /// Whether the file reads this stick as four directions rather than one
    /// vector — which is what decides whether it has direction rows at all.
    pub digital: bool,
    /// What the whole stick does, as the row shows it.
    pub role: String,
    /// Its four directions, in [`Dir::ALL`] order.
    pub dirs: [String; 4],
}

/// Every source the editor lists: Select carries the Game Mode menu in every
/// map, so it is not one of them.
pub fn sources() -> Vec<Source> {
    Pad::ALL
        .into_iter()
        .filter(|pad| *pad != Pad::Select)
        .map(Source::Button)
        .chain(Side::ALL.into_iter().map(Source::Stick))
        .collect()
}

pub struct MapEdit {
    visible: bool,
    /// The map being edited — not necessarily the one Game Mode runs, so a
    /// mapping can be set up without disturbing a game already under way.
    map: String,
    /// Its name, for the panel's title.
    name: String,
    /// The top list's highlight.
    selected: usize,
    /// The stick whose own rows are up, and where its highlight is.
    stick: Option<(Side, usize)>,
    /// The kind list over the focused row; `None` while the rows are.
    kind: Option<usize>,
    /// The row the on-screen keyboard is picking a key for.
    picking: Option<Slot>,
    /// What it picked, as the file spells it — read and cleared by the app.
    picked: Option<String>,
    /// Whether anything changed, so an untouched visit writes no file.
    dirty: bool,
    targets: Targets,
}

impl MapEdit {
    pub fn new() -> Self {
        Self {
            visible: false,
            map: String::new(),
            name: String::new(),
            selected: 0,
            stick: None,
            kind: None,
            picking: None,
            picked: None,
            dirty: false,
            targets: Targets::default(),
        }
    }

    pub fn visible(&self) -> bool {
        self.visible
    }

    /// The map whose sources these rows are.
    pub fn map_id(&self) -> &str {
        &self.map
    }

    pub fn map_name(&self) -> &str {
        &self.name
    }

    pub fn open(&mut self, map: String, name: String) {
        self.visible = true;
        self.map = map;
        self.name = name;
        self.selected = 0;
        self.stick = None;
        self.kind = None;
        self.picking = None;
        self.picked = None;
        self.dirty = false;
    }

    /// Close it, reporting whether the map needs writing.
    pub fn close(&mut self) -> bool {
        self.visible = false;
        self.stick = None;
        self.kind = None;
        self.picking = None;
        std::mem::take(&mut self.dirty)
    }

    /// Back out one list — the kinds, then a stick's rows, then the editor
    /// itself. `false` once nothing is left, which is when it closes.
    pub fn back(&mut self) -> bool {
        if self.kind.take().is_some() {
            return true;
        }
        self.stick.take().is_some()
    }

    /// Adopt a fresh snapshot of the map (after anything changed it).
    pub fn set_targets(&mut self, targets: Targets) {
        self.targets = targets;
        let last = self.rows().saturating_sub(1);
        match &mut self.stick {
            Some((_, at)) => *at = (*at).min(last),
            None => self.selected = self.selected.min(last),
        }
    }

    pub fn targets(&self) -> &Targets {
        &self.targets
    }

    /// The stick whose rows are up, if one is.
    pub fn stick_open(&self) -> Option<Side> {
        self.stick.map(|(side, _)| side)
    }

    /// That stick's rows: what the whole stick does, and its four directions
    /// where it has any — a row that could not be read is a row that is not
    /// there.
    pub fn stick_rows(&self) -> Vec<StickRow> {
        let mut rows = vec![StickRow::Sends];
        if let Some(side) = self.stick_open() {
            if self.targets.sticks[side as usize].digital {
                rows.extend(Dir::ALL.map(StickRow::Direction));
            }
        }
        rows
    }

    /// Where the highlight is on whichever list is up.
    pub fn selected(&self) -> usize {
        match (self.kind, self.stick) {
            (Some(at), _) => at,
            (None, Some((_, at))) => at,
            (None, None) => self.selected,
        }
    }

    /// Move it, clamped to the ends of that list.
    pub fn move_sel(&mut self, dy: i32) {
        let at = crate::list::step(self.selected(), dy, self.rows());
        match (&mut self.kind, &mut self.stick) {
            (Some(slot), _) => *slot = at,
            (None, Some((_, slot))) => *slot = at,
            (None, None) => self.selected = at,
        }
    }

    /// Focus a row by index (clicking it).
    pub fn select(&mut self, index: usize) {
        if index < self.rows() {
            self.move_sel(index as i32 - self.selected() as i32);
        }
    }

    /// How many rows whichever list is up has.
    fn rows(&self) -> usize {
        match (self.kind, self.stick) {
            (Some(_), _) => self.slot().map_or(0, |slot| Kind::all(slot).len()),
            (None, Some(_)) => self.stick_rows().len(),
            (None, None) => sources().len(),
        }
    }

    /// The row the kind list writes to, which is the focused one unless a stick
    /// is open, when it is that stick's.
    pub fn slot(&self) -> Option<Slot> {
        let Some((side, at)) = self.stick else {
            return match sources().get(self.selected)? {
                Source::Button(pad) => Some(Slot::Button(*pad)),
                Source::Stick(side) => Some(Slot::Stick(*side)),
            };
        };
        match self.stick_rows().get(at)? {
            StickRow::Sends => Some(Slot::Stick(side)),
            StickRow::Direction(dir) => Some(Slot::Direction(side, *dir)),
        }
    }

    /// Whether the kind list is up, for the screen that draws it.
    pub fn kind_open(&self) -> bool {
        self.kind.is_some()
    }

    /// What **A** takes, or `None` on a list with nothing in it.
    pub fn press(&self) -> Option<EditPress> {
        let slot = self.slot()?;
        if let Some(at) = self.kind {
            let kinds = Kind::all(slot);
            return kinds.get(at).map(|kind| EditPress::Take(*kind, slot));
        }
        // A stick at the top level opens its own rows; from inside them the
        // same row opens what the whole stick can send.
        match (slot, self.stick.is_none()) {
            (Slot::Stick(side), true) => Some(EditPress::OpenStick(side)),
            _ => Some(EditPress::OpenKinds),
        }
    }

    pub fn open_stick(&mut self, side: Side) {
        self.stick = Some((side, 0));
    }

    pub fn open_kinds(&mut self) {
        self.kind = Some(0);
    }

    pub fn close_kinds(&mut self) {
        self.kind = None;
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    /// The row the keyboard is picking a key for, if it is up.
    pub fn picking(&self) -> Option<Slot> {
        self.picking
    }

    pub fn set_picking(&mut self, slot: Option<Slot>) {
        self.picking = slot;
    }

    /// The slot the on-screen keyboard writes its pick into.
    pub fn picked_mut(&mut self) -> &mut Option<String> {
        &mut self.picked
    }

    /// Take a pick the keyboard left, if any.
    pub fn take_picked(&mut self) -> Option<String> {
        self.picked.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opened() -> MapEdit {
        let mut edit = MapEdit::new();
        edit.open("keys".to_string(), "Keyboard keys".to_string());
        let mut targets = Targets {
            pads: vec!["-".to_string(); Pad::COUNT],
            ..Targets::default()
        };
        targets.sticks[Side::Left as usize].digital = true;
        edit.set_targets(targets);
        edit
    }

    /// Select is the one pad no map may name, so the editor must not offer
    /// a row that would be refused on save. The sticks follow the buttons.
    #[test]
    fn the_reserved_pad_is_not_a_row_and_the_sticks_are() {
        let sources = sources();
        assert!(!sources.contains(&Source::Button(Pad::Select)));
        assert_eq!(sources.len(), Pad::COUNT - 1 + Side::ALL.len());
        assert_eq!(sources.last(), Some(&Source::Stick(Side::Right)));
    }

    /// Every kind is on screen, and only the key one defers to the keyboard.
    /// What a slot is offered follows what the engine can actually do with it.
    #[test]
    fn a_row_can_be_set_to_anything_its_list_shows() {
        let button = Kind::all(Slot::Button(Pad::A));
        let dir = Kind::all(Slot::Direction(Side::Left, Dir::Up));
        let stick = Kind::all(Slot::Stick(Side::Left));
        for kind in [button, dir, stick].concat() {
            assert!(!kind.label().is_empty());
            assert_eq!(kind.take() == Take::Key, kind == Kind::Key);
        }
        // A direction's axis is withheld wholesale, so passthrough there would
        // send the page nothing while claiming otherwise.
        assert!(!dir.contains(&Kind::Passthrough));
        assert!(button.contains(&Kind::Passthrough));
        // Only a whole stick can be a vector, and only it can be split up.
        for kind in [Kind::Cursor, Kind::Scroll, Kind::Directions] {
            assert!(stick.contains(&kind) && !button.contains(&kind));
        }
    }

    /// A stick is a row until it is opened, and then it is its own list.
    #[test]
    fn a_stick_opens_its_own_rows_and_a_button_opens_its_kinds() {
        let mut edit = opened();
        assert_eq!(edit.press(), Some(EditPress::OpenKinds));
        edit.select(sources().len() - 2);
        assert_eq!(edit.press(), Some(EditPress::OpenStick(Side::Left)));
        edit.open_stick(Side::Left);
        // Its first row is the whole stick; the four directions follow because
        // this one is digital.
        assert_eq!(edit.slot(), Some(Slot::Stick(Side::Left)));
        assert_eq!(edit.stick_rows().len(), 1 + Dir::ALL.len());
        edit.move_sel(1);
        assert_eq!(edit.slot(), Some(Slot::Direction(Side::Left, Dir::Up)));
        assert_eq!(edit.press(), Some(EditPress::OpenKinds));
    }

    /// A stick read as one vector has nothing to show per direction.
    #[test]
    fn an_analog_stick_has_no_direction_rows() {
        let mut edit = opened();
        edit.open_stick(Side::Right);
        assert_eq!(edit.stick_rows(), [StickRow::Sends]);
        edit.move_sel(99);
        assert_eq!(edit.slot(), Some(Slot::Stick(Side::Right)));
    }

    /// B walks back out one list at a time, and says when there is none left.
    #[test]
    fn back_pops_one_list_at_a_time() {
        let mut edit = opened();
        edit.open_stick(Side::Left);
        edit.open_kinds();
        assert!(edit.back() && !edit.kind_open());
        assert!(edit.back() && edit.stick_open().is_none());
        assert!(!edit.back());
    }

    #[test]
    fn the_highlight_stops_at_the_ends_of_whichever_list_is_up() {
        let mut edit = opened();
        edit.move_sel(-1);
        assert_eq!(edit.selected(), 0);
        edit.move_sel(99);
        assert_eq!(edit.selected(), sources().len() - 1);
        // The kind list moves instead while it is open, and the row stays put.
        edit.select(0);
        let row = edit.selected();
        edit.open_kinds();
        edit.move_sel(99);
        assert_eq!(
            edit.press(),
            Some(EditPress::Take(Kind::Ignore, Slot::Button(Pad::A)))
        );
        edit.close_kinds();
        assert_eq!(edit.selected(), row);
    }
}
