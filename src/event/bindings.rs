//! Rebindable gamepad buttons, loaded from `bindings.toml` in the user data
//! dir (a template with the defaults is written on first run, like the main
//! config). A binding maps a *gesture* to a semantic [`Action`]:
//!
//! ```toml
//! [gamepad]
//! a = "confirm"             # tap
//! "hold:start" = "bookmark" # hold the button past [gamepad] hold_ms
//! "l1+r1" = "reload"        # chord: press one while holding the other
//! ```
//!
//! Buttons that carry a hold or chord binding can't dispatch their tap on the
//! press edge (the gesture is still ambiguous), so their tap fires on release —
//! see [`crate::event::gamepad`] for the state machine. `confirm` is the one
//! action needing both edges (page clicks/drags), so hold/chord gestures on its
//! button are rejected at load. Unknown names are logged and skipped, never
//! silently dropped.
//!
//! The same file carries a `[keyboard]` table mapping shortcuts to the same
//! actions ([`KeyBindings`]): modifier combos (`"ctrl+r"`) always fire, while
//! plain keys (`"f"`, Vimium-style) are suppressed whenever a text input — on
//! the page or the address bar — holds focus, so typing stays intact.

use crate::app::{AppCommand, InputCommand, MenuAction};
use crate::config;
use crate::osk::OskCommand;
use sdl2::controller::Button;
use sdl2::keyboard::{Keycode, Mod};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};

/// What a gesture does — semantic actions, mapped onto the same commands the
/// hardcoded layout used to emit (so contextual behavior is unchanged).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Action {
    /// Confirm: click / select / activate. Needs both press and release edges.
    Confirm,
    /// Cancel: close the open overlay, otherwise one step back.
    Cancel,
    /// Toggle the on-screen keyboard / backspace while it's open.
    Osk,
    /// Reload the page (space while the on-screen keyboard is open).
    Reload,
    /// Previous: menu section to the left while the menu is open, otherwise
    /// history back.
    Prev,
    /// Next: menu section to the right while the menu is open, otherwise
    /// history forward.
    Next,
    /// Toggle link-hint navigation.
    Hints,
    /// Bookmark the current page.
    Bookmark,
    /// Open / close the full-screen menu.
    Menu,
    /// Switch to the next open tab (wraps around).
    TabNext,
    /// Switch to the previous open tab (wraps around).
    TabPrev,
    /// Overlay navigation by one step (arrow keys by default): menu rows /
    /// sections, the OSK grid, or hint hops — whatever overlay is open. Falls
    /// through to the page when none is.
    NavUp,
    NavDown,
    NavLeft,
    NavRight,
    /// Toggle the D-pad / left stick between moving the cursor and scrolling
    /// the page — the scroll fallback for devices without a right analog
    /// stick. Handled inside the gamepad (it changes how the aim vector is
    /// read), so it never becomes a command.
    Scroll,
    /// Explicitly unbound.
    None,
}

impl Action {
    const ALL: [Action; 17] = [
        Action::Confirm,
        Action::Cancel,
        Action::Osk,
        Action::Reload,
        Action::Prev,
        Action::Next,
        Action::Hints,
        Action::Bookmark,
        Action::Menu,
        Action::TabNext,
        Action::TabPrev,
        Action::NavUp,
        Action::NavDown,
        Action::NavLeft,
        Action::NavRight,
        Action::Scroll,
        Action::None,
    ];

    /// Whether this is an overlay-navigation step (see [`Action::NavUp`]):
    /// these fire only while an overlay is open (otherwise the key goes to the
    /// page) and, unlike other shortcuts, auto-repeat while held.
    pub fn is_nav(self) -> bool {
        matches!(
            self,
            Action::NavUp | Action::NavDown | Action::NavLeft | Action::NavRight
        )
    }

    /// The config-file name of this action; [`Action::parse`] is its inverse,
    /// so typed code (e.g. the default bindings) never spells raw strings.
    pub fn name(self) -> &'static str {
        match self {
            Action::Confirm => "confirm",
            Action::Cancel => "cancel",
            Action::Osk => "osk",
            Action::Reload => "reload",
            Action::Prev => "prev",
            Action::Next => "next",
            Action::Hints => "hints",
            Action::Bookmark => "bookmark",
            Action::Menu => "menu",
            Action::TabNext => "tab_next",
            Action::TabPrev => "tab_prev",
            Action::NavUp => "nav_up",
            Action::NavDown => "nav_down",
            Action::NavLeft => "nav_left",
            Action::NavRight => "nav_right",
            Action::Scroll => "scroll",
            Action::None => "none",
        }
    }

    fn parse(name: &str) -> Option<Action> {
        Self::ALL.into_iter().find(|action| action.name() == name)
    }

    /// Emit this action as a one-shot gesture (keyboard shortcuts): Confirm
    /// sends its press+release pair, everything else fires once.
    pub fn push_tap(self, commands: &mut Vec<AppCommand>) {
        commands.extend(self.command(true));
        if self == Action::Confirm {
            commands.extend(self.command(false));
        }
    }

    /// The command a gesture emits. `pressed` matters only for [`Action::Confirm`]
    /// (the press/release edges of a click); everything else fires once.
    pub fn command(self, pressed: bool) -> Option<AppCommand> {
        Some(match self {
            Action::Confirm => AppCommand::Input(InputCommand::Confirm(pressed)),
            Action::Cancel => AppCommand::Input(InputCommand::Cancel),
            Action::Osk => AppCommand::Input(InputCommand::ToggleOsk),
            // Routed through the contextual OSK-space intent: space while the
            // keyboard is open, reload otherwise — same behavior the hardcoded
            // Y button had.
            Action::Reload => AppCommand::Input(InputCommand::Osk(OskCommand::Space)),
            Action::Prev => AppCommand::Input(InputCommand::Shoulder(-1)),
            Action::Next => AppCommand::Input(InputCommand::Shoulder(1)),
            Action::Hints => AppCommand::Input(InputCommand::Hints),
            Action::Bookmark => AppCommand::ToggleBookmark,
            Action::Menu => AppCommand::Menu(MenuAction::Open),
            Action::TabNext => AppCommand::Input(InputCommand::CycleTab(1)),
            Action::TabPrev => AppCommand::Input(InputCommand::CycleTab(-1)),
            Action::NavUp => AppCommand::Input(InputCommand::Nav(0, -1)),
            Action::NavDown => AppCommand::Input(InputCommand::Nav(0, 1)),
            Action::NavLeft => AppCommand::Input(InputCommand::Nav(-1, 0)),
            Action::NavRight => AppCommand::Input(InputCommand::Nav(1, 0)),
            // Scroll is resolved inside the gamepad, not routed.
            Action::Scroll | Action::None => return None,
        })
    }
}

/// Bindable physical buttons (the D-pad feeds the aim vector and L2/R2 are
/// axes with their own contextual roles, so neither is listed).
pub fn parse_button(name: &str) -> Option<Button> {
    Some(match name {
        "a" => Button::A,
        "b" => Button::B,
        "x" => Button::X,
        "y" => Button::Y,
        "l1" => Button::LeftShoulder,
        "r1" => Button::RightShoulder,
        "l3" => Button::LeftStick,
        "r3" => Button::RightStick,
        "start" => Button::Start,
        "select" => Button::Back,
        _ => return None,
    })
}

/// On-disk shape of `bindings.toml`: one table per input device. BTreeMap so
/// the written template is sorted stably.
#[derive(Default, Serialize, Deserialize)]
struct Store {
    #[serde(default)]
    gamepad: BTreeMap<String, String>,
    #[serde(default)]
    keyboard: BTreeMap<String, String>,
}

/// The stock layout, spelled with the typed [`Action`]s (no raw strings to
/// typo): frequent actions on taps — hints on Y, reload on Start — and the
/// rare bookmark behind a hold.
fn default_gamepad_bindings() -> BTreeMap<String, String> {
    [
        ("a", Action::Confirm),
        ("b", Action::Cancel),
        ("x", Action::Osk),
        ("y", Action::Hints),
        ("l1", Action::Prev),
        ("r1", Action::Next),
        ("l3", Action::Hints),
        ("start", Action::Reload),
        ("hold:start", Action::Bookmark),
        ("hold:y", Action::Scroll),
        ("select", Action::Menu),
    ]
    .into_iter()
    .map(|(gesture, action)| (gesture.to_string(), action.name().to_string()))
    .collect()
}

/// The stock keyboard shortcuts — all behind Ctrl, so none can collide with
/// typing into the page. Plain keys (e.g. Vimium's `f`) are supported in the
/// config; they're just not bound by default.
fn default_keyboard_bindings() -> BTreeMap<String, String> {
    [
        ("ctrl+r", Action::Reload),
        ("ctrl+b", Action::Bookmark),
        ("ctrl+m", Action::Menu),
        ("ctrl+left", Action::Prev),
        ("ctrl+right", Action::Next),
        ("ctrl+f", Action::Hints),
        ("ctrl+t", Action::TabNext),
        ("ctrl+shift+t", Action::TabPrev),
        ("up", Action::NavUp),
        ("down", Action::NavDown),
        ("left", Action::NavLeft),
        ("right", Action::NavRight),
    ]
    .into_iter()
    .map(|(gesture, action)| (gesture.to_string(), action.name().to_string()))
    .collect()
}

fn bindings_path() -> String {
    format!("{}bindings.toml", config::data_dir())
}

/// Load `bindings.toml`. A missing file yields the defaults (and the template
/// is written so there's a file to edit); a malformed one is logged and falls
/// back to the defaults too.
fn load_store() -> Store {
    let path = bindings_path();
    match std::fs::read_to_string(&path) {
        Ok(text) => match toml::from_str(&text) {
            Ok(store) => store,
            Err(e) => {
                log::error!("invalid bindings `{path}`: {e}; using defaults");
                Store::default()
            }
        },
        Err(_) => {
            let store = Store {
                gamepad: default_gamepad_bindings(),
                keyboard: default_keyboard_bindings(),
            };
            match toml::to_string_pretty(&store) {
                Ok(text) => {
                    if let Err(e) = std::fs::write(&path, text) {
                        log::warn!("could not write default bindings `{path}`: {e}");
                    }
                }
                Err(e) => log::warn!("could not serialize default bindings: {e}"),
            }
            store
        }
    }
}

/// The parsed binding tables, ready for per-press lookup.
pub struct Bindings {
    tap: HashMap<Button, Action>,
    hold: HashMap<Button, Action>,
    /// Chords, keyed by the normalized (ordered) button pair.
    chord: HashMap<(Button, Button), Action>,
    /// Buttons whose tap must wait for release: they carry a hold binding or
    /// take part in a chord, so a press alone doesn't identify the gesture yet.
    deferred: HashSet<Button>,
}

impl Bindings {
    /// Load and parse the gamepad table of `bindings.toml`. An empty table
    /// (fresh file without `[gamepad]`) falls back to the defaults — an
    /// unbound pad would be unusable on a handheld.
    pub fn load() -> Self {
        let store = load_store();
        let map = if store.gamepad.is_empty() {
            default_gamepad_bindings()
        } else {
            store.gamepad
        };
        Self::from_map(&map)
    }

    /// Parse a gesture → action map. Invalid entries are logged and skipped;
    /// the resulting table is whatever parsed cleanly.
    fn from_map(map: &BTreeMap<String, String>) -> Self {
        let mut bindings = Self {
            tap: HashMap::new(),
            hold: HashMap::new(),
            chord: HashMap::new(),
            deferred: HashSet::new(),
        };

        for (gesture, action_name) in map {
            let Some(action) = Action::parse(action_name) else {
                log::warn!("bindings: unknown action `{action_name}` for `{gesture}`");
                continue;
            };
            let gesture = gesture.trim().to_ascii_lowercase();
            if let Some(button) = gesture.strip_prefix("hold:").and_then(parse_button) {
                bindings.hold.insert(button, action);
            } else if let Some((first, second)) = gesture.split_once('+') {
                match (parse_button(first.trim()), parse_button(second.trim())) {
                    (Some(a), Some(b)) if a != b => {
                        bindings.chord.insert(normalize(a, b), action);
                    }
                    _ => log::warn!("bindings: invalid chord `{gesture}`"),
                }
            } else if let Some(button) = parse_button(&gesture) {
                bindings.tap.insert(button, action);
            } else {
                log::warn!("bindings: unknown gesture `{gesture}`");
            }
        }

        // `confirm` needs the press edge immediately (clicks, drags): holds and
        // chords on its button would defer it, so they lose and are dropped.
        let confirm: Vec<Button> = bindings
            .tap
            .iter()
            .filter(|(_, action)| **action == Action::Confirm)
            .map(|(button, _)| *button)
            .collect();
        for button in confirm {
            if bindings.hold.remove(&button).is_some() {
                log::warn!("bindings: ignoring hold on the confirm button {button:?}");
            }
            let conflicted: Vec<_> = bindings
                .chord
                .keys()
                .filter(|(a, b)| *a == button || *b == button)
                .copied()
                .collect();
            for pair in conflicted {
                log::warn!("bindings: ignoring chord {pair:?} involving the confirm button");
                bindings.chord.remove(&pair);
            }
        }

        for button in bindings.hold.keys() {
            bindings.deferred.insert(*button);
        }
        for (a, b) in bindings.chord.keys() {
            bindings.deferred.insert(*a);
            bindings.deferred.insert(*b);
        }
        bindings
    }

    pub fn tap(&self, button: Button) -> Option<Action> {
        self.tap.get(&button).copied()
    }

    pub fn hold(&self, button: Button) -> Option<Action> {
        self.hold.get(&button).copied()
    }

    pub fn chord(&self, a: Button, b: Button) -> Option<Action> {
        self.chord.get(&normalize(a, b)).copied()
    }

    /// Whether this button's tap waits for release (hold/chord candidate).
    pub fn is_deferred(&self, button: Button) -> bool {
        self.deferred.contains(&button)
    }
}

/// Chords are order-independent: key them by the (numerically) ordered pair.
fn normalize(a: Button, b: Button) -> (Button, Button) {
    if (a as i32) <= (b as i32) {
        (a, b)
    } else {
        (b, a)
    }
}

/// Modifier bitmask for keyboard shortcuts (left/right variants folded).
const CTRL: u8 = 1;
const ALT: u8 = 2;
const SHIFT: u8 = 4;

fn mods_of(keymod: Mod) -> u8 {
    let mut mods = 0;
    if keymod.intersects(Mod::LCTRLMOD | Mod::RCTRLMOD) {
        mods |= CTRL;
    }
    if keymod.intersects(Mod::LALTMOD | Mod::RALTMOD) {
        mods |= ALT;
    }
    if keymod.intersects(Mod::LSHIFTMOD | Mod::RSHIFTMOD) {
        mods |= SHIFT;
    }
    mods
}

/// Keyboard shortcuts from the `[keyboard]` table: `"ctrl+shift+t"`-style
/// gestures over the same [`Action`]s. Matching is strict (the pressed
/// modifiers must equal the bound ones), so `f` won't fire while Shift is down.
pub struct KeyBindings {
    map: HashMap<(Keycode, u8), Action>,
}

impl KeyBindings {
    /// Load and parse the keyboard table of `bindings.toml`; an empty/absent
    /// table falls back to the defaults.
    pub fn load() -> Self {
        let store = load_store();
        let map = if store.keyboard.is_empty() {
            default_keyboard_bindings()
        } else {
            store.keyboard
        };
        Self::from_map(&map)
    }

    fn from_map(map: &BTreeMap<String, String>) -> Self {
        let mut table = HashMap::new();
        for (gesture, action_name) in map {
            let Some(action) = Action::parse(action_name) else {
                log::warn!("key bindings: unknown action `{action_name}` for `{gesture}`");
                continue;
            };
            if action == Action::Scroll {
                log::warn!("key bindings: `scroll` is gamepad-only; ignoring `{gesture}`");
                continue;
            }
            let mut mods = 0u8;
            let mut key = None;
            for token in gesture.trim().to_ascii_lowercase().split('+') {
                match token.trim() {
                    "ctrl" => mods |= CTRL,
                    "alt" => mods |= ALT,
                    "shift" => mods |= SHIFT,
                    token => match (parse_key(token), key) {
                        (Some(parsed), None) => key = Some(parsed),
                        _ => {
                            key = None;
                            break;
                        }
                    },
                }
            }
            match key {
                Some(key) => _ = table.insert((key, mods), action),
                None => log::warn!("key bindings: invalid shortcut `{gesture}`"),
            }
        }
        Self { map: table }
    }

    /// The bound action for this key event, plus whether the binding is a
    /// *plain* one (no Ctrl/Alt) — those are only safe outside text inputs.
    pub fn lookup(&self, key: Keycode, keymod: Mod) -> Option<(Action, bool)> {
        let mods = mods_of(keymod);
        self.map
            .get(&(key, mods))
            .map(|action| (*action, mods & (CTRL | ALT) == 0))
    }
}

/// Resolve a key name: a few friendly aliases, then SDL's own key names
/// (case-normalized, so `f`, `left`, and `f5` all work).
fn parse_key(name: &str) -> Option<Keycode> {
    let name = match name {
        "esc" => "Escape",
        "enter" => "Return",
        "pageup" => "PageUp",
        "pagedown" => "PageDown",
        other => other,
    };
    Keycode::from_name(name)
        .or_else(|| Keycode::from_name(&name.to_ascii_uppercase()))
        .or_else(|| {
            let mut chars = name.chars();
            let first = chars.next()?.to_ascii_uppercase();
            Keycode::from_name(&format!("{first}{}", chars.as_str()))
        })
}
