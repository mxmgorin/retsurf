//! Everything the UI, gamepad, keyboard, and mouse can
//! ask the app to do. Producers (toolbar clicks, menu rows, gamepad buttons)
//! push these into the per-frame queue; [`crate::app`] executes them, sending
//! contextual [`InputCommand`]s through its central router.

use crate::browser::BrowserCommand;
use crate::overlay::menu::Section;
use crate::overlay::osk::{OskCommand, PadInput};
use crate::overlay::settings::SettingsSection;

#[derive(Clone)]
pub enum AppCommand {
    Shutdown,
    /// The window was resized. Carries no size on purpose: the executor re-reads
    /// the live drawable size from the window itself, which is already adjusted
    /// for the toolbar and DPI.
    Resize,
    Browser(BrowserCommand),
    Input(InputCommand),
    Menu(MenuAction),
    ToggleBookmark,
    /// Put the caret in the address bar, with the keyboard up for a pad.
    FocusAddressBar,
    /// Close the focused tab. Closing the last one leaves a fresh tab open.
    CloseTab,
    GameMode,
    /// An action on Game Mode's own menu (see [`crate::overlay::game::menu`]).
    GameMenu(GameMenuAction),
    /// An action on its input-map screens (see [`crate::overlay::game::input_maps`]).
    GameInputMaps(GameInputMapsAction),
    /// An action on its map editor (see [`crate::overlay::game::map_edit`]).
    GameMapEdit(GameMapEditAction),
    /// An action on the modal page-prompt overlay (select pickers and JS
    /// dialogs — see [`crate::overlay::prompt`]).
    Prompt(PromptAction),
    /// An action on the settings overlay (see [`crate::overlay::settings`]).
    Settings(SettingsAction),
}

impl AppCommand {
    /// Whether this still fires while Game Mode is on: the mode's own overlays,
    /// the way out, the loop's upkeep.
    pub fn in_game_mode(&self) -> bool {
        match self {
            AppCommand::Shutdown
            | AppCommand::Resize
            | AppCommand::Input(_)
            | AppCommand::Prompt(_)
            | AppCommand::GameMenu(_)
            | AppCommand::GameInputMaps(_)
            | AppCommand::GameMapEdit(_)
            | AppCommand::GameMode => true,
            AppCommand::Browser(_)
            | AppCommand::Menu(_)
            | AppCommand::ToggleBookmark
            | AppCommand::FocusAddressBar
            | AppCommand::CloseTab
            | AppCommand::Settings(_) => false,
        }
    }
}

/// Actions on Game Mode's menu. `Click` carries its target row; the rest act
/// on the focused one.
#[derive(Clone)]
pub enum GameMenuAction {
    /// Act on the focused row.
    Activate,
    /// Focus row `index` and activate it.
    Click(usize),
}

/// Actions on Game Mode's input-map screens.
#[derive(Clone)]
pub enum GameInputMapsAction {
    Close,
    Activate,
    /// Focus row `index` and take it.
    Click(usize),
    /// A name the on-screen keyboard submitted, for a rename or a copy.
    Name(String),
}

/// Actions on the Game Mode map editor. `Click` carries its target row; the
/// rest act on the focused one.
#[derive(Clone)]
pub enum GameMapEditAction {
    Close,
    Activate,
    /// Focus row `index` and open its list.
    Click(usize),
    /// A gesture captured for the row that adds a source, as the bindings file
    /// would spell it; `keyboard` tells the two devices apart, whose spellings
    /// collide.
    Capture {
        gesture: String,
        keyboard: bool,
    },
    /// Stop listening without a source (the idle give-up).
    CaptureCancel,
    /// Unbind the focused row, which takes its line out of the file.
    Remove,
}

/// Actions on the settings overlay. `Select` carries its target row;
/// `Activate` and `Adjust` act on the focused one.
#[derive(Clone)]
pub enum SettingsAction {
    /// Open the overlay, seeding the draft from the live config.
    Open,
    /// Save the draft to disk, re-apply what can change live, and close.
    Close,
    /// Jump to a section. Relative switching comes through
    /// [`InputCommand::Shoulder`] instead.
    SetSection(SettingsSection),
    /// Focus row `index`.
    Select(usize),
    /// Act on the focused row: toggle a bool, cycle a choice, step a number, or
    /// open the on-screen keyboard on a text row.
    Activate,
    /// Step the focused field by a direction (-1 left, +1 right).
    Adjust(i32),
    /// Follow a link on the read-only About tab: save & close the overlay, then
    /// navigate the focused tab to `url`.
    OpenLink(String),
    /// While capturing a binding (Controls section): the gesture just performed,
    /// to add to the focused action. `keyboard` tells a key combo from a gamepad
    /// gesture, whose strings can collide (e.g. `"a"`).
    CaptureBinding { gesture: String, keyboard: bool },
    /// Cancel binding capture without changing anything.
    CaptureCancel,
    /// About tab: query GitHub for a newer release.
    CheckUpdate,
    /// About tab: download + verify + swap the available release in place.
    InstallUpdate,
    /// About tab: quit so the launcher re-execs the freshly swapped binary.
    QuitForUpdate,
}

/// Actions on the modal page-prompt overlay. `ClickSlot` carries its target;
/// `Activate` and `Cancel` act on the focused slot.
#[derive(Clone)]
pub enum PromptAction {
    /// Activate the focused slot: choose or toggle an option, or
    /// press the focused dialog button.
    Activate,
    /// Dismiss the front control with its default response.
    Cancel,
    /// Focus and activate slot `index`.
    ClickSlot(usize),
}

/// Actions on the full-screen menu (Tabs / Bookmarks / History / Downloads).
/// Absolute variants carry their target; relative ones act on the focus.
#[derive(Clone)]
pub enum MenuAction {
    /// Toggle the menu open/closed.
    Open,
    Close,
    /// Jump to a specific section. Relative movement comes through
    /// [`InputCommand::Nav`] instead.
    SetSection(Section),
    /// Open the highlighted entry and close the menu.
    OpenSelected,
    /// Remove the highlighted entry.
    RemoveSelected,
    /// Clear all entries in the active section.
    Clear,
    OpenUrl(String),
    ToggleBookmark(String),
    /// Open the speed-dial editor overlay.
    DialEdit,
    /// Close the speed-dial editor.
    DialClose,
    /// Pin `url` to the speed dial from the editor's field (normalized to a URL),
    /// clearing the field.
    DialAdd(String),
    DialRemoveAt(usize),
    /// Put the settings shortcut back on the dial.
    DialPinSettings,
    /// Remove the entry at `index` in the active section.
    RemoveAt(usize),
    /// Switch to the tab at `index` and close the menu.
    OpenTab(usize),
    /// Close the tab at `index`.
    CloseTab(usize),
    /// Open a new tab and close the menu.
    NewTab,
}

/// A *contextual* input intent: one whose effect depends on what is on screen.
/// The central router decides what each does.
#[derive(Clone)]
pub enum InputCommand {
    /// Primary action: activate the keyboard key, or click the page/toolbar.
    /// Carries the press state so page clicks get matching down/up events.
    Confirm(bool),
    /// Cancel: close the on-screen keyboard if open, else go back.
    Cancel,
    /// Toggle the on-screen keyboard, or backspace while it's open.
    ToggleOsk,
    /// Shoulder by direction (-1 left, +1 right): switch the menu's section
    /// while it's open, otherwise navigate the page back / forward.
    Shoulder(i32),
    /// Trigger (L2 = left, R2 = right) with its press state.
    Trigger { right: bool, pressed: bool },
    /// Page zoom by ladder step (0 resets): parked while the keyboard owns
    /// the triggers, like other browser shortcuts under a takeover overlay.
    Zoom(i32),
    /// A dedicated keyboard key. Applied only while the keyboard is open.
    Osk(OskCommand),
    /// Link-hint mode: enter it (collecting the page's clickable elements)
    /// or exit if already shown. See [`crate::overlay::hints`].
    Hints,
    /// Switch the active tab by a delta, wrapping (the `tab_next` / `tab_prev`
    /// binding actions).
    CycleTab(i32),
    /// One overlay-navigation step — keyboard arrows (`nav_*` bindings) or the
    /// stick shaped by the router's threshold + auto-repeat. Acts on whichever
    /// overlay is open (menu / OSK / hints); a no-op with none.
    Nav(i32, i32),
    /// A discrete D-pad press edge (-1/0/1 per axis), on top of the D-pad's own
    /// contribution to the aim vector.
    DpadPress(i32, i32),
    /// A typed letter for a keyboard-driven hint round, the counterpart of the
    /// gamepad's combo symbols. Emitted only while such a round is open.
    HintKey(char),
    /// A page click with a button the Confirm intent cannot carry: that one
    /// also activates the chrome, which knows only "the pointer".
    Click {
        button: crate::event::game::input_map::ClickButton,
        pressed: bool,
    },
    /// A pad button the keyboard claimed ahead of its binding.
    OskButton(PadInput),
    /// Per-frame analog state, all normalized to -1..=1. `aim` merges the left
    /// stick and D-pad, `stick` is the stick alone (hint mode hops on it while
    /// the D-pad types combos), `scroll` is the page-scroll vector, and
    /// `scroll_mode` makes the aim scroll the page.
    Analog {
        aim: (f32, f32),
        stick: (f32, f32),
        scroll: (f32, f32),
        /// The right stick's full vector.
        right: (f32, f32),
        scroll_mode: bool,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The reserved Select must not open the browser's menu over a game, and no
    /// shortcut resolved under Game Mode's overlays may navigate out of one.
    #[test]
    fn game_mode_drops_the_browser_vocabulary_and_keeps_its_own() {
        for command in [
            AppCommand::Menu(MenuAction::Open),
            AppCommand::Settings(SettingsAction::Open),
            AppCommand::Browser(BrowserCommand::Back),
            AppCommand::ToggleBookmark,
        ] {
            assert!(!command.in_game_mode());
        }
        // The mode's own controls, and what the loop needs whatever is on screen.
        for command in [
            AppCommand::GameMode,
            AppCommand::GameMenu(GameMenuAction::Activate),
            AppCommand::Input(InputCommand::Cancel),
            AppCommand::Prompt(PromptAction::Cancel),
            AppCommand::Shutdown,
            AppCommand::Resize,
        ] {
            assert!(command.in_game_mode());
        }
    }
}
