//! The Game Mode map editor: a row per source the map binds, showing what it
//! sends and letting the pad itself change it. The device this is for has no
//! keyboard and no file manager, so a map it cannot edit here is a map it cannot
//! edit at all; the file stays the fuller interface
//! ([`crate::event::game::input_map`]).
//!
//! Every row answers A the same way, with the list of what it can send. A
//! source is a row only once the map binds it, and a stick read as four
//! directions grows its four rows under itself. The trailing row listens for
//! the source to add — a button, a key, or a stick pushed over — which covers a
//! pad this build has never heard of and a keyboard whose keys no screen could
//! list.
//!
//! Layers are still the file's — they need names this screen has no room to pick.

use crate::event::game::input_map::{Dir, Side, STICK_PREFIX};
use inputbind::{KeyGesture, Mods, Pad, PadGesture};

/// What one row writes to.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Slot {
    Button(Pad),
    /// A physical key, by the name the file spells it with.
    Key(String),
    /// The stick as one vector.
    Stick(Side),
    /// One of its four directions.
    Direction(Side, Dir),
}

impl Slot {
    /// The path the file holds it under, table included — what the list opened
    /// over a row is titled, since nothing else there says which source it is
    /// about. TOML reads `pad.a = "key.Space"` as the `[pad]` table's `a`, so
    /// this is a line of the file as much as a name for one.
    pub fn name(&self) -> String {
        match self {
            Slot::Button(pad) => format!("pad.{}", pad.name()),
            Slot::Key(key) => format!("key.{key}"),
            Slot::Stick(side) => format!("{STICK_PREFIX}{}", side.name()),
            Slot::Direction(side, dir) => {
                format!("{STICK_PREFIX}{}.{}", side.name(), dir.name())
            }
        }
    }

    /// The row's own label: the name inside its table, since the heading over
    /// the row already says which table that is.
    pub fn label(&self) -> String {
        match self {
            Slot::Button(pad) => pad.name().to_string(),
            Slot::Key(key) => key.clone(),
            Slot::Stick(side) => side.name().to_string(),
            Slot::Direction(side, dir) => format!("{}.{}", side.name(), dir.name()),
        }
    }
}

/// What a row can be set to. One list per slot, so nothing about a row is
/// hidden behind a verb the screen cannot show.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// Hands over to the on-screen keyboard, which picks the key itself.
    Key,
    /// Opens the mouse list — the buttons, and the cursor and scroll steps.
    Mouse,
    /// The page reads the source through the Gamepad API. An unbound one does
    /// the same thing; this is the file saying so out loud, and the two are one
    /// entry here because they are one behaviour.
    Passthrough,
    /// Consumed and dropped: the source does nothing at all.
    Ignore,
    /// Read the whole stick as four directions instead of one vector.
    Directions,
}

impl Kind {
    /// What this slot can be told to send. A direction is offered no
    /// Passthrough: a stick read as directions withholds the whole axis, so
    /// the page would see nothing either way.
    pub fn all(slot: &Slot) -> &'static [Kind] {
        match slot {
            Slot::Button(_) | Slot::Key(_) => {
                &[Kind::Key, Kind::Mouse, Kind::Passthrough, Kind::Ignore]
            }
            Slot::Direction(..) => &[Kind::Key, Kind::Mouse, Kind::Ignore],
            Slot::Stick(_) => &[
                Kind::Directions,
                Kind::Mouse,
                Kind::Passthrough,
                Kind::Ignore,
            ],
        }
    }

    /// What the row says the source will send. The two that open another list
    /// name the device it is about, since that is the next thing seen.
    pub fn label(self) -> &'static str {
        match self {
            Kind::Key => "Keyboard",
            Kind::Mouse => "Mouse",
            Kind::Passthrough => "Passthrough",
            Kind::Ignore => "Ignore",
            Kind::Directions => "Four directions",
        }
    }

    /// What taking it does to the map.
    pub fn take(self) -> Take {
        match self {
            Kind::Key => Take::Key,
            Kind::Mouse => Take::Mouse,
            Kind::Directions => Take::Arrows,
            Kind::Passthrough => Take::Text("passthrough"),
            Kind::Ignore => Take::Text("none"),
        }
    }
}

/// What the mouse list offers. A stick points or scrolls as a whole; a source
/// with an edge presses a button, or steps the cursor or the page for as long
/// as it is held.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum MouseKind {
    ButtonLeft,
    ButtonRight,
    ButtonMiddle,
    CursorUp,
    CursorDown,
    CursorLeft,
    CursorRight,
    ScrollUp,
    ScrollDown,
    ScrollLeft,
    ScrollRight,
    Cursor,
    Scroll,
}

impl MouseKind {
    pub fn all(slot: &Slot) -> &'static [MouseKind] {
        match slot {
            Slot::Stick(_) => &[MouseKind::Cursor, MouseKind::Scroll],
            _ => &[
                MouseKind::ButtonLeft,
                MouseKind::ButtonRight,
                MouseKind::ButtonMiddle,
                MouseKind::CursorUp,
                MouseKind::CursorDown,
                MouseKind::CursorLeft,
                MouseKind::CursorRight,
                MouseKind::ScrollUp,
                MouseKind::ScrollDown,
                MouseKind::ScrollLeft,
                MouseKind::ScrollRight,
            ],
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            MouseKind::ButtonLeft => "Button left",
            MouseKind::ButtonRight => "Button right",
            MouseKind::ButtonMiddle => "Button middle",
            MouseKind::CursorUp => "Cursor up",
            MouseKind::CursorDown => "Cursor down",
            MouseKind::CursorLeft => "Cursor left",
            MouseKind::CursorRight => "Cursor right",
            MouseKind::ScrollUp => "Scroll up",
            MouseKind::ScrollDown => "Scroll down",
            MouseKind::ScrollLeft => "Scroll left",
            MouseKind::ScrollRight => "Scroll right",
            MouseKind::Cursor => "Cursor",
            MouseKind::Scroll => "Scroll",
        }
    }

    /// The target it writes, as the file spells it.
    pub fn target(self) -> &'static str {
        match self {
            MouseKind::ButtonLeft => "mouse.left",
            MouseKind::ButtonRight => "mouse.right",
            MouseKind::ButtonMiddle => "mouse.middle",
            MouseKind::CursorUp => "mouse.cursor.up",
            MouseKind::CursorDown => "mouse.cursor.down",
            MouseKind::CursorLeft => "mouse.cursor.left",
            MouseKind::CursorRight => "mouse.cursor.right",
            MouseKind::ScrollUp => "mouse.scroll.up",
            MouseKind::ScrollDown => "mouse.scroll.down",
            MouseKind::ScrollLeft => "mouse.scroll.left",
            MouseKind::ScrollRight => "mouse.scroll.right",
            MouseKind::Cursor => "mouse.cursor",
            MouseKind::Scroll => "mouse.scroll",
        }
    }
}

/// What taking a kind does — most write a target, the rest ask another question.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Take {
    /// Write this as the target, the way the file spells it.
    Text(&'static str),
    /// Summon the on-screen keyboard to pick the key.
    Key,
    /// Open the mouse list over this row.
    Mouse,
    /// Turn the stick into four directions, seeded with the arrows — a stick
    /// with none set reaches the page instead, which the row would not say.
    Arrows,
}

/// What **A** does, given which of the two lists is up and which row it is on.
/// Qualified because the map screens have one of these too.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum EditPress {
    /// Open the list of what the focused row can send.
    OpenKinds,
    /// Listen for the source to give a row (the trailing row).
    StartCapture,
    /// Take the kind the list is on, for the row it was opened over.
    Take(Kind, Slot),
    /// Take the mouse kind its list is on, for the same row.
    TakeMouse(MouseKind, Slot),
}

/// The source a captured gesture names, or why the map cannot hold it. The map
/// gives one target per source, so it has no room for what a bindings file can
/// still say: a hold, a chord, a modified key.
pub fn source_of(gesture: &str, keyboard: bool) -> Result<Slot, &'static str> {
    if keyboard {
        return match KeyGesture::parse(gesture) {
            Some(key) if key.mods == Mods::NONE => Ok(Slot::Key(key.name)),
            _ => Err("one key at a time, without modifiers"),
        };
    }
    // A stick arrives named: a deflection is neither a tap nor a hold.
    if let Some(name) = gesture.strip_prefix(STICK_PREFIX) {
        return match Side::parse(name) {
            Some(side) => Ok(Slot::Stick(side)),
            None => Err("that stick is not one this map knows"),
        };
    }
    match PadGesture::parse(gesture) {
        // Select opens the Game Mode menu in every map, so the game never gets
        // it — the file's own refusal, said here in time to be read.
        Some(PadGesture::Tap(Pad::Select)) => Err("select opens the Game Mode menu"),
        Some(PadGesture::Tap(pad)) => Ok(Slot::Button(pad)),
        _ => Err("one button at a time, tapped"),
    }
}

/// What an unbound source reads as in the editor's rows — the one spelling,
/// shared by the snapshot builder and the renderer's fallback.
pub const UNBOUND: &str = "-";

/// The trailing row, which listens for a source rather than editing one. It
/// names no device: the headings do that, and the screen says what to press
/// once it is listening.
pub const ADD_ROW: &str = "+ Add";

/// One source the map binds, as its row shows it — a snapshot, since the map
/// lives in the event handler that resolved it.
pub struct Row {
    pub slot: Slot,
    /// What it sends, as the file spells it.
    pub target: String,
}

pub struct MapEdit {
    visible: bool,
    /// The map being edited — not necessarily the one Game Mode runs, so a
    /// mapping can be set up without disturbing a game already under way.
    map: String,
    /// Its name, for the panel's title.
    name: String,
    /// The source list's highlight, which runs one past the rows: the last row
    /// is the one that adds.
    selected: usize,
    /// The kind list over the focused row; `None` while the rows are.
    kind: Option<usize>,
    /// The mouse list over the kind list, opened by its Mouse row.
    mouse: Option<usize>,
    /// Whether the screen is listening for a source to add.
    capturing: bool,
    /// The captured source, while its kind list is up. It has no row until a
    /// kind is taken, so backing out of that list writes nothing.
    fresh: Option<Slot>,
    /// Why the last capture gave no row, shown on the row that listened.
    note: Option<String>,
    /// The row the on-screen keyboard is picking a key for.
    picking: Option<Slot>,
    /// What it picked, as the file spells it — read and cleared by the app.
    picked: Option<String>,
    /// Whether anything changed, so an untouched visit writes no file.
    dirty: bool,
    rows: Vec<Row>,
}

impl MapEdit {
    pub fn new() -> Self {
        Self {
            visible: false,
            map: String::new(),
            name: String::new(),
            selected: 0,
            kind: None,
            mouse: None,
            capturing: false,
            fresh: None,
            note: None,
            picking: None,
            picked: None,
            dirty: false,
            rows: vec![],
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
        self.kind = None;
        self.mouse = None;
        self.capturing = false;
        self.fresh = None;
        self.note = None;
        self.picking = None;
        self.picked = None;
        self.dirty = false;
    }

    /// Close it, reporting whether the map needs writing.
    pub fn close(&mut self) -> bool {
        self.visible = false;
        self.kind = None;
        self.mouse = None;
        self.capturing = false;
        self.fresh = None;
        self.note = None;
        self.picking = None;
        std::mem::take(&mut self.dirty)
    }

    /// Back out one list — the mouse list, then the kinds. `false` leaves the
    /// rows, which is when the editor closes.
    pub fn back(&mut self) -> bool {
        if self.mouse.take().is_some() {
            return true;
        }
        self.fresh = None;
        self.kind.take().is_some()
    }

    /// Adopt a fresh snapshot of the map. Rows come and go as bindings are made
    /// and dropped, so the highlight can be left past the end.
    pub fn set_rows(&mut self, rows: Vec<Row>) {
        self.rows = rows;
        self.selected = self.selected.min(self.rows.len());
    }

    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    /// Where the highlight is on whichever list is up.
    pub fn selected(&self) -> usize {
        self.mouse.or(self.kind).unwrap_or(self.selected)
    }

    /// Move it, clamped to the ends of that list.
    pub fn move_sel(&mut self, dy: i32) {
        let at = crate::list::step(self.selected(), dy, self.row_count());
        match (&mut self.mouse, &mut self.kind) {
            (Some(row), _) => *row = at,
            (None, Some(row)) => *row = at,
            (None, None) => self.selected = at,
        }
    }

    /// Focus a row by index.
    pub fn select(&mut self, index: usize) {
        if index < self.row_count() {
            self.move_sel(index as i32 - self.selected() as i32);
        }
    }

    /// Put the highlight on `slot`'s row, where the map has one — how a source
    /// just bound is shown to have landed.
    pub fn select_slot(&mut self, slot: &Slot) {
        if let Some(at) = self.rows.iter().position(|row| row.slot == *slot) {
            self.selected = at;
        }
    }

    /// How many rows whichever list is up has. The source list carries the row
    /// that adds past the ones the map binds.
    fn row_count(&self) -> usize {
        let Some(slot) = self.slot() else {
            return self.rows.len() + 1;
        };
        match (self.mouse.is_some(), self.kind.is_some()) {
            (true, _) => MouseKind::all(&slot).len(),
            (false, true) => Kind::all(&slot).len(),
            (false, false) => self.rows.len() + 1,
        }
    }

    /// The row the kind list writes to — a captured source on its way in, or
    /// the focused row, which the kind list leaves where it was.
    pub fn slot(&self) -> Option<Slot> {
        if self.fresh.is_some() {
            return self.fresh.clone();
        }
        self.rows.get(self.selected).map(|row| row.slot.clone())
    }

    /// Whether the kind list is up, for the screen that draws it.
    pub fn kind_open(&self) -> bool {
        self.kind.is_some()
    }

    /// Whether the mouse list is up, which is drawn over the kinds.
    pub fn mouse_open(&self) -> bool {
        self.mouse.is_some()
    }

    /// What **A** takes, or `None` on a list with nothing in it.
    pub fn press(&self) -> Option<EditPress> {
        let Some(at) = self.kind else {
            // The row past the bound ones has no slot: it is the one listening.
            return match self.slot() {
                Some(_) => Some(EditPress::OpenKinds),
                None => Some(EditPress::StartCapture),
            };
        };
        let slot = self.slot()?;
        if let Some(at) = self.mouse {
            return MouseKind::all(&slot)
                .get(at)
                .map(|kind| EditPress::TakeMouse(*kind, slot));
        }
        Kind::all(&slot)
            .get(at)
            .map(|kind| EditPress::Take(*kind, slot))
    }

    pub fn open_kinds(&mut self) {
        self.kind = Some(0);
    }

    pub fn open_mouse(&mut self) {
        self.mouse = Some(0);
    }

    /// Close whatever is over the rows — both lists, since taking a target ends
    /// the whole question.
    pub fn close_kinds(&mut self) {
        self.kind = None;
        self.mouse = None;
        self.fresh = None;
    }

    /// Listen for the source a new row will be for. Whatever the last attempt
    /// had to say is cleared: the row is being asked again.
    pub fn start_capture(&mut self) {
        self.capturing = true;
        self.note = None;
    }

    /// Whether the screen is listening, which is what routes raw input here.
    pub fn capturing(&self) -> bool {
        self.capturing
    }

    /// Stop listening, saying why where the caller has something to say (a
    /// gesture the map cannot hold); a timeout passes `None` and shows nothing.
    pub fn stop_capture(&mut self, note: Option<String>) {
        self.capturing = false;
        self.note = note;
    }

    /// What the row that listens has to say, if the last attempt gave no row.
    pub fn note(&self) -> Option<&str> {
        self.note.as_deref()
    }

    /// A captured source: its kind list opens over the rows, and only taking a
    /// kind gives it one of its own.
    pub fn open_kinds_for(&mut self, slot: Slot) {
        self.fresh = Some(slot);
        self.open_kinds();
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    /// The row the keyboard is picking a key for, if it is up.
    pub fn picking(&self) -> Option<Slot> {
        self.picking.clone()
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

    /// An editor on a map that binds A and one key, with a digital left stick
    /// and an analog right one — every shape a row has.
    fn opened() -> MapEdit {
        let mut edit = MapEdit::new();
        edit.open("keys".to_string(), "Keyboard keys".to_string());
        edit.set_rows(rows());
        edit
    }

    fn rows() -> Vec<Row> {
        let mut rows = vec![
            row(Slot::Stick(Side::Left), "directions"),
            row(Slot::Direction(Side::Left, Dir::Up), "ArrowUp"),
            row(Slot::Stick(Side::Right), "mouse.cursor"),
            row(Slot::Button(Pad::A), "Space"),
        ];
        rows.push(row(Slot::Key("w".to_string()), "ArrowUp"));
        rows
    }

    fn row(slot: Slot, target: &str) -> Row {
        Row {
            slot,
            target: target.to_string(),
        }
    }

    /// Every kind is on screen, and only the two that ask another question
    /// defer instead of writing. What a slot is offered follows what the engine
    /// can do with it.
    #[test]
    fn a_row_can_be_set_to_anything_its_list_shows() {
        let button = Kind::all(&Slot::Button(Pad::A));
        let key = Kind::all(&Slot::Key("w".to_string()));
        let dir = Kind::all(&Slot::Direction(Side::Left, Dir::Up));
        let stick = Kind::all(&Slot::Stick(Side::Left));
        for kind in [button, dir, stick].concat() {
            assert!(!kind.label().is_empty());
            let asks = matches!(kind.take(), Take::Key | Take::Mouse);
            assert_eq!(asks, matches!(kind, Kind::Key | Kind::Mouse), "{kind:?}");
        }
        // A key is a source like a button, and answers with the same list.
        assert_eq!(button, key);
        // A direction's axis is withheld wholesale, so passthrough there would
        // send the page nothing while claiming otherwise.
        assert!(!dir.contains(&Kind::Passthrough));
        assert!(button.contains(&Kind::Passthrough));
        // Only a whole stick can be split into directions, and only a source
        // with an edge can press a button or scroll a step.
        assert!(stick.contains(&Kind::Directions) && !button.contains(&Kind::Directions));
        let stick_mouse = MouseKind::all(&Slot::Stick(Side::Left));
        let button_mouse = MouseKind::all(&Slot::Button(Pad::A));
        assert_eq!(stick_mouse, [MouseKind::Cursor, MouseKind::Scroll]);
        assert!(button_mouse.contains(&MouseKind::ButtonRight));
        // An edge steps the cursor where a stick aims it, and cannot do the
        // whole-vector job the other way round.
        assert!(button_mouse.contains(&MouseKind::CursorUp));
        assert!(!button_mouse.contains(&MouseKind::Cursor));
        assert!(!stick_mouse.contains(&MouseKind::CursorUp));
        // Every mouse row writes a target under the one prefix.
        for kind in [stick_mouse, button_mouse].concat() {
            assert!(!kind.label().is_empty());
            assert!(kind.target().starts_with("mouse."), "{kind:?}");
        }
    }

    /// What a capture may name: one source, which is all a map row holds.
    #[test]
    fn a_capture_names_one_source_or_says_why_not() {
        assert_eq!(source_of("a", false), Ok(Slot::Button(Pad::A)));
        assert_eq!(source_of("w", true), Ok(Slot::Key("w".to_string())));
        assert_eq!(
            source_of("stick.right", false),
            Ok(Slot::Stick(Side::Right))
        );
        assert!(source_of("stick.middle", false).is_err());
        // A modifier on its own is a key like any other.
        assert_eq!(
            source_of("leftshift", true),
            Ok(Slot::Key("leftshift".to_string()))
        );
        // The file can spell these; a map row cannot hold them.
        for (gesture, keyboard) in [("hold:b", false), ("l1+r1", false), ("ctrl+w", true)] {
            assert!(source_of(gesture, keyboard).is_err());
        }
        // Select carries the Game Mode menu in every map.
        assert!(source_of("select", false).is_err());
    }

    /// A row is named inside its table and spelled out in full wherever no
    /// heading says which table that is — the three `up`s a map can hold are
    /// one name on screen and three paths in the file.
    #[test]
    fn a_row_is_named_inside_its_table_and_its_path_outside() {
        let slots = [
            Slot::Button(Pad::Up),
            Slot::Key("up".to_string()),
            Slot::Direction(Side::Left, Dir::Up),
        ];
        let labels: Vec<String> = slots.iter().map(Slot::label).collect();
        let names: Vec<String> = slots.iter().map(Slot::name).collect();
        assert_eq!(labels, ["up", "up", "left.up"]);
        assert_eq!(names, ["pad.up", "key.up", "stick.left.up"]);
    }

    /// The row past the ones the map binds is what listens for a new source.
    #[test]
    fn the_trailing_row_captures_and_the_rest_open_their_kinds() {
        let mut edit = opened();
        assert_eq!(edit.press(), Some(EditPress::OpenKinds));
        edit.move_sel(99);
        assert_eq!(edit.selected(), rows().len());
        assert_eq!(edit.slot(), None);
        assert_eq!(edit.press(), Some(EditPress::StartCapture));
    }

    /// A captured source has no row until its kind is taken, so backing out of
    /// the list it opens leaves the map as it was.
    #[test]
    fn a_captured_source_writes_nothing_until_its_kind_is_taken() {
        let mut edit = opened();
        edit.start_capture();
        assert!(edit.capturing());
        edit.stop_capture(None);
        let slot = Slot::Key("q".to_string());
        edit.open_kinds_for(slot.clone());
        assert!(!edit.capturing());
        assert_eq!(edit.slot(), Some(slot.clone()));
        assert_eq!(edit.press(), Some(EditPress::Take(Kind::Key, slot.clone())));
        // B: the list goes, and with it the source that had no row.
        assert!(edit.back());
        assert_eq!(edit.slot(), edit.rows().first().map(|row| row.slot.clone()));
    }

    /// A gesture the map cannot hold leaves its reason on the row that asked.
    #[test]
    fn a_refused_capture_says_so_where_it_was_asked() {
        let mut edit = opened();
        edit.start_capture();
        edit.stop_capture(Some("one source at a time".to_string()));
        assert!(!edit.capturing());
        assert_eq!(edit.note(), Some("one source at a time"));
        // Asking again drops what the last answer had to say.
        edit.start_capture();
        assert_eq!(edit.note(), None);
    }

    /// A source just bound is shown to have landed, wherever its row sorted.
    #[test]
    fn the_highlight_follows_a_source_to_its_row() {
        let mut edit = opened();
        let slot = Slot::Key("w".to_string());
        edit.select_slot(&slot);
        assert_eq!(edit.slot(), Some(slot));
        // A slot with no row leaves the highlight where it was.
        let at = edit.selected();
        edit.select_slot(&Slot::Button(Pad::B));
        assert_eq!(edit.selected(), at);
    }

    /// The rows a map loses cannot be left highlighted: the highlight would
    /// point past the end of the list.
    #[test]
    fn the_highlight_survives_the_rows_going_away() {
        let mut edit = opened();
        edit.move_sel(99);
        edit.set_rows(vec![]);
        assert_eq!(edit.selected(), 0);
        assert_eq!(edit.press(), Some(EditPress::StartCapture));
    }

    /// B leaves the kind list, and then has nothing left to pop — which is when
    /// the editor closes.
    #[test]
    fn back_pops_the_kind_list_and_then_nothing() {
        let mut edit = opened();
        edit.open_kinds();
        assert!(edit.back() && !edit.kind_open());
        assert!(!edit.back());
    }

    #[test]
    fn the_highlight_stops_at_the_ends_of_whichever_list_is_up() {
        let mut edit = opened();
        edit.move_sel(-1);
        assert_eq!(edit.selected(), 0);
        edit.move_sel(99);
        assert_eq!(edit.selected(), rows().len());
        // The kind list moves instead while it is open, and the row stays put.
        edit.select(3);
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
