//! The command vocabulary: everything the UI, gamepad, keyboard, and mouse can
//! ask the app to do. Producers (toolbar clicks, menu rows, gamepad buttons)
//! push these into the per-frame queue; [`super::App`] executes them, sending
//! contextual [`InputCommand`]s through the central router (see
//! [`super::router`]).

use crate::browser::BrowserCommand;
use crate::menu::Section;
use crate::osk::OskCommand;

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
    /// Switch the active section by a delta (gamepad/keyboard ◀▶).
    SwitchSection(i32),
    /// Jump to a specific section (clicking its tab).
    SetSection(Section),
    /// Move the active section's selection by `dy` rows (gamepad/keyboard ▲▼).
    Move(i32),
    /// Open the highlighted entry and close the menu (A / Enter).
    OpenSelected,
    /// Remove the highlighted entry (X / Delete).
    RemoveSelected,
    /// Clear all entries in the active section (History's "Clear all").
    Clear,
    /// Load a specific URL and close the menu (clicking a list row).
    OpenUrl(String),
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
    Primary(bool),
    /// Cancel (B): close the on-screen keyboard if open, else go back.
    Cancel,
    /// Keyboard (X): toggle the on-screen keyboard, or backspace while it's open.
    Keyboard,
    /// Shoulder (L1/R1) by direction (-1 left, +1 right): switch the menu's section
    /// while it's open, otherwise navigate the page back / forward.
    Shoulder(i32),
    /// Trigger (L2 = left, R2 = right) with its press state. Drives the on-screen
    /// keyboard (L2 Shift, R2 Enter) when it's open, otherwise cycles tabs.
    Trigger { right: bool, pressed: bool },
    /// A dedicated keyboard key (Y). Applied only while the keyboard is open.
    Osk(OskCommand),
    /// Per-frame analog state: aim vector (left stick + D-pad) and scroll (right
    /// stick Y), each normalized to -1..=1. Drives the cursor, keyboard grid
    /// navigation, or page scroll depending on context.
    Analog { aim: (f32, f32), scroll: f32 },
}
