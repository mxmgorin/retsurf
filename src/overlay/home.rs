//! The built-in start page, rendered as an egui overlay (not a web page) so it
//! navigates with the gamepad exactly like the other overlays. It's shown over
//! the active tab whenever that tab is on `retsurf:home` — a blank page; see
//! [`crate::browser::home`]. State here is which item holds focus (via the
//! shared [`GridNav`], field above the grid) and the search field's edit
//! buffer; the central router moves the selection and activates, and
//! [`crate::ui::home`] renders it (reading the tile list from the menu's live
//! speed dial).

use super::grid::{FieldSide, GridNav};

pub struct Home {
    nav: GridNav,
    /// The search field's edit buffer — typed via the OSK, or directly with a
    /// desktop keyboard through the egui text field.
    input: String,
}

impl Home {
    pub fn new() -> Self {
        Self {
            nav: GridNav::new(FieldSide::Above),
            input: String::new(),
        }
    }

    /// Reset to the entry state (search field focused, empty buffer) — called
    /// when the start page becomes active.
    pub fn reset(&mut self) {
        self.nav.focus_field();
        self.input.clear();
    }

    pub fn input(&self) -> &str {
        &self.input
    }

    pub fn input_mut(&mut self) -> &mut String {
        &mut self.input
    }

    pub fn search_focused(&self) -> bool {
        self.nav.field_focused()
    }

    /// The focused tile's index, if a tile (not the search field) is focused.
    pub fn tile(&self) -> Option<usize> {
        self.nav.tile()
    }

    /// Focus the search field (e.g. when the OSK opens to type).
    pub fn focus_search(&mut self) {
        self.nav.focus_field();
    }

    /// Record the grid column count from the renderer.
    pub fn set_cols(&mut self, cols: usize) {
        self.nav.set_cols(cols);
    }

    /// Move the selection by a dominant-axis step (`dx`/`dy` in -1..=1) across
    /// the search field (on top) and the `count`-tile grid below it.
    pub fn move_sel(&mut self, dx: i32, dy: i32, count: usize) {
        self.nav.move_sel(dx, dy, count);
    }

    /// Keep the selection valid if the bookmark count shrank (or hit zero).
    pub fn clamp(&mut self, count: usize) {
        self.nav.clamp(count);
    }
}
