//! The command vocabulary: everything the UI, gamepad, keyboard, and mouse can
//! ask the app to do. Producers (toolbar clicks, menu rows, gamepad buttons)
//! push these into the per-frame queue; [`super::App`] executes them, sending
//! contextual [`InputCommand`]s through the central router (see
//! [`super::router`]).

use crate::browser::BrowserCommand;
use crate::overlay::menu::Section;
use crate::overlay::osk::OskCommand;
use crate::overlay::settings::SettingsSection;

#[derive(Clone)]
pub enum AppCommand {
    Shutdown,
    /// The window was resized. Carries no size on purpose: the executor re-reads
    /// the live drawable size from the window itself, which is already adjusted
    /// for the toolbar and DPI (the event's logical size is neither).
    Resize,
    Browser(BrowserCommand),
    Input(InputCommand),
    Menu(MenuAction),
    /// Add the current page to bookmarks, or remove it if already saved (★ / Start).
    ToggleBookmark,
    /// An action on the modal page-prompt overlay (select pickers and JS
    /// dialogs — see [`crate::overlay::prompt`]).
    Prompt(PromptAction),
    /// An action on the settings overlay (see [`crate::overlay::settings`]).
    Settings(SettingsAction),
}

/// Actions on the settings overlay. The mouse pushes `Select` then `Activate` /
/// `Adjust`; the gamepad and keyboard push `Activate` / `Adjust` (from the
/// router's contextual mapping) against the already-focused row.
#[derive(Clone)]
pub enum SettingsAction {
    /// Open the overlay (the ⚙ toolbar button), seeding the draft from the live
    /// config.
    Open,
    /// Save the draft to disk, re-apply what can change live, and close (B / ✖).
    Close,
    /// Jump to a section (clicking its tab in the bar). Relative section
    /// switching (L1/R1) comes through [`InputCommand::Shoulder`] instead.
    SetSection(SettingsSection),
    /// Focus row `index` (clicking it).
    Select(usize),
    /// Act on the focused row: toggle a bool, cycle a choice, step a number, or
    /// open the on-screen keyboard on a text row (A / Enter / clicking a row).
    Activate,
    /// Step the focused field by a direction (◀ = -1, ▶ = +1).
    Adjust(i32),
    /// Follow a link on the read-only About tab: save & close the overlay, then
    /// navigate the focused tab to `url` (clicking a link row).
    OpenLink(String),
}

/// Actions on the modal page-prompt overlay. The gamepad and keyboard push
/// `Activate` / `Cancel` through the router; mouse clicks push `ClickSlot`
/// with the row or button they hit.
#[derive(Clone)]
pub enum PromptAction {
    /// Activate the focused slot (A / Enter): choose or toggle an option, or
    /// press the focused dialog button.
    Activate,
    /// Dismiss the front control with its default response (B / Esc).
    Cancel,
    /// Focus and activate slot `index` (clicking it).
    ClickSlot(usize),
}

/// Actions on the full-screen menu (Tabs / Bookmarks / History / Downloads). The
/// mouse pushes the absolute variants (`SetSection`, `OpenUrl`, `RemoveAt`); the
/// gamepad and keyboard push the relative ones, routed from [`InputCommand`] by
/// the central router.
#[derive(Clone)]
pub enum MenuAction {
    /// Toggle the menu open/closed (Select / ☰).
    Open,
    /// Close the menu (B / Close button / Esc).
    Close,
    /// Jump to a specific section (clicking its tab). Relative section/row
    /// movement comes through [`InputCommand::Nav`] instead.
    SetSection(Section),
    /// Open the highlighted entry and close the menu (A / Enter).
    OpenSelected,
    /// Remove the highlighted entry (X / Delete).
    RemoveSelected,
    /// Clear all entries in the active section (History's "Clear all").
    Clear,
    /// Load a specific URL and close the menu (clicking a list row).
    OpenUrl(String),
    /// Toggle `url` in the saved bookmarks, keeping the menu open (clicking a
    /// row's ★/☆ button in Tabs / History).
    ToggleBookmark(String),
    /// Open the speed-dial editor overlay (the start page's "Edit" tile).
    DialEdit,
    /// Close the speed-dial editor (its ✖ button; the gamepad closes with B).
    DialClose,
    /// Pin `url` to the speed dial from the editor's field (normalized to a URL),
    /// clearing the field; keeps the editor open.
    DialAdd(String),
    /// Remove the pin at `index` in the speed-dial editor (clicking its ✖).
    DialRemoveAt(usize),
    /// Toggle the ⚙ settings shortcut on/off the dial (the editor's "Pin
    /// settings" button); keeps the editor open.
    DialToggleSettings,
    /// Remove the entry at `index` in the active section (clicking its ✖).
    RemoveAt(usize),
    /// Switch to the tab at `index` and close the menu (clicking a tab row).
    OpenTab(usize),
    /// Close the tab at `index` (clicking a tab's ✖).
    CloseTab(usize),
    /// Open a new tab and close the menu (clicking "+ New tab").
    NewTab,
}

/// A *contextual* input intent from a control device — one whose effect depends
/// on what's on screen. The gamepad only translates physical buttons/sticks into
/// these (unambiguous navigation goes straight to [`BrowserCommand`]); the central
/// router decides what each does given the current state (keyboard open? cursor
/// over the page or the toolbar?).
#[derive(Clone)]
pub enum InputCommand {
    /// Primary action (A): activate the keyboard key, or click the page/toolbar.
    /// Carries the press state so page clicks get matching down/up events.
    Confirm(bool),
    /// Cancel (B): close the on-screen keyboard if open, else go back.
    Cancel,
    /// Keyboard (X): toggle the on-screen keyboard, or backspace while it's open.
    ToggleOsk,
    /// Shoulder (L1/R1) by direction (-1 left, +1 right): switch the menu's section
    /// while it's open, otherwise navigate the page back / forward.
    Shoulder(i32),
    /// Trigger (L2 = left, R2 = right) with its press state. Drives the on-screen
    /// keyboard (L2 Shift, R2 Enter) when it's open, otherwise cycles tabs.
    Trigger { right: bool, pressed: bool },
    /// A dedicated keyboard key (Y). Applied only while the keyboard is open.
    Osk(OskCommand),
    /// Link-hint mode (L3): enter it (collecting the page's clickable elements)
    /// or exit if already shown. See [`crate::overlay::hints`].
    Hints,
    /// Switch the active tab by a delta, wrapping (the `tab_next` / `tab_prev`
    /// binding actions).
    CycleTab(i32),
    /// One overlay-navigation step — keyboard arrows (`nav_*` bindings) or the
    /// stick shaped by the router's threshold + auto-repeat. Acts on whichever
    /// overlay is open (menu / OSK / hints); a no-op with none.
    Nav(i32, i32),
    /// A discrete D-pad press edge (dominant direction, -1/0/1 per axis), in
    /// addition to the D-pad's contribution to the aim vector. Only hint mode
    /// consumes it (as a combo symbol); other contexts already navigate off the
    /// merged aim and ignore it.
    DpadPress(i32, i32),
    /// Per-frame analog state. `aim` is the left stick + D-pad merged (drives the
    /// cursor and most overlay navigation); `stick` is the left stick alone, so
    /// hint mode can hop on the stick while the D-pad types combo symbols.
    /// `scroll` is the right stick Y. All normalized to -1..=1. `scroll_mode` is
    /// the gamepad's latched toggle (the `scroll` action): overlays keep using
    /// the raw aim for navigation, but on the bare page the aim scrolls instead
    /// of moving the cursor.
    Analog {
        aim: (f32, f32),
        stick: (f32, f32),
        scroll: f32,
        scroll_mode: bool,
    },
}
