//! retsurf's bindable vocabulary and its `bindings.toml`, over [`inputbind`].
//! The gesture machine, the file, capture and the editor model all live there;
//! this supplies the three things it asks of a host — an [`Action`] set, the
//! default [`Store`], and a key-name resolver.
//!
//! ```toml
//! [gamepad]
//! a = "confirm"             # tap
//! "hold:start" = "reload"   # hold past [input] hold_ms
//! "l1+r1" = "zoom_reset"    # chord: press R1 while holding L1
//!
//! [keyboard]
//! "ctrl+r" = "reload"
//! ```
//!
//! Plain key gestures (no Ctrl/Alt, Vimium-style) are muted while a text input
//! holds focus; see [`crate::event::keyboard`].

use crate::browser::BrowserCommand;
use crate::command::{AppCommand, InputCommand, MenuAction, SettingsAction};
use crate::config;
use crate::overlay::osk::OskCommand;
use inputbind::editor::{Groups, Requirement};
use inputbind::sdl::KeyNames;
use inputbind::{Action as Bindable, Bindings, Store};

/// Generate [`Action`], its `bindings.toml` tokens and Controls labels, and
/// [`GROUPS`] from one table. Listing an action under a group is what makes it
/// exist: [`ALL`] flattens [`GROUPS`].
macro_rules! action_table {
    (
        $(
            $group:literal => {
                $(
                    $(#[$vmeta:meta])*
                    $variant:ident => $token:literal, $label:literal,
                )+
            }
        )+
    ) => {
        /// What a gesture does — semantic actions, mapped onto the same commands
        /// the hardcoded layout used to emit (so contextual behavior is unchanged).
        #[derive(Clone, Copy, PartialEq, Eq, Debug)]
        pub enum Action {
            $( $( $(#[$vmeta])* $variant, )+ )+
        }

        impl Action {
            /// The stable `bindings.toml` token.
            const fn token(self) -> &'static str {
                match self { $( $( Action::$variant => $token, )+ )+ }
            }

            /// Friendly label for the settings UI (the Controls rows).
            const fn label(self) -> &'static str {
                match self { $( $( Action::$variant => $label, )+ )+ }
            }
        }

        /// The Controls screen's sections, in display order (actions sort by name
        /// within each). Also the source [`ALL`] flattens; a duplicate listing is
        /// an unreachable match arm at compile time.
        pub const GROUPS: Groups<Action> = &[
            $( ($group, &[ $( Action::$variant, )+ ]), )+
        ];
    };
}

action_table! {
    "General" => {
        /// Confirm: click / select / activate. Needs both press and release edges.
        Confirm => "confirm", "Confirm",
        /// Cancel: close the open overlay, otherwise one step back.
        Cancel => "cancel", "Cancel",
        /// Open / close the full-screen menu.
        Menu => "menu", "Menu",
        /// Open the settings overlay (see [`crate::overlay::settings`]).
        Settings => "settings", "Settings",
        /// Toggle the on-screen keyboard / backspace while it's open.
        Osk => "osk", "Keyboard",
        /// The Game Mode gesture, resolved against the mode's state: enter it, open
        /// its menu inside, or close that menu. Leaving is the menu's Exit row, so
        /// one gesture covers the whole mode (see [`crate::app`]).
        GameMode => "game_mode", "Game Mode",
        /// Quit immediately. Unbound by default; [`default_store`] carries the
        /// stock exit.
        Quit => "quit", "Quit",
    }
    "Navigation" => {
        /// Previous: menu section to the left while the menu is open, otherwise
        /// history back.
        Prev => "prev", "Back / prev",
        /// Next: menu section to the right while the menu is open, otherwise
        /// history forward.
        Next => "next", "Forward / next",
        /// Navigate the active tab to the configured home page.
        Home => "home", "Home",
        /// Toggle link-hint navigation.
        Hints => "hints", "Link hints",
        /// Toggle the D-pad / left stick between cursor and page scroll, for devices
        /// with no right stick. Latched inside the gamepad, never a command.
        Scroll => "scroll", "Scroll toggle",
        /// Overlay navigation by one step: menu rows /
        /// sections, the OSK grid, or hint hops — whatever overlay is open. Falls
        /// through to the page when none is.
        NavUp => "nav_up", "Nav up",
        /// Overlay navigation one step down; see [`Action::NavUp`].
        NavDown => "nav_down", "Nav down",
        /// Overlay navigation one step left; see [`Action::NavUp`].
        NavLeft => "nav_left", "Nav left",
        /// Overlay navigation one step right; see [`Action::NavUp`].
        NavRight => "nav_right", "Nav right",
    }
    "Page" => {
        /// Reload the page (space while the on-screen keyboard is open).
        Reload => "reload", "Reload",
        /// Toggle reader mode on the current page.
        Reader => "reader", "Reader mode",
        /// Bookmark the current page.
        Bookmark => "bookmark", "Bookmark",
        /// Step the page zoom up the ladder.
        ZoomIn => "zoom_in", "Zoom in",
        /// Step the page zoom down the ladder.
        ZoomOut => "zoom_out", "Zoom out",
        /// Return the page zoom to the config default.
        ZoomReset => "zoom_reset", "Zoom reset",
    }
    "Tabs" => {
        /// Switch to the next open tab (wraps around).
        TabNext => "tab_next", "Next tab",
        /// Switch to the previous open tab (wraps around).
        TabPrev => "tab_prev", "Previous tab",
        /// Open a new tab at the home page.
        NewTab => "new_tab", "New tab",
    }
}

/// Every action, flattened from [`GROUPS`] — an action missing there could
/// neither parse from `bindings.toml` nor reach the Controls screen.
const ALL: [Action; group_len()] = flatten_groups();

const fn group_len() -> usize {
    let mut n = 0;
    let mut g = 0;
    while g < GROUPS.len() {
        n += GROUPS[g].1.len();
        g += 1;
    }
    n
}

const fn flatten_groups() -> [Action; group_len()] {
    let mut out = [Action::Confirm; group_len()];
    let mut i = 0;
    let mut g = 0;
    while g < GROUPS.len() {
        let members = GROUPS[g].1;
        let mut m = 0;
        while m < members.len() {
            out[i] = members[m];
            i += 1;
            m += 1;
        }
        g += 1;
    }
    out
}

impl Bindable for Action {
    fn name(&self) -> &'static str {
        self.token()
    }

    fn parse(name: &str) -> Option<Action> {
        ALL.into_iter().find(|action| action.name() == name)
    }

    fn all() -> &'static [Action] {
        &ALL
    }

    fn display(&self) -> &'static str {
        self.label()
    }

    fn repeats(&self) -> bool {
        self.is_nav()
    }

    fn is_held(&self) -> bool {
        *self == Action::Confirm
    }

    fn needs_press_edge(&self) -> bool {
        self.is_held() || self.is_nav()
    }
}

impl Action {
    /// Whether this is an overlay-navigation step (see [`Action::NavUp`]):
    /// these fire only while an overlay is open (otherwise the key goes to the
    /// page) and, unlike other shortcuts, auto-repeat while held.
    pub fn is_nav(self) -> bool {
        matches!(
            self,
            Action::NavUp | Action::NavDown | Action::NavLeft | Action::NavRight
        )
    }

    /// For the key path, which has no gesture machine to pair the edges: a held
    /// action sends both at once.
    pub fn push_tap(self, commands: &mut Vec<AppCommand>) {
        commands.extend(self.command(true));
        if self.is_held() {
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
            // Through the contextual OSK-space intent: space while the keyboard
            // is open, reload otherwise — what the hardcoded Y button did.
            Action::Reload => AppCommand::Input(InputCommand::Osk(OskCommand::Space)),
            Action::Prev => AppCommand::Input(InputCommand::Shoulder(-1)),
            Action::Next => AppCommand::Input(InputCommand::Shoulder(1)),
            Action::Hints => AppCommand::Input(InputCommand::Hints),
            Action::Bookmark => AppCommand::ToggleBookmark,
            Action::Home => AppCommand::Browser(BrowserCommand::Home),
            Action::Reader => AppCommand::Browser(BrowserCommand::Reader),
            Action::Menu => AppCommand::Menu(MenuAction::Open),
            Action::Settings => AppCommand::Settings(SettingsAction::Open),
            Action::Quit => AppCommand::Shutdown,
            Action::GameMode => AppCommand::GameMode,
            Action::TabNext => AppCommand::Input(InputCommand::CycleTab(1)),
            Action::TabPrev => AppCommand::Input(InputCommand::CycleTab(-1)),
            Action::NewTab => AppCommand::Menu(MenuAction::NewTab),
            Action::ZoomIn => AppCommand::Browser(BrowserCommand::Zoom(1)),
            Action::ZoomOut => AppCommand::Browser(BrowserCommand::Zoom(-1)),
            Action::ZoomReset => AppCommand::Browser(BrowserCommand::Zoom(0)),
            Action::NavUp => AppCommand::Input(InputCommand::Nav(0, -1)),
            Action::NavDown => AppCommand::Input(InputCommand::Nav(0, 1)),
            Action::NavLeft => AppCommand::Input(InputCommand::Nav(-1, 0)),
            Action::NavRight => AppCommand::Input(InputCommand::Nav(1, 0)),
            // Scroll is resolved inside the gamepad, not routed.
            Action::Scroll => return None,
        })
    }
}

/// What the pad must keep, whatever else is rebound: a handheld has no keyboard
/// or mouse, so losing these strands the user on the screen that took them away.
pub const REQUIRED: &[Requirement<Action>] = &[
    ("Confirm", &[Action::Confirm]),
    ("Cancel", &[Action::Cancel]),
    ("Opening settings", &[Action::Settings]),
];

/// Per-surface override tables (`[surface.<name>]`). None: the router is what
/// makes an action contextual, so a binding means the same thing everywhere.
pub const SURFACES: &[&str] = &[];

/// The stock layout. Chords are ordered and a two-shoulder squeeze is not, so
/// both orders are bound — which also defers both pads, as the old chord did.
fn default_gamepad_bindings() -> inputbind::Table {
    [
        ("a", Action::Confirm),
        ("b", Action::Cancel),
        // The only free hold slot that isn't a stickless-unfriendly stick click.
        ("hold:b", Action::Home),
        ("x", Action::Osk),
        ("y", Action::Hints),
        ("l1", Action::Prev),
        ("r1", Action::Next),
        // These defer their taps to release; back/forward survive that fine.
        ("hold:l1", Action::ZoomOut),
        ("hold:r1", Action::ZoomIn),
        // Completes the zoom set; otherwise gamepad-unreachable (ctrl+0 only).
        ("l1+r1", Action::ZoomReset),
        ("r1+l1", Action::ZoomReset),
        ("l3", Action::Hints),
        ("r3", Action::Settings),
        // Scroll mode is how stickless devices scroll; both gestures defer.
        ("start", Action::Scroll),
        ("hold:start", Action::Reload),
        // On a hold so stickless devices (no R3) have reader out of the box.
        ("hold:x", Action::Reader),
        ("hold:y", Action::Bookmark),
        ("select", Action::Menu),
        ("hold:select", Action::Settings),
        // The pad's way in; the way out inside is hold:select, hardcoded there
        // because Game Mode bypasses these tables (see `event::game_mode`).
        ("select+y", Action::GameMode),
        // Pressed again while settings is open this quits — the only gamepad
        // exit on a handheld. Bind `quit` directly for a one-press exit.
        ("select+start", Action::Settings),
        ("start+select", Action::Settings),
    ]
    .into_iter()
    .map(|(gesture, action)| (gesture.to_string(), action.name().to_string()))
    .collect()
}

/// The stock keyboard shortcuts. Ctrl combos always fire; the plain keys are
/// muted while a text input holds focus, so they can't collide with typing.
fn default_keyboard_bindings() -> inputbind::Table {
    [
        ("ctrl+r", Action::Reload),
        ("ctrl+b", Action::Bookmark),
        ("ctrl+h", Action::Home),
        ("ctrl+e", Action::Reader),
        ("ctrl+m", Action::Menu),
        ("ctrl+,", Action::Settings),
        // A Ctrl+Alt chord because no game binds one, and inside Game Mode this
        // is the only key the browser still answers.
        ("ctrl+alt+g", Action::GameMode),
        ("ctrl+left", Action::Prev),
        ("ctrl+right", Action::Next),
        ("ctrl+t", Action::TabNext),
        ("ctrl+shift+t", Action::TabPrev),
        ("t", Action::NewTab),
        ("ctrl+=", Action::ZoomIn),
        ("ctrl+-", Action::ZoomOut),
        ("ctrl+0", Action::ZoomReset),
        // Vimium-style plain keys (muted while typing).
        ("f", Action::Hints),
        ("enter", Action::Confirm),
        ("backspace", Action::Cancel),
        // Navigation: arrows and vim hjkl move overlays (page when none is open).
        ("up", Action::NavUp),
        ("down", Action::NavDown),
        ("left", Action::NavLeft),
        ("right", Action::NavRight),
        ("k", Action::NavUp),
        ("j", Action::NavDown),
        ("h", Action::NavLeft),
        ("l", Action::NavRight),
    ]
    .into_iter()
    .map(|(gesture, action)| (gesture.to_string(), action.name().to_string()))
    .collect()
}

pub fn default_store() -> Store {
    Store {
        gamepad: default_gamepad_bindings(),
        keyboard: default_keyboard_bindings(),
        surface: Default::default(),
    }
}

fn bindings_path() -> String {
    format!("{}bindings.toml", config::data_dir())
}

/// Load `bindings.toml`, writing the defaults as a template on first run and
/// merging in the ones an older file predates (see [`merge_missing_defaults`]).
pub fn load_store() -> Store {
    let mut store = Store::load(bindings_path(), default_store);
    merge_missing_defaults(&mut store);
    store
}

/// Give back the default gestures of an action with no binding at all in that
/// device's table: the file is written only when absent, so an action added after
/// a user's first run would otherwise ship unreachable.
fn merge_missing_defaults(store: &mut Store) {
    for (table, defaults) in [
        (&mut store.gamepad, default_gamepad_bindings()),
        (&mut store.keyboard, default_keyboard_bindings()),
    ] {
        // Snapshot first, so every gesture of an unbound action comes back
        // together — the first insertion would otherwise hide the rest.
        let bound: Vec<String> = table.values().cloned().collect();
        let missing: Vec<(String, String)> = defaults
            .into_iter()
            .filter(|(gesture, action)| !table.contains_key(gesture) && !bound.contains(action))
            .collect();
        for (gesture, action) in missing {
            log::info!("bindings: `{action}` had nothing bound; restoring `{gesture}`");
            table.insert(gesture, action);
        }
    }
}

/// Write an edited store back (the settings overlay saving on close).
pub fn save(store: &Store) {
    store.save(bindings_path());
}

/// Parse a store into the runtime tables; `keys` comes from SDL once at startup.
pub fn build(store: &Store, keys: &KeyNames) -> Bindings<Action> {
    Bindings::new(store, SURFACES, |name| keys.code(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_action_is_listed_in_exactly_one_group() {
        for action in ALL {
            let groups: Vec<&str> = GROUPS
                .iter()
                .filter(|(_, members)| members.contains(&action))
                .map(|(name, _)| *name)
                .collect();
            assert_eq!(
                groups.len(),
                1,
                "`{}` is in {groups:?}, not exactly one group",
                action.name()
            );
        }
    }

    #[test]
    fn every_action_round_trips_through_its_config_name() {
        for action in ALL {
            assert_eq!(Action::parse(action.name()), Some(action));
        }
        assert_eq!(Action::parse("fly"), None);
        // `none` is the store's unbound sentinel, never an action.
        assert_eq!(Action::parse(inputbind::UNBOUND), None);
    }

    /// A gesture the tables would drop at load must fail here, not on-device.
    #[test]
    fn the_default_gamepad_layout_survives_the_gesture_rules() {
        let store = default_store();
        // A stub resolver: only the gamepad table is under test.
        let bindings = Bindings::new(&store, SURFACES, |name| name.bytes().next().map(u32::from));
        for (text, name) in &store.gamepad {
            let gesture = inputbind::PadGesture::parse(text)
                .unwrap_or_else(|| panic!("`{text}` is not a gesture"));
            let action = Action::parse(name).unwrap_or_else(|| panic!("`{name}` is not an action"));
            let bound = match gesture {
                inputbind::PadGesture::Tap(pad) => bindings.tap(pad, None),
                inputbind::PadGesture::Hold(pad) => bindings.hold(pad),
                inputbind::PadGesture::Chord(a, b) => bindings.chord(a, b),
            };
            assert_eq!(bound, Some(action), "`{text}` was dropped at load");
        }
    }

    #[test]
    fn the_defaults_meet_every_requirement() {
        assert!(inputbind::editor::meets_every_requirement(
            &default_store().gamepad,
            REQUIRED
        ));
    }

    /// An unknown key name is only logged, so a typo would ship as a dead shortcut.
    #[test]
    fn every_default_key_gesture_resolves_through_sdl() {
        let names = KeyNames::new();
        let store = default_store();
        let bindings = build(&store, &names);
        for (text, name) in &store.keyboard {
            let gesture = inputbind::KeyGesture::parse(text)
                .unwrap_or_else(|| panic!("`{text}` is not a key gesture"));
            let code = names
                .code(&gesture.name)
                .unwrap_or_else(|| panic!("SDL has no key `{}` (`{text}`)", gesture.name));
            let action = Action::parse(name).unwrap_or_else(|| panic!("`{name}` is not an action"));
            assert_eq!(bindings.key(code, gesture.mods), Some(action), "`{text}`");
        }
    }

    /// The one key that still fires inside Game Mode, where every other one goes
    /// to the page — so a plain key would be one the game wanted.
    #[test]
    fn the_game_mode_key_carries_a_modifier() {
        let store = default_store();
        let mut found = 0;
        for (text, name) in &store.keyboard {
            if Action::parse(name) != Some(Action::GameMode) {
                continue;
            }
            found += 1;
            let gesture = inputbind::KeyGesture::parse(text)
                .unwrap_or_else(|| panic!("`{text}` is not a key gesture"));
            assert!(!gesture.mods.is_plain(), "`{text}` is a plain key");
        }
        assert_eq!(found, 1, "game_mode needs exactly one default key");
    }

    /// The case this exists for: a file written before `game_mode` existed. It
    /// is unreachable on both devices until the defaults are merged back.
    #[test]
    fn an_action_the_file_predates_gets_its_defaults_back() {
        let mut store = default_store();
        store.gamepad.retain(|_, name| name != "game_mode");
        store.keyboard.retain(|_, name| name != "game_mode");
        merge_missing_defaults(&mut store);
        assert_eq!(store, default_store());
    }

    /// A rebound action is not missing, so the user's choice survives — and the
    /// *other* device still gets its default, which is how one file can be half
    /// upgraded (measured on a real one, 2026-09-15).
    #[test]
    fn a_rebound_action_is_left_alone_device_by_device() {
        let mut store = default_store();
        store.keyboard.retain(|_, name| name != "game_mode");
        store.keyboard.insert("ctrl+g".into(), "game_mode".into());
        store.gamepad.retain(|_, name| name != "game_mode");
        merge_missing_defaults(&mut store);
        assert_eq!(
            store.keyboard.get("ctrl+g").map(String::as_str),
            Some("game_mode")
        );
        assert_eq!(store.keyboard.get("ctrl+alt+g"), None);
        assert_eq!(
            store.gamepad.get("select+y").map(String::as_str),
            Some("game_mode")
        );
    }

    /// A gesture the file already spells is the user's, whatever it names — the
    /// merge may never take one back.
    #[test]
    fn a_taken_gesture_is_never_reclaimed() {
        let mut store = default_store();
        store.gamepad.retain(|_, name| name != "game_mode");
        store.gamepad.insert("select+y".into(), "reader".into());
        merge_missing_defaults(&mut store);
        assert_eq!(
            store.gamepad.get("select+y").map(String::as_str),
            Some("reader")
        );
    }

    /// Merging the stock file changes nothing, so it cannot churn on launch.
    #[test]
    fn merging_the_defaults_is_a_no_op() {
        let mut store = default_store();
        merge_missing_defaults(&mut store);
        assert_eq!(store, default_store());
    }

    /// `scroll` latches inside the pad, so a key bound to it would do nothing.
    #[test]
    fn no_default_keyboard_binding_is_a_no_op() {
        for (text, name) in &default_store().keyboard {
            let action = Action::parse(name).unwrap_or_else(|| panic!("`{name}` is not an action"));
            assert!(
                action.command(true).is_some(),
                "`{text}` is bound to `{name}`, which does nothing from a key"
            );
        }
    }
}
