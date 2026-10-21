//! The built-in start page, rendered as an egui overlay (not a web page) so it
//! navigates with the gamepad exactly like the other overlays. It's shown over
//! the active tab whenever that tab is on `retsurf:home` — a blank page; see
//! [`crate::browser::home`]. State here is which item holds focus (via the
//! shared [`GridNav`], field above or below the grid) and the search field's edit
//! buffer; the central router moves the selection and activates, and
//! [`crate::ui::home`] renders it (reading the tile list from the menu's live
//! bookmarks).

use super::grid::{FieldAt, GridNav};

pub struct Home {
    nav: GridNav,
    /// The search field's edit buffer — typed via the OSK, or directly with a
    /// desktop keyboard through the egui text field.
    input: String,
}

impl Home {
    /// A start page focused on its search field on `search`'s side of the
    /// grid, or on its first tile when it shows none.
    pub fn new(search: Option<FieldAt>) -> Self {
        Self {
            nav: GridNav::new(search),
            input: String::new(),
        }
    }

    /// Reset to the entry state (search field focused, empty buffer) — called
    /// when the start page becomes active.
    pub fn reset(&mut self) {
        self.nav.focus_first();
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

    /// Focus the tile at `index`.
    pub fn select_tile(&mut self, index: usize) {
        self.nav.select_tile(index);
    }

    /// The focused tile's index, if a tile (not the search field) is focused.
    pub fn tile(&self) -> Option<usize> {
        self.nav.tile()
    }

    /// Place the search field on a side of the grid, or hide it.
    pub fn set_search(&mut self, search: Option<FieldAt>) {
        self.nav.set_field(search);
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
    /// the search field and the `count`-tile grid.
    pub fn move_sel(&mut self, dx: i32, dy: i32, count: usize) {
        self.nav.move_sel(dx, dy, count);
    }

    /// Keep the selection valid if the bookmark count shrank (or hit zero).
    pub fn clamp(&mut self, count: usize) {
        self.nav.clamp(count);
    }
}
