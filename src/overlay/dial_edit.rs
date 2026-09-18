//! The speed-dial editor: a standalone full-screen overlay (reached from the
//! start page's "Edit" tile) for managing the pinned shortcuts ([`crate::data::dial`]).
//! It shows the pins as a deletable tile grid with a URL field + "Add" button
//! beneath (the shared [`GridNav`], field below the grid). Tile indices are dial
//! indices; the slot past them is the "Pin settings" tile shown while
//! `SETTINGS_PIN` is off the dial.
//! The dial itself lives in the menu's store, the central router drives the
//! actions, and [`crate::ui::dial_edit`] renders it.

use super::grid::{FieldSide, GridNav};

/// The focused item, under the name this screen's consumers know it by.
pub use super::grid::GridItem as EditItem;

pub struct DialEdit {
    visible: bool,
    nav: GridNav,
    /// The URL edit buffer — typed via the OSK, or directly through the egui
    /// text field on a desktop keyboard.
    input: String,
}

impl DialEdit {
    pub fn new() -> Self {
        Self {
            visible: false,
            nav: GridNav::new(FieldSide::Below),
            input: String::new(),
        }
    }

    #[inline]
    pub fn visible(&self) -> bool {
        self.visible
    }

    /// Open the editor, focused on the (empty) URL field so the user can type
    /// straight away.
    pub fn open(&mut self) {
        self.visible = true;
        self.nav.focus_field();
        self.input.clear();
    }

    pub fn close(&mut self) {
        self.visible = false;
    }

    #[inline]
    pub fn item(&self) -> EditItem {
        self.nav.item()
    }

    pub fn input(&self) -> &str {
        &self.input
    }

    pub fn input_mut(&mut self) -> &mut String {
        &mut self.input
    }

    pub fn clear_input(&mut self) {
        self.input.clear();
    }

    pub fn field_focused(&self) -> bool {
        self.nav.field_focused()
    }

    /// Focus the URL field (e.g. when the OSK opens to type).
    pub fn focus_field(&mut self) {
        self.nav.focus_field();
    }

    /// Focus a grid tile, so the selection can follow a pin that moved.
    pub fn select_tile(&mut self, slot: usize) {
        self.nav.select_tile(slot);
    }

    /// The focused tile's index, if a tile (not the field / Add) is focused.
    pub fn tile(&self) -> Option<usize> {
        self.nav.tile()
    }

    /// Record the grid column count from the renderer.
    pub fn set_cols(&mut self, cols: usize) {
        self.nav.set_cols(cols);
    }

    /// Move the selection by a dominant-axis step across the `count`-tile grid
    /// and the URL field below it.
    pub fn move_sel(&mut self, dx: i32, dy: i32, count: usize) {
        self.nav.move_sel(dx, dy, count);
    }

    /// Keep a tile selection valid if the pin count shrank (after a delete).
    pub fn clamp(&mut self, count: usize) {
        self.nav.clamp(count);
    }
}
