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
//! Layers follow the base rows: a row per layer opens the same screen scoped
//! to it, and a source holds one open by sending `Layer`. A new layer is
//! named on the on-screen keyboard.

use crate::event::game::input_map::{
    Dir, KeyMods, Side, KEY_PREFIX, LAYER_PREFIX, PAD_PREFIX, STICK_PREFIX,
};
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
    /// The source as the screen says it: `L3`, `D-pad up`, `Left stick up`.
    pub fn label(&self) -> String {
        match self {
            Slot::Button(pad) => pad_label(*pad).to_string(),
            Slot::Key(key) => key_label(key),
            Slot::Stick(side) => stick_label(*side).to_string(),
            Slot::Direction(side, dir) => format!("{} {}", stick_label(*side), dir.name()),
        }
    }

    /// The source a path in the file names: `pad.l3`, `key.q`, `stick.left.up`.
    pub fn from_path(path: &str) -> Option<Slot> {
        if let Some(pad) = path.strip_prefix(PAD_PREFIX) {
            return Pad::parse(pad).map(Slot::Button);
        }
        if let Some(key) = path.strip_prefix(KEY_PREFIX) {
            return Some(Slot::Key(key.to_string()));
        }
        let stick = path.strip_prefix(STICK_PREFIX)?;
        match stick.split_once('.') {
            Some((side, dir)) => Some(Slot::Direction(Side::parse(side)?, Dir::parse(dir)?)),
            None => Side::parse(stick).map(Slot::Stick),
        }
    }
}

/// A pad button as the screen says it.
pub fn pad_label(pad: Pad) -> &'static str {
    match pad {
        Pad::A => "A",
        Pad::B => "B",
        Pad::X => "X",
        Pad::Y => "Y",
        Pad::L1 => "L1",
        Pad::R1 => "R1",
        Pad::L2 => "L2",
        Pad::R2 => "R2",
        Pad::L3 => "L3",
        Pad::R3 => "R3",
        Pad::Start => "Start",
        Pad::Select => "Select",
        Pad::Up => "D-pad up",
        Pad::Down => "D-pad down",
        Pad::Left => "D-pad left",
        Pad::Right => "D-pad right",
    }
}

fn stick_label(side: Side) -> &'static str {
    match side {
        Side::Left => "Left stick",
        Side::Right => "Right stick",
    }
}

/// A key as the screen says it: the first letter upper-cased, a standard name
/// split into words (`ArrowUp` reads `Arrow up`).
fn key_label(name: &str) -> String {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return String::new();
    };
    let mut out: String = first.to_uppercase().collect();
    for c in chars {
        if c.is_uppercase() {
            out.push(' ');
            out.extend(c.to_lowercase());
        } else {
            out.push(c);
        }
    }
    out
}

/// What a stick's row holds once it is four directions; not a target the file
/// spells, but the row's value.
pub const DIRECTIONS: &str = "directions";

/// How the file spells the two targets that name no device.
const PASSTHROUGH: &str = "passthrough";
const IGNORE: &str = "none";

/// The prefix every mouse target shares.
const MOUSE_PREFIX: &str = "mouse.";

/// A target as the screen says it: `Space`, `Left click`, `Layer mouse`.
/// Takes the file's spelling; one it does not know is shown as written.
pub fn target_label(text: &str) -> String {
    match text {
        PASSTHROUGH => return Kind::Passthrough.label().to_string(),
        IGNORE => return Kind::Ignore.label().to_string(),
        DIRECTIONS => return Kind::Directions.label().to_string(),
        _ => {}
    }
    if let Some(kind) = MouseKind::ALL.iter().find(|kind| kind.target() == text) {
        return kind.label().to_string();
    }
    if let Some(key) = text.strip_prefix(KEY_PREFIX) {
        return key_label(key);
    }
    if let Some(pad) = text.strip_prefix(PAD_PREFIX).and_then(Pad::parse) {
        return format!("Gamepad {}", pad_label(pad));
    }
    if let Some(layer) = text.strip_prefix(LAYER_PREFIX) {
        return format!("Layer {layer}");
    }
    text.to_string()
}

/// The kind a target is, for the list to open on.
fn kind_of(text: &str) -> Option<Kind> {
    Some(match text {
        PASSTHROUGH => Kind::Passthrough,
        IGNORE => Kind::Ignore,
        DIRECTIONS => Kind::Directions,
        _ if text.starts_with(KEY_PREFIX) => Kind::Key,
        _ if text.starts_with(MOUSE_PREFIX) => Kind::Mouse,
        _ if text.starts_with(PAD_PREFIX) => Kind::Gamepad,
        _ if text.starts_with(LAYER_PREFIX) => Kind::Layer,
        _ => return None,
    })
}

/// What a row can be set to. One list per slot, so nothing about a row is
/// hidden behind a verb the screen cannot show.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind {
    /// Hands over to the on-screen keyboard, which picks the key itself.
    Key,
    /// Opens the mouse list — the buttons, and the cursor and scroll steps.
    Mouse,
    /// Opens the gamepad list — the buttons the page reads through the
    /// Gamepad API, which is how a keyboard drives a pad-only game.
    Gamepad,
    /// The page reads the source through the Gamepad API. An unbound one does
    /// the same thing; this is the file saying so out loud, and the two are one
    /// entry here because they are one behaviour.
    Passthrough,
    /// Consumed and dropped: the source does nothing at all.
    Ignore,
    /// Read the whole stick as four directions instead of one vector.
    Directions,
    /// Opens the layer list: the source holds the picked layer open.
    Layer,
    /// Opens the modifier list: the modifiers sent with the row's key.
    Modifiers,
    /// Unbind the row.
    Remove,
}

/// Where a kind sits in the kinds list, in screen order.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum KindGroup {
    /// What the source sends, and how.
    Send,
    /// The source reaching the page as is, or doing nothing.
    Other,
    Remove,
}

/// A modifier a key row can send with its key.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Modifier {
    Shift,
    Ctrl,
    Alt,
}

impl Modifier {
    pub const ALL: [Modifier; 3] = [Modifier::Ctrl, Modifier::Alt, Modifier::Shift];

    pub fn label(self) -> &'static str {
        match self {
            Modifier::Shift => "Shift",
            Modifier::Ctrl => "Ctrl",
            Modifier::Alt => "Alt",
        }
    }

    pub fn is_in(self, mods: KeyMods) -> bool {
        match self {
            Modifier::Shift => mods.shift,
            Modifier::Ctrl => mods.ctrl,
            Modifier::Alt => mods.alt,
        }
    }

    pub fn toggled(self, mut mods: KeyMods) -> KeyMods {
        let flag = match self {
            Modifier::Shift => &mut mods.shift,
            Modifier::Ctrl => &mut mods.ctrl,
            Modifier::Alt => &mut mods.alt,
        };
        *flag = !*flag;
        mods
    }
}

/// The modifiers alone, as the Modifiers row says them: `Ctrl+Shift`, or `None`.
pub fn mods_label(mods: KeyMods) -> String {
    let names: Vec<&str> = Modifier::ALL
        .iter()
        .filter(|modifier| modifier.is_in(mods))
        .map(|modifier| modifier.label())
        .collect();
    match names.is_empty() {
        true => "None".to_string(),
        false => names.join("+"),
    }
}

/// A target's label with the modifiers it is sent with: `Ctrl+Shift+1`.
pub fn held_label(label: String, mods: KeyMods) -> String {
    Modifier::ALL
        .iter()
        .rev()
        .filter(|modifier| modifier.is_in(mods))
        .fold(label, |label, modifier| {
            format!("{}+{label}", modifier.label())
        })
}

impl Kind {
    /// What this slot can be told to send. A direction is offered no
    /// Passthrough: a stick read as directions withholds the whole axis, so
    /// the page would see nothing either way.
    pub fn all(slot: &Slot) -> &'static [Kind] {
        match slot {
            Slot::Button(_) | Slot::Key(_) => &[
                Kind::Key,
                Kind::Mouse,
                Kind::Gamepad,
                Kind::Passthrough,
                Kind::Ignore,
            ],
            Slot::Direction(..) => &[Kind::Key, Kind::Mouse, Kind::Gamepad, Kind::Ignore],
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
            Kind::Gamepad => "Gamepad",
            Kind::Passthrough => "Passthrough",
            Kind::Ignore => "Ignore",
            Kind::Directions => "Four directions",
            Kind::Layer => "Layer",
            Kind::Modifiers => "Modifiers",
            Kind::Remove => "Remove",
        }
    }

    /// The group the kinds list shows it in.
    pub fn group(self) -> KindGroup {
        match self {
            Kind::Passthrough | Kind::Ignore => KindGroup::Other,
            Kind::Remove => KindGroup::Remove,
            _ => KindGroup::Send,
        }
    }

    /// What taking it does to the map.
    pub fn take(self) -> Take {
        match self {
            Kind::Key => Take::Key,
            Kind::Mouse => Take::Mouse,
            Kind::Gamepad => Take::Gamepad,
            Kind::Directions => Take::Arrows,
            Kind::Layer => Take::Layer,
            Kind::Modifiers => Take::Modifiers,
            Kind::Remove => Take::Remove,
            Kind::Passthrough => Take::Text(PASSTHROUGH),
            Kind::Ignore => Take::Text(IGNORE),
        }
    }
}

/// Which device's own list is open over the kinds.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Device {
    Mouse,
    Gamepad,
    Layer,
    Modifiers,
}

impl Device {
    /// What the title says after the row's name.
    pub fn title(self) -> &'static str {
        match self {
            Device::Mouse => "MOUSE",
            Device::Gamepad => "GAMEPAD",
            Device::Layer => "LAYER",
            Device::Modifiers => "MODIFIERS",
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
    const ALL: [MouseKind; 13] = [
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
        MouseKind::Cursor,
        MouseKind::Scroll,
    ];

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
            MouseKind::ButtonLeft => "Left click",
            MouseKind::ButtonRight => "Right click",
            MouseKind::ButtonMiddle => "Middle click",
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
    /// Open the gamepad list over this row.
    Gamepad,
    /// Open the layer list over this row.
    Layer,
    /// Open the modifier list over this row.
    Modifiers,
    /// Take the row's line out of the file.
    Remove,
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
    /// Take the gamepad button its list is on, for the same row.
    TakePad(Pad, Slot),
    /// Make the row hold this layer open.
    TakeLayer(String, Slot),
    /// Add or drop a modifier on the row's key; the list stays up.
    ToggleModifier(Modifier, Slot),
    /// Scope the screen to a layer.
    OpenLayer(String),
    /// Name a new layer.
    NewLayer,
    /// Rename the layer the screen is scoped to.
    RenameLayer,
    /// Listen for the source that will hold the scoped layer open.
    CaptureActivator,
    /// Ask before deleting it.
    AskDelete,
    /// The answer: `true` deletes.
    Delete(bool),
}

/// The source a captured gesture names, or why the map cannot hold it: one
/// target per source, so no hold, chord or modified key.
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
        Some(PadGesture::Tap(pad)) => Ok(Slot::Button(pad)),
        _ => Err("one button at a time, tapped"),
    }
}

/// What an unbound source reads as in the editor's rows.
pub const UNBOUND: &str = "-";

/// The row after the sources, which listens for one to bind.
pub const NEW_SOURCE_ROW: &str = "New binding";

/// The row after the layers that declares one.
pub const NEW_LAYER_ROW: &str = "New layer";

/// What a new layer is called until the keyboard says otherwise.
pub const NEW_LAYER_NAME: &str = "layer";

/// The rows of the deletion question.
pub const DELETE_ROW: &str = "Delete";
pub const CANCEL_ROW: &str = "Cancel";
const CONFIRM_ROWS: [&str; 2] = [DELETE_ROW, CANCEL_ROW];

/// What the rows past the sources are, in screen order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tail {
    /// Listens for a source to bind.
    New,
    /// Opens the layer at this index of [`MapEdit::layers`].
    Layer(usize),
    NewLayer,
    /// Listens for the source that holds the scoped layer open.
    HeldBy,
    Rename,
    Delete,
}

impl Tail {
    pub fn label(self) -> &'static str {
        match self {
            Tail::New => NEW_SOURCE_ROW,
            Tail::NewLayer => NEW_LAYER_ROW,
            Tail::HeldBy => "Held by",
            Tail::Rename => "Rename",
            Tail::Delete => DELETE_ROW,
            // Labelled by the layer's own name.
            Tail::Layer(_) => "",
        }
    }
}

/// One layer, as the base screen lists it.
#[derive(Clone, PartialEq, Debug)]
pub struct LayerRow {
    pub name: String,
    /// The sources that hold it open.
    pub held_by: Vec<Slot>,
    /// Whether it remaps a stick, which no stick direction may open.
    pub has_sticks: bool,
}

impl LayerRow {
    /// Its activators as the screen says them; a layer nothing opens says so.
    pub fn held_by_label(&self) -> String {
        match self.held_by.is_empty() {
            true => "nothing".to_string(),
            false => self
                .held_by
                .iter()
                .map(Slot::label)
                .collect::<Vec<_>>()
                .join(", "),
        }
    }
}

/// What a capture is listening for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum CaptureFor {
    /// A source to bind in the scope.
    Source,
    /// The base source that holds the scoped layer open.
    Activator,
}

/// What a name being typed is for.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LayerName {
    New,
    Rename,
}

pub struct Naming {
    pub what: LayerName,
    pub text: String,
}

/// One source the map binds, as its row shows it — a snapshot, since the map
/// lives in the event handler that resolved it.
pub struct Row {
    pub slot: Slot,
    /// What it sends, as the file spells it.
    pub target: String,
    /// The modifiers a key target is sent with.
    pub mods: KeyMods,
}

impl Row {
    /// What it sends, as the screen says it.
    pub fn label(&self) -> String {
        held_label(target_label(&self.target), self.mods)
    }
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
    /// The device list over the kind list, opened by its Mouse or Gamepad row:
    /// which device it is about, and where its highlight sits.
    device: Option<(Device, usize)>,
    /// What the screen is listening for, if it is.
    capturing: Option<CaptureFor>,
    /// What the last capture was for, so its note shows on the row that asked.
    asked: CaptureFor,
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
    /// The layer the rows belong to; `None` for the base.
    layer: Option<String>,
    /// The map's layers, listed under the base rows.
    layers: Vec<LayerRow>,
    /// The layer just left, whose row takes the highlight once the base rows
    /// are back.
    left: Option<String>,
    /// Whether the scope's rows have been shown, so a new snapshot is an update
    /// rather than a first look.
    shown: bool,
    /// The deletion question's highlight, while it is up.
    confirm: Option<usize>,
    /// A layer name the keyboard is typing.
    naming: Option<Naming>,
}

impl MapEdit {
    pub fn new() -> Self {
        Self {
            visible: false,
            map: String::new(),
            name: String::new(),
            selected: 0,
            kind: None,
            device: None,
            capturing: None,
            asked: CaptureFor::Source,
            fresh: None,
            note: None,
            picking: None,
            picked: None,
            dirty: false,
            rows: vec![],
            layer: None,
            layers: vec![],
            left: None,
            shown: false,
            confirm: None,
            naming: None,
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
        self.device = None;
        self.capturing = None;
        self.fresh = None;
        self.note = None;
        self.picking = None;
        self.picked = None;
        self.dirty = false;
        self.layer = None;
        self.left = None;
        self.shown = false;
        self.confirm = None;
        self.naming = None;
    }

    /// Close it, reporting whether the map needs writing.
    pub fn close(&mut self) -> bool {
        self.visible = false;
        self.kind = None;
        self.device = None;
        self.capturing = None;
        self.fresh = None;
        self.note = None;
        self.picking = None;
        self.layer = None;
        self.confirm = None;
        self.naming = None;
        std::mem::take(&mut self.dirty)
    }

    /// Back out one level: the question, a device list, the kinds, then a
    /// layer. `false` leaves the base rows, which is when the editor closes.
    pub fn back(&mut self) -> bool {
        if self.confirm.take().is_some() || self.device.take().is_some() {
            return true;
        }
        self.fresh = None;
        if self.kind.take().is_some() {
            return true;
        }
        if self.layer.is_some() {
            self.leave_layer();
            return true;
        }
        false
    }

    /// Adopt a fresh snapshot of the map: the rows of the scope, and the
    /// layers. Rows come and go as bindings are made and dropped, so the
    /// highlight can be left past the end.
    pub fn set_rows(&mut self, rows: Vec<Row>, layers: Vec<LayerRow>) {
        // A tail row keeps its place in the tail as the sources above it change.
        let sources = self.rows.len();
        self.rows = rows;
        self.layers = layers;
        self.selected = match self.selected.checked_sub(sources) {
            Some(offset) if self.shown => self.rows.len() + offset,
            _ => self.selected.min(self.rows.len()),
        };
        self.shown = true;
        if let Some(name) = self.left.take() {
            let at = self
                .tail()
                .iter()
                .position(|tail| matches!(tail, Tail::Layer(i) if self.layers[*i].name == name));
            if let Some(at) = at {
                self.selected = self.rows.len() + at;
            }
        }
        self.selected = self.selected.min(self.rows.len() + self.tail().len() - 1);
    }

    /// The rows past the sources: the layers on the base, a layer's own verbs
    /// inside one.
    pub fn tail(&self) -> Vec<Tail> {
        let mut tail = vec![Tail::New];
        match self.layer {
            Some(_) => tail.extend([Tail::HeldBy, Tail::Rename, Tail::Delete]),
            None => {
                tail.extend((0..self.layers.len()).map(Tail::Layer));
                tail.push(Tail::NewLayer);
            }
        }
        tail
    }

    pub fn layers(&self) -> &[LayerRow] {
        &self.layers
    }

    /// The layer the rows belong to; `None` for the base.
    pub fn layer(&self) -> Option<&str> {
        self.layer.as_deref()
    }

    /// Scope the screen to a layer, from its first row.
    pub fn open_layer(&mut self, name: String) {
        self.layer = Some(name);
        self.selected = 0;
        self.shown = false;
        self.close_kinds();
    }

    /// Back to the base rows, with the highlight on the layer just left.
    pub fn leave_layer(&mut self) {
        self.left = self.layer.take();
        self.selected = 0;
        self.shown = false;
        self.confirm = None;
    }

    /// Follow a rename of the layer the screen is scoped to.
    pub fn rename_scope(&mut self, name: String) {
        if self.layer.is_some() {
            self.layer = Some(name);
        }
    }

    /// The scoped layer as the base lists it.
    pub fn scoped_layer(&self) -> Option<&LayerRow> {
        let name = self.layer.as_deref()?;
        self.layers.iter().find(|layer| layer.name == name)
    }

    /// The kinds `slot` is offered here. Layer only on the base, where there
    /// is one it may open: layers do not nest. The modifiers only for a row
    /// that sends a key. Remove only for a bound row.
    pub fn kinds(&self, slot: &Slot) -> Vec<Kind> {
        let mut kinds = Kind::all(slot).to_vec();
        if self.current_kind() == Some(Kind::Key) {
            let at = kinds
                .iter()
                .position(|kind| *kind == Kind::Key)
                .map_or(0, |at| at + 1);
            kinds.insert(at, Kind::Modifiers);
        }
        if self.layer.is_none() && !self.layer_choices(slot).is_empty() {
            let at = kinds.len() - 1;
            kinds.insert(at, Kind::Layer);
        }
        if self.fresh.is_none() {
            kinds.push(Kind::Remove);
        }
        kinds.sort_by_key(|kind| kind.group());
        kinds
    }

    /// The tail row the highlight is on, if it is on one.
    pub fn focused_tail(&self) -> Option<Tail> {
        let offset = self.selected.checked_sub(self.rows.len())?;
        self.tail().get(offset).copied()
    }

    /// What the row the kind list is about sends now, as the file spells it;
    /// `None` for a source that has no row yet.
    pub fn current(&self) -> Option<&str> {
        if self.fresh.is_some() {
            return None;
        }
        self.rows.get(self.selected).map(|row| row.target.as_str())
    }

    /// The modifiers the row's key is sent with; none for a source with no row yet.
    pub fn current_mods(&self) -> KeyMods {
        match self.fresh {
            Some(_) => KeyMods::default(),
            None => self
                .rows
                .get(self.selected)
                .map(|row| row.mods)
                .unwrap_or_default(),
        }
    }

    /// The kind the row sends now, if the list offers it.
    pub fn current_kind(&self) -> Option<Kind> {
        self.current().and_then(kind_of)
    }

    /// What each row of a device list writes, as the file spells it.
    pub fn device_targets(&self, device: Device, slot: &Slot) -> Vec<String> {
        match device {
            Device::Mouse => MouseKind::all(slot)
                .iter()
                .map(|kind| kind.target().to_string())
                .collect(),
            Device::Gamepad => Pad::ALL
                .iter()
                .map(|pad| format!("{PAD_PREFIX}{}", pad.name()))
                .collect(),
            Device::Layer => self
                .layer_choices(slot)
                .iter()
                .map(|name| format!("{LAYER_PREFIX}{name}"))
                .collect(),
            // Toggles, not targets: nothing here is what the row sends.
            Device::Modifiers => vec![],
        }
    }

    /// The layers `slot` may hold open; a stick direction may not open one that
    /// remaps a stick.
    pub fn layer_choices(&self, slot: &Slot) -> Vec<&str> {
        if matches!(slot, Slot::Stick(_)) {
            return vec![];
        }
        let direction = matches!(slot, Slot::Direction(..));
        self.layers
            .iter()
            .filter(|layer| !(direction && layer.has_sticks))
            .map(|layer| layer.name.as_str())
            .collect()
    }

    /// Put the deletion question up, on Cancel.
    pub fn ask_confirm(&mut self) {
        self.confirm = Some(1);
    }

    pub fn close_confirm(&mut self) {
        self.confirm = None;
    }

    pub fn confirming(&self) -> bool {
        self.confirm.is_some()
    }

    /// The question's rows, in screen order.
    pub fn confirm_rows(&self) -> &'static [&'static str] {
        &CONFIRM_ROWS
    }

    /// Hand the keyboard a layer name to edit.
    pub fn start_naming(&mut self, what: LayerName, text: String) {
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

    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    /// Where the highlight is on whichever list is up.
    pub fn selected(&self) -> usize {
        self.confirm
            .or(self.device.map(|(_, at)| at))
            .or(self.kind)
            .unwrap_or(self.selected)
    }

    /// Move it, clamped to the ends of that list.
    pub fn move_sel(&mut self, dy: i32) {
        let at = crate::list::step(self.selected(), dy, self.row_count());
        match (&mut self.confirm, &mut self.device, &mut self.kind) {
            (Some(row), _, _) => *row = at,
            (None, Some((_, row)), _) => *row = at,
            (None, None, Some(row)) => *row = at,
            (None, None, None) => self.selected = at,
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

    /// How many rows whichever list is up has. The source list carries its
    /// [`Tail`] past the ones the map binds.
    fn row_count(&self) -> usize {
        if self.confirm.is_some() {
            return CONFIRM_ROWS.len();
        }
        let sources = self.rows.len() + self.tail().len();
        let Some(slot) = self.slot().filter(|_| self.kind.is_some()) else {
            return sources;
        };
        match self.device {
            Some((Device::Mouse, _)) => MouseKind::all(&slot).len(),
            Some((Device::Gamepad, _)) => Pad::ALL.len(),
            Some((Device::Layer, _)) => self.layer_choices(&slot).len(),
            Some((Device::Modifiers, _)) => Modifier::ALL.len(),
            None => self.kinds(&slot).len(),
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

    /// Which device list is up, if one is — drawn over the kinds.
    pub fn device_open(&self) -> Option<Device> {
        self.device.map(|(device, _)| device)
    }

    /// What **A** takes, or `None` on a list with nothing in it.
    pub fn press(&self) -> Option<EditPress> {
        if let Some(at) = self.confirm {
            return Some(EditPress::Delete(CONFIRM_ROWS[at] == DELETE_ROW));
        }
        let Some(at) = self.kind else {
            if self.slot().is_some() {
                return Some(EditPress::OpenKinds);
            }
            let tail = self.tail();
            return Some(
                match tail.get(self.selected.checked_sub(self.rows.len())?)? {
                    Tail::New => EditPress::StartCapture,
                    Tail::Layer(i) => EditPress::OpenLayer(self.layers[*i].name.clone()),
                    Tail::NewLayer => EditPress::NewLayer,
                    Tail::HeldBy => EditPress::CaptureActivator,
                    Tail::Rename => EditPress::RenameLayer,
                    Tail::Delete => EditPress::AskDelete,
                },
            );
        };
        let slot = self.slot()?;
        match self.device {
            Some((Device::Mouse, at)) => {
                return MouseKind::all(&slot)
                    .get(at)
                    .map(|kind| EditPress::TakeMouse(*kind, slot))
            }
            Some((Device::Gamepad, at)) => {
                return Pad::ALL.get(at).map(|pad| EditPress::TakePad(*pad, slot))
            }
            Some((Device::Modifiers, at)) => {
                return Modifier::ALL
                    .get(at)
                    .map(|modifier| EditPress::ToggleModifier(*modifier, slot))
            }
            Some((Device::Layer, at)) => {
                let name = self.layer_choices(&slot).get(at)?.to_string();
                return Some(EditPress::TakeLayer(name, slot));
            }
            None => {}
        }
        self.kinds(&slot)
            .get(at)
            .map(|kind| EditPress::Take(*kind, slot))
    }

    /// Open the kind list on the kind the row sends now.
    pub fn open_kinds(&mut self) {
        let at = self.slot().and_then(|slot| {
            let current = self.current_kind()?;
            self.kinds(&slot).iter().position(|kind| *kind == current)
        });
        self.kind = Some(at.unwrap_or(0));
    }

    /// Open one device's own list over the kinds, on what the row sends now.
    pub fn open_device(&mut self, device: Device) {
        let at = self.slot().and_then(|slot| {
            let current = self.current()?;
            self.device_targets(device, &slot)
                .iter()
                .position(|target| target == current)
        });
        self.device = Some((device, at.unwrap_or(0)));
    }

    /// Close whatever is over the rows — both lists, since taking a target ends
    /// the whole question.
    pub fn close_kinds(&mut self) {
        self.kind = None;
        self.device = None;
        self.fresh = None;
    }

    /// Listen for a source. Whatever the last attempt had to say is cleared:
    /// the row is being asked again.
    pub fn start_capture(&mut self, purpose: CaptureFor) {
        self.capturing = Some(purpose);
        self.asked = purpose;
        self.note = None;
    }

    /// What the screen is listening for; listening is what routes raw input here.
    pub fn capturing(&self) -> Option<CaptureFor> {
        self.capturing
    }

    /// Stop listening, saying why where the caller has something to say (a
    /// gesture the map cannot hold); a timeout passes `None` and shows nothing.
    pub fn stop_capture(&mut self, note: Option<String>) {
        self.capturing = None;
        self.note = note;
    }

    /// What the row that asked has to say, if the last capture gave nothing.
    pub fn note(&self, row: CaptureFor) -> Option<&str> {
        self.note.as_deref().filter(|_| self.asked == row)
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
        edit.set_rows(rows(), vec![]);
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
            mods: KeyMods::default(),
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
        // Only a source with an edge can stand in for a gamepad button.
        assert!(button.contains(&Kind::Gamepad) && !stick.contains(&Kind::Gamepad));
        assert!(dir.contains(&Kind::Gamepad));
    }

    /// The kinds come in their groups, so the screen can head each one.
    #[test]
    fn the_kinds_come_grouped() {
        let mut edit = opened();
        edit.set_rows(rows(), vec![layer("aim", false)]);
        let groups: Vec<KindGroup> = edit
            .kinds(&Slot::Button(Pad::A))
            .iter()
            .map(|kind| kind.group())
            .collect();
        assert!(groups.is_sorted());
        assert_eq!(groups.last(), Some(&KindGroup::Remove));
        let kinds = edit.kinds(&Slot::Button(Pad::A));
        assert_eq!(Kind::Layer.group(), KindGroup::Send);
        assert!(kinds.contains(&Kind::Layer));
    }

    /// A key row offers its modifiers as one row, which opens a list of toggles
    /// that stays up while they are pressed.
    #[test]
    fn a_key_row_lists_its_modifiers_behind_one_row() {
        let mut edit = MapEdit::new();
        edit.open("keys".to_string(), "Keyboard keys".to_string());
        let slot = Slot::Button(Pad::A);
        let shifted = KeyMods {
            shift: true,
            ..KeyMods::default()
        };
        let key = Row {
            slot: slot.clone(),
            target: "key.1".to_string(),
            mods: shifted,
        };
        edit.set_rows(vec![key], vec![]);
        edit.select_slot(&slot);
        edit.open_kinds();
        let kinds = edit.kinds(&slot);
        assert_eq!(kinds.iter().filter(|k| **k == Kind::Modifiers).count(), 1);
        assert_eq!(mods_label(edit.current_mods()), "Shift");

        edit.open_device(Device::Modifiers);
        assert_eq!(edit.row_count(), Modifier::ALL.len());
        let press = edit.press();
        assert_eq!(
            press,
            Some(EditPress::ToggleModifier(Modifier::ALL[0], slot))
        );
        assert!(edit.back() && edit.kind_open());
    }

    /// Both device lists open over the kinds, and B takes them off again — the
    /// row underneath is untouched until one of their rows is taken.
    #[test]
    fn a_device_list_opens_over_the_kinds_and_backs_out_of_them() {
        let mut edit = opened();
        edit.select(3); // pad.a
        edit.open_kinds();
        for device in [Device::Mouse, Device::Gamepad] {
            edit.open_device(device);
            assert_eq!(edit.device_open(), Some(device));
            assert!(!device.title().is_empty());
            // The row is still the one the kinds were opened over.
            assert_eq!(edit.slot(), Some(Slot::Button(Pad::A)));
            assert!(edit.back() && edit.device_open().is_none());
            assert!(edit.kind_open());
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
    }

    /// The three `up`s a map can hold are three paths in the file and three
    /// different words on screen.
    #[test]
    fn a_source_reads_as_words_and_parses_from_its_path() {
        let slots = ["pad.up", "key.up", "stick.left.up", "stick.right"]
            .map(|path| Slot::from_path(path).expect("a path the file spells"));
        let labels: Vec<String> = slots.iter().map(Slot::label).collect();
        assert_eq!(labels, ["D-pad up", "Up", "Left stick up", "Right stick"]);
        assert_eq!(Slot::from_path("pad.l4"), None);
    }

    /// A target reads as what it does, not as the file spells it.
    #[test]
    fn a_target_reads_as_words() {
        let labels = [
            "key.ArrowUp",
            "key.z",
            "mouse.left",
            "mouse.cursor",
            "pad.l3",
            "layer:aim",
            "passthrough",
            "none",
            DIRECTIONS,
        ]
        .map(target_label);
        assert_eq!(
            labels,
            [
                "Arrow up",
                "Z",
                "Left click",
                "Cursor",
                "Gamepad L3",
                "Layer aim",
                "Passthrough",
                "Ignore",
                "Four directions"
            ]
        );
    }

    /// The row past the ones the map binds is what listens for a new source.
    #[test]
    fn the_trailing_row_captures_and_the_rest_open_their_kinds() {
        let mut edit = opened();
        assert_eq!(edit.press(), Some(EditPress::OpenKinds));
        edit.select(rows().len());
        assert_eq!(edit.slot(), None);
        assert_eq!(edit.press(), Some(EditPress::StartCapture));
    }

    /// A captured source has no row until its kind is taken, so backing out of
    /// the list it opens leaves the map as it was.
    #[test]
    fn a_captured_source_writes_nothing_until_its_kind_is_taken() {
        let mut edit = opened();
        edit.start_capture(CaptureFor::Source);
        assert_eq!(edit.capturing(), Some(CaptureFor::Source));
        edit.stop_capture(None);
        let slot = Slot::Key("q".to_string());
        edit.open_kinds_for(slot.clone());
        assert_eq!(edit.capturing(), None);
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
        edit.start_capture(CaptureFor::Source);
        edit.stop_capture(Some("one source at a time".to_string()));
        assert_eq!(edit.capturing(), None);
        assert_eq!(edit.note(CaptureFor::Source), Some("one source at a time"));
        // The note is for the row that asked, not every row that listens.
        assert_eq!(edit.note(CaptureFor::Activator), None);
        // Asking again drops what the last answer had to say.
        edit.start_capture(CaptureFor::Source);
        assert_eq!(edit.note(CaptureFor::Source), None);
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
        edit.select(rows().len());
        edit.set_rows(vec![], vec![]);
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
        assert_eq!(edit.selected(), rows().len() + 1);
        assert_eq!(edit.press(), Some(EditPress::NewLayer));
        // The kind list moves instead while it is open, and the row stays put.
        edit.select(3);
        let row = edit.selected();
        edit.open_kinds();
        edit.move_sel(99);
        assert_eq!(
            edit.press(),
            Some(EditPress::Take(Kind::Remove, Slot::Button(Pad::A)))
        );
        edit.close_kinds();
        assert_eq!(edit.selected(), row);
    }

    fn layer(name: &str, has_sticks: bool) -> LayerRow {
        LayerRow {
            name: name.to_string(),
            held_by: vec![],
            has_sticks,
        }
    }

    /// The base lists its layers after the row that adds a source, each
    /// opening the same screen scoped to it; B comes back to its row.
    #[test]
    fn a_layer_row_opens_the_screen_scoped_to_it() {
        let mut edit = opened();
        edit.set_rows(rows(), vec![layer("aim", false), layer("mouse", true)]);
        edit.select(rows().len() + 2);
        assert_eq!(
            edit.press(),
            Some(EditPress::OpenLayer("mouse".to_string()))
        );
        edit.open_layer("mouse".to_string());
        edit.set_rows(vec![], vec![layer("aim", false), layer("mouse", true)]);
        assert_eq!(
            edit.tail(),
            [Tail::New, Tail::HeldBy, Tail::Rename, Tail::Delete]
        );
        assert!(edit.back());
        assert_eq!(edit.layer(), None);
        edit.set_rows(rows(), vec![layer("aim", false), layer("mouse", true)]);
        assert_eq!(edit.selected(), rows().len() + 2);
        // Nothing left to back out of: the editor closes.
        assert!(!edit.back());
    }

    /// Layers do not nest, and a stick direction may not open one that remaps
    /// a stick, so neither is offered.
    #[test]
    fn a_source_is_offered_the_layers_it_may_open() {
        let mut edit = opened();
        let a = Slot::Button(Pad::A);
        let up = Slot::Direction(Side::Left, Dir::Up);
        assert!(!edit.kinds(&a).contains(&Kind::Layer));
        edit.set_rows(rows(), vec![layer("aim", false), layer("mouse", true)]);
        assert!(edit.kinds(&a).contains(&Kind::Layer));
        assert!(!edit.kinds(&Slot::Stick(Side::Left)).contains(&Kind::Layer));
        assert_eq!(edit.layer_choices(&a), ["aim", "mouse"]);
        assert_eq!(edit.layer_choices(&up), ["aim"]);
        edit.open_layer("aim".to_string());
        assert!(!edit.kinds(&a).contains(&Kind::Layer));
    }

    /// The layer list writes the activator for the row it was opened over.
    #[test]
    fn the_layer_list_takes_a_layer_for_the_row() {
        let mut edit = opened();
        edit.set_rows(rows(), vec![layer("aim", false)]);
        edit.select(3); // pad.a
        edit.open_kinds();
        edit.open_device(Device::Layer);
        assert_eq!(
            edit.press(),
            Some(EditPress::TakeLayer(
                "aim".to_string(),
                Slot::Button(Pad::A)
            ))
        );
    }

    /// Deleting a layer asks first, and the question opens on Cancel.
    #[test]
    fn a_layer_is_deleted_only_on_a_yes() {
        let mut edit = opened();
        edit.open_layer("aim".to_string());
        edit.set_rows(vec![], vec![layer("aim", false)]);
        edit.move_sel(99);
        assert_eq!(edit.press(), Some(EditPress::AskDelete));
        edit.ask_confirm();
        assert_eq!(edit.press(), Some(EditPress::Delete(false)));
        edit.move_sel(-1);
        assert_eq!(edit.press(), Some(EditPress::Delete(true)));
        assert!(edit.back() && !edit.confirming());
        assert_eq!(edit.layer(), Some("aim"));
    }

    #[test]
    fn no_row_trails_off() {
        for tail in [Tail::New, Tail::NewLayer, Tail::Rename, Tail::Delete] {
            assert!(!tail.label().ends_with('.'), "{tail:?}");
        }
    }

    /// The kind list opens on what the row sends now, and so does a device
    /// list; a source with no row yet has nothing to remove.
    #[test]
    fn a_list_opens_on_what_the_row_sends() {
        let mut edit = opened();
        edit.set_rows(vec![row(Slot::Button(Pad::R2), "mouse.right")], vec![]);
        edit.open_kinds();
        assert_eq!(edit.current_kind(), Some(Kind::Mouse));
        assert_eq!(
            edit.press(),
            Some(EditPress::Take(Kind::Mouse, Slot::Button(Pad::R2)))
        );
        edit.open_device(Device::Mouse);
        assert_eq!(
            edit.press(),
            Some(EditPress::TakeMouse(
                MouseKind::ButtonRight,
                Slot::Button(Pad::R2)
            ))
        );
        assert!(edit
            .kinds(&Slot::Button(Pad::R2))
            .ends_with(&[Kind::Remove]));
        edit.close_kinds();
        edit.open_kinds_for(Slot::Button(Pad::B));
        assert!(!edit.kinds(&Slot::Button(Pad::B)).contains(&Kind::Remove));
        assert_eq!(edit.current(), None);
    }

    /// Inside a layer, the row after the one that binds listens for what opens it.
    #[test]
    fn held_by_listens_for_the_activator() {
        let mut edit = opened();
        edit.open_layer("aim".to_string());
        edit.set_rows(vec![], vec![layer("aim", false)]);
        edit.select(1);
        assert_eq!(edit.focused_tail(), Some(Tail::HeldBy));
        assert_eq!(edit.press(), Some(EditPress::CaptureActivator));
        assert_eq!(
            edit.scoped_layer().map(LayerRow::held_by_label).as_deref(),
            Some("nothing")
        );
    }
}
