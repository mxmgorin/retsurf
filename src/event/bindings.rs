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

use crate::app::{AppCommand, InputCommand, MenuAction, SettingsAction};
use crate::browser::BrowserCommand;
use crate::config;
use crate::overlay::osk::OskCommand;
use inputbind::editor::{Groups, Requirement};
use inputbind::sdl::KeyNames;
use inputbind::{Action as Bindable, Bindings, Store};

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
    /// Navigate the active tab to the configured home page.
    Home,
    /// Toggle reader mode on the current page.
    Reader,
    /// Open / close the full-screen menu.
    Menu,
    /// Open the settings overlay (see [`crate::overlay::settings`]).
    Settings,
    /// Quit immediately. Unbound by default: the stock exit is a second
    /// Select+Start while settings is open (see [`default_store`]).
    Quit,
    /// Switch to the next open tab (wraps around).
    TabNext,
    /// Switch to the previous open tab (wraps around).
    TabPrev,
    /// Open a new tab at the home page.
    NewTab,
    /// Step the page zoom up / down the ladder, or back to the config default.
    ZoomIn,
    ZoomOut,
    ZoomReset,
    /// Overlay navigation by one step (arrow keys by default): menu rows /
    /// sections, the OSK grid, or hint hops — whatever overlay is open. Falls
    /// through to the page when none is.
    NavUp,
    NavDown,
    NavLeft,
    NavRight,
    /// Toggle the D-pad / left stick between cursor and page scroll, for devices
    /// with no right stick. Latched inside the gamepad, never a command.
    Scroll,
}

/// Every action. [`GROUPS`] decides display order, so this only has to be complete.
const ALL: [Action; 24] = [
    Action::Confirm,
    Action::Cancel,
    Action::Osk,
    Action::Reload,
    Action::Prev,
    Action::Next,
    Action::Hints,
    Action::Bookmark,
    Action::Home,
    Action::Reader,
    Action::Menu,
    Action::Settings,
    Action::Quit,
    Action::TabNext,
    Action::TabPrev,
    Action::NewTab,
    Action::ZoomIn,
    Action::ZoomOut,
    Action::ZoomReset,
    Action::NavUp,
    Action::NavDown,
    Action::NavLeft,
    Action::NavRight,
    Action::Scroll,
];

impl Bindable for Action {
    fn name(&self) -> &'static str {
        match self {
            Action::Confirm => "confirm",
            Action::Cancel => "cancel",
            Action::Osk => "osk",
            Action::Reload => "reload",
            Action::Prev => "prev",
            Action::Next => "next",
            Action::Hints => "hints",
            Action::Bookmark => "bookmark",
            Action::Home => "home",
            Action::Reader => "reader",
            Action::Menu => "menu",
            Action::Settings => "settings",
            Action::Quit => "quit",
            Action::TabNext => "tab_next",
            Action::TabPrev => "tab_prev",
            Action::NewTab => "new_tab",
            Action::ZoomIn => "zoom_in",
            Action::ZoomOut => "zoom_out",
            Action::ZoomReset => "zoom_reset",
            Action::NavUp => "nav_up",
            Action::NavDown => "nav_down",
            Action::NavLeft => "nav_left",
            Action::NavRight => "nav_right",
            Action::Scroll => "scroll",
        }
    }

    fn parse(name: &str) -> Option<Action> {
        ALL.into_iter().find(|action| action.name() == name)
    }

    fn all() -> &'static [Action] {
        &ALL
    }

    /// Friendly label for the settings UI (the Controls rows).
    fn display(&self) -> &'static str {
        match self {
            Action::Confirm => "Confirm",
            Action::Cancel => "Cancel",
            Action::Osk => "Keyboard",
            Action::Reload => "Reload",
            Action::Prev => "Back / prev",
            Action::Next => "Forward / next",
            Action::Hints => "Link hints",
            Action::Bookmark => "Bookmark",
            Action::Home => "Home",
            Action::Reader => "Reader mode",
            Action::Menu => "Menu",
            Action::Settings => "Settings",
            Action::Quit => "Quit",
            Action::TabNext => "Next tab",
            Action::TabPrev => "Previous tab",
            Action::NewTab => "New tab",
            Action::ZoomIn => "Zoom in",
            Action::ZoomOut => "Zoom out",
            Action::ZoomReset => "Zoom reset",
            Action::NavUp => "Nav up",
            Action::NavDown => "Nav down",
            Action::NavLeft => "Nav left",
            Action::NavRight => "Nav right",
            Action::Scroll => "Scroll toggle",
        }
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
            // Routed through the contextual OSK-space intent: space while the
            // keyboard is open, reload otherwise — same behavior the hardcoded
            // Y button had.
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

/// The Controls screen's sections, in display order (actions sort by name
/// within each). Every [`ALL`] entry belongs to exactly one — a test checks it.
pub const GROUPS: Groups<Action> = &[
    (
        "General",
        &[
            Action::Confirm,
            Action::Cancel,
            Action::Menu,
            Action::Settings,
            Action::Osk,
            Action::Quit,
        ],
    ),
    (
        "Navigation",
        &[
            Action::Prev,
            Action::Next,
            Action::Home,
            Action::Hints,
            Action::Scroll,
            Action::NavUp,
            Action::NavDown,
            Action::NavLeft,
            Action::NavRight,
        ],
    ),
    (
        "Page",
        &[
            Action::Reload,
            Action::Reader,
            Action::Bookmark,
            Action::ZoomIn,
            Action::ZoomOut,
            Action::ZoomReset,
        ],
    ),
    ("Tabs", &[Action::TabNext, Action::TabPrev, Action::NewTab]),
];

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

/// Load `bindings.toml`, writing the defaults as a template on first run.
pub fn load_store() -> Store {
    Store::load(bindings_path(), default_store)
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
