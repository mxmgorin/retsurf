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
//! [`Action`] itself is input-device-agnostic (it only names a command), so a
//! future `[keyboard.bindings]` can reuse it with its own gesture parser.

use crate::app::{AppCommand, InputCommand, MenuAction};
use crate::config;
use crate::osk::OskCommand;
use sdl2::controller::Button;
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
    /// Toggle the D-pad / left stick between moving the cursor and scrolling
    /// the page — the scroll fallback for devices without a right analog
    /// stick. Handled inside the gamepad (it changes how the aim vector is
    /// read), so it never becomes a command.
    Scroll,
    /// Explicitly unbound.
    None,
}

impl Action {
    const ALL: [Action; 11] = [
        Action::Confirm,
        Action::Cancel,
        Action::Osk,
        Action::Reload,
        Action::Prev,
        Action::Next,
        Action::Hints,
        Action::Bookmark,
        Action::Menu,
        Action::Scroll,
        Action::None,
    ];

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
            Action::Scroll => "scroll",
            Action::None => "none",
        }
    }

    fn parse(name: &str) -> Option<Action> {
        Self::ALL.into_iter().find(|action| action.name() == name)
    }

    /// The command a gesture emits. `pressed` matters only for [`Action::Confirm`]
    /// (the press/release edges of a click); everything else fires once.
    pub fn command(self, pressed: bool) -> Option<AppCommand> {
        Some(match self {
            Action::Confirm => AppCommand::Input(InputCommand::Primary(pressed)),
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

/// On-disk shape of `bindings.toml`: one table per input device (a keyboard
/// table can join later). BTreeMap so the written template is sorted stably.
#[derive(Default, Serialize, Deserialize)]
struct Store {
    #[serde(default)]
    gamepad: BTreeMap<String, String>,
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
