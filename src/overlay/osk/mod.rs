//! On-screen keyboard for gamepad text entry, styled after the Steam Deck's.
//! Opened with the **X** button; keys are typed into the address bar (which also
//! doubles as search). Owned and rendered by [`crate::ui`], which drives it from
//! gamepad input. Beyond grid navigation (D-pad + **A**), the common keys have
//! direct shortcuts: **X** backspace, **Y** space, **L2** shift, **R2** enter.
//!
//! Layouts are built in ([`LAYOUTS`]: QWERTY and ЙЦУКЕН so far); the config's
//! `[osk] layouts` list picks which are enabled, and the **Lang** key cycles
//! through them in that order. Each layout defines only the four character
//! rows — the frame (Tab, Caps, Enter, Shift, Space, Fn, arrows) is fixed.
//!
//! The **Fn** key swaps the characters for [`NAMED_ROWS`]: the page gets those
//! as real events, a key picker ([`OskTarget::Capture`]) records them by name.
//!
//! `[osk] style = "wheel"` swaps the grid for a wheel ([`wheel`]); a key
//! picker always gets the grid. Pad intents arrive as [`PadInput`] and the style
//! on screen decides their meaning ([`Osk::read`]).

use crate::browser::{AppBrowser, BrowserCommand};
use crate::command::{AppCommand, GameInputMapsAction, MenuAction, PromptAction};
use crate::config::{Face, OskConfig, OskStyle, PadLayout};
use crate::event::sdl2_servo::{char_keyboard_event, code_for_named, named_keyboard_event};
use keyboard_types::{Code, NamedKey};
use layout::{Layout, LayoutDef, LAYOUTS, NAMED_KEYS};
use std::cell::OnceCell;
use std::str::FromStr;
use wheel::Wheel;

mod layout;
pub mod wheel;

pub use layout::Key;
use Key::*;

/// Where typed input goes: the egui address bar, a borrowed edit buffer, or the
/// page's focused element (via Servo keyboard events). Picked per command by
/// [`crate::ui::AppUi::osk`] from what currently holds focus.
pub enum OskTarget<'a> {
    AddressBar,
    Prompt(&'a mut String),
    /// The start page's search field (see [`crate::overlay::home`]); Enter
    /// submits it as a navigation in the active tab.
    Home(&'a mut String),
    /// The speed-dial editor's URL field (see [`crate::overlay::dial_edit`]);
    /// Enter pins it to the dial rather than navigating.
    DialEdit(&'a mut String),
    /// A settings-overlay text field (see [`crate::overlay::settings`]); Enter
    /// just hides the keyboard (the value already lives in the draft).
    Settings(&'a mut String),
    /// The Game Mode map editor picking a key for a row: the keyboard is a
    /// key *picker* here, so a press is recorded as the map spells it and
    /// nothing reaches the page (see [`crate::overlay::game::map_edit`]).
    Capture(&'a mut Option<String>),
    /// A Game Mode input map name, for a rename or a copy (see
    /// [`crate::overlay::game::input_maps`]); Enter is what commits it to a file.
    GameName(&'a mut String),
    Page,
}

/// An operation on the on-screen keyboard. The router produces these from the
/// contextual buttons, the stick and the dedicated keys, then dispatches them
/// via [`Osk::handle`].
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum OskCommand {
    Show,
    Hide,
    /// Apply the selected key.
    Activate,
    /// Delete the character before the caret.
    Backspace,
    Space,
    /// Set the held-Shift modifier: `true` while the trigger is held.
    Shift(bool),
    /// Hold the wheel's digits layer up: `true` while the trigger is held.
    Digits(bool),
    /// Submit (load the address bar or send Enter), then hide.
    Enter,
    /// Move the selection by one cell (`dx`, `dy` ∈ -1..=1).
    Move(i32, i32),
    /// Apply `key` without selecting it.
    Press(Key),
    /// A wheel face press: the aimed group's corner.
    Face(Face),
}

/// A gamepad input while the keyboard has focus. A/B/X/Y also carry the confirm,
/// cancel, keyboard and hints actions.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum PadInput {
    A,
    B,
    X,
    Y,
    /// L1 (-1) or R1 (+1).
    Shoulder(i32),
    /// L2 held or released.
    LeftTrigger(bool),
    /// R2 held or released.
    RightTrigger(bool),
    /// One navigation step.
    Nav(i32, i32),
    /// A discrete D-pad press.
    Dpad(i32, i32),
    /// The left stick, and the deflection that counts as a push.
    Stick((f32, f32), f32),
    /// Carry the keyboard this many logical px.
    Move(f32, f32),
    /// The right stick's click.
    R3,
    Start,
    Select,
}

/// What the keyboard makes of a [`PadInput`].
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Reading {
    /// Not the keyboard's: the caller keeps its own meaning.
    Pass,
    /// Taken with no command; `true` when it changed what is drawn.
    Drawn(bool),
    Command(OskCommand),
}

/// On-screen keyboard state: visibility, the selected cell, shift/caps, and
/// the enabled layouts.
pub struct Osk {
    pub visible: bool,
    pub caps: bool,
    /// Held Shift from the L2 trigger — a momentary modifier, on while pulled.
    shift_held: bool,
    /// One-shot Shift from the on-screen Shift key — armed by a tap, consumed by
    /// the next character.
    shift_once: bool,
    /// Caret as a char index into the editable target's buffer: typed input and
    /// Backspace act here, `<`/`>` move it. Reset to buffer end on Show, clamped
    /// on use. Page/Settings ignore it, staying append-only as before.
    caret: usize,
    row: usize,
    col: usize,
    /// The enabled layouts in Lang-cycle order; never empty.
    langs: Vec<&'static LayoutDef>,
    /// Index of the active one in [`Self::langs`].
    lang: usize,
    /// Its grid, built on demand and dropped when the keyboard hides: 4 KB of
    /// keys and a shift map for a screen that is usually not up.
    grid: OnceCell<Layout>,
    /// Whether the keyboard is a key picker rather than a keyboard.
    picking: bool,
    /// Whether the picker is on [`NAMED_ROWS`] rather than the characters.
    named: bool,
    style: OskStyle,
    wheel: Wheel,
    /// How far the keyboard was moved from its place at the bottom, in logical px.
    offset: (f32, f32),
    /// The offset's (min, max), as the last frame drew them.
    bounds: Option<((f32, f32), (f32, f32))>,
    clipped: (f32, f32),
}

impl Osk {
    pub fn new(cfg: &OskConfig, pad_layout: PadLayout) -> Self {
        let mut langs: Vec<&'static LayoutDef> = cfg
            .layouts
            .iter()
            .filter_map(|id| {
                let def = LAYOUTS.iter().find(|d| d.name.eq_ignore_ascii_case(id));
                if def.is_none() {
                    let known: Vec<_> = LAYOUTS.iter().map(|d| d.name).collect();
                    log::warn!("osk: unknown layout `{id}` (available: {known:?}); skipping");
                }
                def
            })
            .collect();
        // The keyboard is the only text input on a handheld — never come up
        // without one.
        if langs.is_empty() {
            langs.push(&LAYOUTS[0]);
        }

        Self {
            visible: false,
            caps: false,
            shift_held: false,
            shift_once: false,
            caret: 0,
            // Start on `a` (the home row's first letter), not the top-left
            // backtick; the cell then persists across hide/show.
            row: 2,
            col: 1,
            langs,
            lang: 0,
            grid: OnceCell::new(),
            picking: false,
            named: false,
            style: cfg.style,
            wheel: Wheel::new(pad_layout),
            offset: (0.0, 0.0),
            bounds: None,
            clipped: (0.0, 0.0),
        }
    }

    pub fn set_style(&mut self, style: OskStyle) {
        self.style = style;
        self.wheel.centre();
    }

    pub fn set_pad_layout(&mut self, layout: PadLayout) {
        self.wheel.set_pad_layout(layout);
    }

    /// Whether the wheel is up rather than the grid.
    pub fn wheel(&self) -> bool {
        self.style == OskStyle::Wheel && !self.picking
    }

    /// What a pad input means to the style on screen.
    pub fn read(&mut self, input: PadInput) -> Reading {
        match input {
            PadInput::Move(dx, dy) => {
                let want = (self.offset.0 + dx, self.offset.1 + dy);
                self.offset = self.bounded(want);
                self.clipped = (want.0 - self.offset.0, want.1 - self.offset.1);
                return Reading::Drawn(true);
            }
            PadInput::R3 => {
                self.offset = (0.0, 0.0);
                return Reading::Drawn(true);
            }
            _ => {}
        }
        match self.wheel() {
            true => self.wheel.read(input),
            false => grid_command(input).map_or(Reading::Pass, Reading::Command),
        }
    }

    pub fn offset(&self) -> (f32, f32) {
        self.offset
    }

    /// The part of the last move the bounds held back.
    pub fn clipped(&self) -> (f32, f32) {
        self.clipped
    }

    /// Bound the offset between `min` and `max` from now on; whether that moved it.
    pub fn set_bounds(&mut self, min: (f32, f32), max: (f32, f32)) -> bool {
        self.bounds = Some((min, max));
        let held = self.bounded(self.offset);
        std::mem::replace(&mut self.offset, held) != held
    }

    fn bounded(&self, (x, y): (f32, f32)) -> (f32, f32) {
        match self.bounds {
            Some((min, max)) => (x.clamp(min.0, max.0), y.clamp(min.1, max.1)),
            None => (x, y),
        }
    }

    /// Whether a button `input` means something to the style on screen.
    pub fn takes(&self, input: PadInput) -> bool {
        if matches!(input, PadInput::Move(..) | PadInput::R3) {
            return true;
        }
        match self.wheel() {
            true => self.wheel.command(input),
            false => grid_command(input),
        }
        .is_some()
    }

    /// The active layout, built on demand rather than held: the grid is wanted
    /// only while the keyboard is drawn, and [`Self::hide`] drops it again.
    pub fn layout(&self) -> &Layout {
        self.grid
            .get_or_init(|| Layout::build(self.langs[self.lang]))
    }

    /// Take the keyboard down, dropping the grid with it.
    pub fn hide(&mut self) {
        self.visible = false;
        self.wheel.centre();
        self.grid.take();
    }

    /// The grid on screen: the layout's characters, or the named keys the
    /// [`Key::Fn`] key swaps to.
    pub fn keys(&self) -> &[Vec<Key>] {
        match self.named {
            true => &NAMED_KEYS,
            false => &self.layout().keys,
        }
    }

    pub fn picking(&self) -> bool {
        self.picking
    }

    /// Which grid is up is not its business: that is Fn's, and it persists the
    /// way a layout does.
    pub fn set_picking(&mut self, picking: bool) {
        self.picking = picking;
    }

    /// Swap between the characters and the named keys (the **Fn** key).
    fn toggle_named(&mut self) {
        self.named = !self.named;
        self.clamp_cell();
    }

    /// The grids differ in shape, so every switch between them needs this.
    fn clamp_cell(&mut self) {
        self.row = self.row.min(self.keys().len() - 1);
        self.col = self.col.min(self.keys()[self.row].len() - 1);
    }

    /// Label to show on a key, honoring the current shift/caps state and the
    /// active layout.
    pub fn key_label(&self, key: Key) -> String {
        match key {
            Char(c) => self
                .layout()
                .resolve_char(c, self.shift(), self.caps)
                .to_string(),
            Tab => "Tab".to_string(),
            Caps => "Caps".to_string(),
            Space => "Space".to_string(),
            Backspace => "Bksp".to_string(),
            Shift => "Shift".to_string(),
            Left => "<".to_string(),
            Up => "^".to_string(),
            Down => "v".to_string(),
            Right => ">".to_string(),
            Enter => "Enter".to_string(),
            Lang => self.layout().name.to_uppercase(),
            Clear => "Clr".to_string(),
            Hide => "Hide".to_string(),
            // Labeled with the grid it leads to.
            Fn => match self.named {
                true => "abc".to_string(),
                false => "Fn".to_string(),
            },
            Named { label, .. } => label.to_string(),
        }
    }

    /// Whether Shift is currently in effect: the L2 trigger is held, or the
    /// on-screen Shift key was tapped and not yet consumed.
    pub fn shift(&self) -> bool {
        self.shift_held || self.shift_once
    }

    /// Dispatch an [`OskCommand`]; `target` is where typed input goes (see
    /// [`OskTarget`]).
    pub fn handle(
        &mut self,
        cmd: OskCommand,
        target: OskTarget,
        browser: &AppBrowser,
        commands: &mut Vec<AppCommand>,
    ) {
        match cmd {
            OskCommand::Show => {
                self.visible = true;
                // Start fresh: a trigger released while hidden never sends its
                // release edge, so don't carry a stale held-Shift into a session.
                self.shift_held = false;
                self.shift_once = false;
                // Caret to the buffer end, so typing continues from the text.
                self.caret = target_char_len(&target, browser);
                self.wheel.set_digits(false);
            }
            OskCommand::Hide => self.hide(),
            OskCommand::Activate => self.activate(target, browser, commands),
            OskCommand::Backspace => self.backspace(target, browser),
            OskCommand::Space => self.type_space(target, browser),
            OskCommand::Shift(held) => self.shift_held = held,
            OskCommand::Digits(held) => self.wheel.set_digits(held),
            OskCommand::Enter => self.enter(target, browser, commands),
            OskCommand::Move(dx, dy) => self.move_sel(dx, dy),
            OskCommand::Press(key) => self.press(key, target, browser, commands),
            OskCommand::Face(face) => self.wheel_face(face, target, browser),
        }
    }

    pub fn selected(&self) -> (usize, usize) {
        (self.row, self.col)
    }

    /// Caret position (char index) for the field the OSK types into, so its
    /// egui `TextEdit` can park its cursor to match.
    pub fn caret(&self) -> usize {
        self.caret
    }

    fn current(&self) -> Key {
        self.keys()[self.row][self.col]
    }

    /// Move the selection by one cell; `dx`/`dy` are -1, 0 or 1. The column is
    /// clamped to the (possibly shorter) destination row.
    fn move_sel(&mut self, dx: i32, dy: i32) {
        let rows = self.keys().len() as i32;
        self.row = (self.row as i32 + dy).clamp(0, rows - 1) as usize;
        let cols = self.keys()[self.row].len() as i32;
        self.col = (self.col as i32 + dx).clamp(0, cols - 1) as usize;
    }

    /// Apply the selected key.
    fn activate(
        &mut self,
        target: OskTarget,
        browser: &AppBrowser,
        commands: &mut Vec<AppCommand>,
    ) {
        self.press(self.current(), target, browser, commands);
    }

    /// Apply `key`. Typed input goes to whatever `target` points at (address
    /// bar / prompt dialog / the page, via Servo keyboard events).
    fn press(
        &mut self,
        key: Key,
        target: OskTarget,
        browser: &AppBrowser,
        commands: &mut Vec<AppCommand>,
    ) {
        match key {
            // The on-screen Shift key arms a one-shot Shift (toggle so a mis-tap
            // can be undone); L2 is the held modifier and lives in `shift_held`.
            Shift => self.shift_once = !self.shift_once,
            Caps => self.caps = !self.caps,
            Char(c) => {
                let shift = self.shift();
                let ch = self.layout().resolve_char(c, shift, self.caps);
                self.input_char(target, ch, shift, browser);
                // Consume the one-shot; the held (L2) modifier stays as-is.
                self.shift_once = false;
            }
            Space => self.type_space(target, browser),
            Backspace => self.backspace(target, browser),
            // In an editable field `<`/`>` slide the caret and the egui TextEdit
            // mirrors it; Up/Down do nothing on a single-line field.
            Left if caret_field(&target) => self.caret = self.caret.saturating_sub(1),
            Right if caret_field(&target) => {
                self.caret = (self.caret + 1).min(target_char_len(&target, browser))
            }
            // Picking: the frame keys name themselves, which is how a row gets
            // an arrow or Tab without the grid carrying one.
            Tab | Left | Right | Up | Down if matches!(target, OskTarget::Capture(_)) => {
                let name = match key {
                    Tab => "Tab",
                    Left => "ArrowLeft",
                    Right => "ArrowRight",
                    Up => "ArrowUp",
                    _ => "ArrowDown",
                };
                if let OskTarget::Capture(slot) = target {
                    *slot = Some(name.to_string());
                }
            }
            Tab if matches!(target, OskTarget::Page) => {
                send_named(browser, NamedKey::Tab, Code::Tab)
            }
            Left if matches!(target, OskTarget::Page) => {
                send_named(browser, NamedKey::ArrowLeft, Code::ArrowLeft)
            }
            Right if matches!(target, OskTarget::Page) => {
                send_named(browser, NamedKey::ArrowRight, Code::ArrowRight)
            }
            Up if matches!(target, OskTarget::Page) => {
                send_named(browser, NamedKey::ArrowUp, Code::ArrowUp)
            }
            Down if matches!(target, OskTarget::Page) => {
                send_named(browser, NamedKey::ArrowDown, Code::ArrowDown)
            }
            // Settings has no on-screen caret, so its arrows stay inert.
            Tab | Left | Right | Up | Down => {}
            Enter => self.enter(target, browser, commands),
            Lang => {
                self.lang = (self.lang + 1) % self.langs.len();
                self.grid.take();
                // The frame is fixed but rows differ in length across layouts.
                self.clamp_cell();
            }
            Fn => self.toggle_named(),
            Named { name, .. } => self.input_named(target, name, browser),
            Clear => self.clear_field(target, browser),
            Hide => self.hide(),
        }
    }

    /// Empty the field being typed into (the **Clr** key): our own buffers
    /// directly, a page field through the DOM.
    fn clear_field(&mut self, target: OskTarget, browser: &AppBrowser) {
        match target {
            OskTarget::AddressBar => browser.get_state_mut().location.clear(),
            OskTarget::Prompt(buf)
            | OskTarget::Home(buf)
            | OskTarget::DialEdit(buf)
            | OskTarget::Settings(buf)
            | OskTarget::GameName(buf) => buf.clear(),
            // Picking has no buffer to clear; the row keeps what it had.
            OskTarget::Capture(_) => {}
            OskTarget::Page => browser.clear_focused_field(),
        }
        self.caret = 0;
    }

    /// A picker records the map's spelling, the page gets a real key event, and
    /// an editable field has no use for either.
    fn input_named(&mut self, target: OskTarget, name: &'static str, browser: &AppBrowser) {
        match target {
            OskTarget::Capture(slot) => *slot = Some(name.to_string()),
            OskTarget::Page => {
                // Every entry parses — `NAMED_ROWS` has a test saying so.
                if let Ok(key) = NamedKey::from_str(name) {
                    send_named(browser, key, code_for_named(name));
                }
            }
            _ => {}
        }
    }

    /// Type a space (the **Space** key or **Y**).
    fn type_space(&mut self, target: OskTarget, browser: &AppBrowser) {
        let shift = self.shift();
        self.input_char(target, ' ', shift, browser);
    }

    /// Delete the character before the caret (the **Backspace** key or **X**).
    fn backspace(&mut self, target: OskTarget, browser: &AppBrowser) {
        match target {
            OskTarget::AddressBar => {
                self.caret = remove_before(&mut browser.get_state_mut().location, self.caret)
            }
            OskTarget::Prompt(buf)
            | OskTarget::Home(buf)
            | OskTarget::DialEdit(buf)
            | OskTarget::Settings(buf)
            | OskTarget::GameName(buf) => self.caret = remove_before(buf, self.caret),
            OskTarget::Capture(slot) => *slot = Some("Backspace".to_string()),
            OskTarget::Page => send_named(browser, NamedKey::Backspace, Code::Backspace),
        }
    }

    /// Insert `c` at the caret (advancing it) for an editable field, or send it
    /// to the page as a key event.
    fn input_char(&mut self, target: OskTarget, c: char, shift: bool, browser: &AppBrowser) {
        match target {
            OskTarget::AddressBar => {
                self.caret = insert_at(&mut browser.get_state_mut().location, self.caret, c)
            }
            OskTarget::Prompt(buf)
            | OskTarget::Home(buf)
            | OskTarget::DialEdit(buf)
            | OskTarget::Settings(buf)
            | OskTarget::GameName(buf) => self.caret = insert_at(buf, self.caret, c),
            OskTarget::Capture(slot) => *slot = Some(c.to_string()),
            OskTarget::Page => {
                browser.handle_input(servo::InputEvent::Keyboard(char_keyboard_event(
                    c, shift, true,
                )));
                browser.handle_input(servo::InputEvent::Keyboard(char_keyboard_event(
                    c, shift, false,
                )));
            }
        }
    }

    /// Submit: load the address bar, confirm the prompt dialog, or send Enter
    /// to the page — then hide (the **Go** key or **R2**).
    fn enter(&mut self, target: OskTarget, browser: &AppBrowser, commands: &mut Vec<AppCommand>) {
        match target {
            OskTarget::AddressBar => commands.push(AppCommand::Browser(BrowserCommand::Load)),
            OskTarget::Prompt(_) => commands.push(AppCommand::Prompt(PromptAction::ClickSlot(0))),
            // Submit the start-page search as a navigation in the active tab —
            // the same path a tile uses; a non-URL falls back to a web search.
            OskTarget::Home(buf) => {
                let text = buf.trim();
                if !text.is_empty() {
                    commands.push(AppCommand::Menu(MenuAction::OpenUrl(text.to_string())));
                }
            }
            // The editor's field pins to the speed dial instead of navigating.
            OskTarget::DialEdit(buf) => {
                let text = buf.trim();
                if !text.is_empty() {
                    commands.push(AppCommand::Menu(MenuAction::DialAdd(text.to_string())));
                }
            }
            // A settings text field already holds the typed value in the draft;
            // Enter just dismisses the keyboard, back to the settings list.
            OskTarget::Settings(_) => {}
            // A map name is written to a file, so Enter is what commits
            // it — dismissing the keyboard any other way leaves it alone.
            OskTarget::GameName(buf) => {
                let text = buf.trim();
                if !text.is_empty() {
                    commands.push(AppCommand::GameInputMaps(GameInputMapsAction::Name(
                        text.to_string(),
                    )));
                }
            }
            // The one key the grid cannot otherwise name for a row.
            OskTarget::Capture(slot) => *slot = Some("Enter".to_string()),
            OskTarget::Page => send_named(browser, NamedKey::Enter, Code::Enter),
        }
        self.visible = false;
    }
}

/// What `input` means on the grid.
fn grid_command(input: PadInput) -> Option<OskCommand> {
    let cmd = match input {
        PadInput::A => OskCommand::Activate,
        PadInput::B => OskCommand::Hide,
        PadInput::X => OskCommand::Backspace,
        PadInput::Y => OskCommand::Space,
        PadInput::LeftTrigger(held) => OskCommand::Shift(held),
        PadInput::RightTrigger(true) => OskCommand::Enter,
        PadInput::Nav(dx, dy) => OskCommand::Move(dx, dy),
        // Left to the caller.
        PadInput::Shoulder(_)
        | PadInput::RightTrigger(false)
        | PadInput::Dpad(..)
        | PadInput::Stick(..)
        | PadInput::Move(..)
        | PadInput::R3
        | PadInput::Start
        | PadInput::Select => return None,
    };
    Some(cmd)
}

/// Whether `<`/`>` move the caret for this target: the editable fields that
/// render an egui caret (not Page, not the caret-less Settings rows).
fn caret_field(target: &OskTarget) -> bool {
    matches!(
        target,
        OskTarget::AddressBar | OskTarget::Prompt(_) | OskTarget::Home(_) | OskTarget::DialEdit(_)
    )
}

/// Char length of the target's buffer (0 for the buffer-less `Page`).
fn target_char_len(target: &OskTarget, browser: &AppBrowser) -> usize {
    match target {
        OskTarget::AddressBar => browser.get_state_mut().location.chars().count(),
        OskTarget::Prompt(buf)
        | OskTarget::Home(buf)
        | OskTarget::DialEdit(buf)
        | OskTarget::Settings(buf)
        | OskTarget::GameName(buf) => buf.chars().count(),
        OskTarget::Capture(_) | OskTarget::Page => 0,
    }
}

/// Byte offset of the `n`-th char, or the string's end if `n` is past it.
fn byte_at(s: &str, n: usize) -> usize {
    s.char_indices().nth(n).map(|(b, _)| b).unwrap_or(s.len())
}

/// Insert `c` at char index `caret` (clamped), returning the caret just past it.
fn insert_at(buf: &mut String, caret: usize, c: char) -> usize {
    let caret = caret.min(buf.chars().count());
    buf.insert(byte_at(buf, caret), c);
    caret + 1
}

/// Remove the char before `caret` (clamped), returning the new caret.
fn remove_before(buf: &mut String, caret: usize) -> usize {
    let caret = caret.min(buf.chars().count());
    if caret == 0 {
        return 0;
    }
    buf.remove(byte_at(buf, caret - 1));
    caret - 1
}

fn send_named(browser: &AppBrowser, key: NamedKey, code: Code) {
    browser.handle_input(servo::InputEvent::Keyboard(named_keyboard_event(
        key, code, true,
    )));
    browser.handle_input(servo::InputEvent::Keyboard(named_keyboard_event(
        key, code, false,
    )));
}

#[cfg(test)]
mod tests {
    use super::layout::NAMED_ROWS;
    use super::*;

    fn osk() -> Osk {
        Osk::new(&OskConfig::default(), PadLayout::default())
    }

    /// A name the map cannot parse is a key that silently does nothing, and a
    /// missing `code` is one a game cannot branch on.
    #[test]
    fn every_named_key_is_one_the_map_can_parse() {
        for (label, name) in NAMED_ROWS.iter().flat_map(|row| row.iter()) {
            assert!(NamedKey::from_str(name).is_ok(), "{name}");
            assert_ne!(code_for_named(name), Code::Unidentified, "{name}");
            assert!(!label.is_empty());
        }
    }

    /// The named keys are the rest of the keyboard, not a picker affordance, so
    /// Fn is on every layout whether the keyboard is typing or picking.
    #[test]
    fn every_layout_carries_the_fn_key() {
        let mut osk = osk();
        for _ in 0..osk.langs.len() {
            assert!(osk.keys().iter().flatten().any(|key| *key == Fn));
            osk.lang = (osk.lang + 1) % osk.langs.len();
            osk.grid.take();
        }
        osk.set_picking(true);
        assert!(osk.keys().iter().flatten().any(|key| *key == Fn));
    }

    /// The way back is on the page Fn leads to, or the characters would be
    /// unreachable.
    #[test]
    fn fn_swaps_the_grid_both_ways() {
        let mut osk = osk();
        let chars: Vec<Key> = osk.keys().iter().flatten().copied().collect();
        assert!(!chars.iter().any(|key| matches!(key, Named { .. })));

        osk.toggle_named();
        assert!(osk
            .keys()
            .iter()
            .flatten()
            .any(|key| matches!(key, Named { .. })));
        assert!(osk.keys().iter().flatten().any(|key| *key == Fn));

        osk.toggle_named();
        assert_eq!(
            osk.keys().iter().flatten().copied().collect::<Vec<_>>(),
            chars
        );
    }

    /// A pick that reset the grid would throw away a choice just made.
    #[test]
    fn the_grid_outlives_a_pick() {
        let mut osk = osk();
        osk.set_picking(true);
        osk.toggle_named();
        osk.set_picking(false);
        assert!(osk.named);
    }

    /// A cell valid on one grid can be past the end of another.
    #[test]
    fn switching_grids_keeps_the_selection_on_a_cell() {
        let mut osk = osk();
        osk.row = osk.keys().len() - 1;
        osk.col = osk.keys()[osk.row].len() - 1;
        osk.toggle_named();
        assert!(osk.row < osk.keys().len());
        assert!(osk.col < osk.keys()[osk.row].len());
    }
}
