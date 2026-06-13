//! The built-in start page, rendered as an egui overlay (not a web page) so it
//! navigates with the gamepad exactly like the other overlays. It's shown over
//! the active tab whenever that tab is on `retsurf:home` — a blank page; see
//! [`crate::browser::home`]. State here is just which item holds focus (the
//! search field or a speed-dial tile) and the search field's edit buffer; the
//! central router moves the selection and activates, and [`crate::ui::home`]
//! renders it (reading the tile list from the menu's live speed dial).

/// The focused item on the start page.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum HomeItem {
    /// The search / URL field (the hero, focused on entry).
    Search,
    /// A speed-dial tile (pinned shortcut) by index.
    Tile(usize),
}

pub struct Home {
    item: HomeItem,
    /// The search field's edit buffer — typed via the OSK, or directly with a
    /// desktop keyboard through the egui text field.
    input: String,
    /// Columns in the speed-dial grid, recorded by the renderer each frame
    /// (it depends on the available width) so grid navigation steps by row.
    cols: usize,
}

impl Home {
    pub fn new() -> Self {
        Self {
            item: HomeItem::Search,
            input: String::new(),
            cols: 1,
        }
    }

    /// Reset to the entry state (search field focused, empty buffer) — called
    /// when the start page becomes active.
    pub fn reset(&mut self) {
        self.item = HomeItem::Search;
        self.input.clear();
    }

    pub fn input(&self) -> &str {
        &self.input
    }

    pub fn input_mut(&mut self) -> &mut String {
        &mut self.input
    }

    /// Whether the search field is the focused item.
    pub fn search_focused(&self) -> bool {
        self.item == HomeItem::Search
    }

    /// The focused tile's index, if a tile (not the search field) is focused.
    pub fn tile(&self) -> Option<usize> {
        match self.item {
            HomeItem::Tile(i) => Some(i),
            HomeItem::Search => None,
        }
    }

    /// Focus the search field (e.g. when the OSK opens to type).
    pub fn focus_search(&mut self) {
        self.item = HomeItem::Search;
    }

    /// Record the grid column count from the renderer.
    pub fn set_cols(&mut self, cols: usize) {
        self.cols = cols.max(1);
    }

    /// Move the selection by a dominant-axis step (`dx`/`dy` ∈ -1..=1) across the
    /// search field (on top) and the `count`-tile grid below it.
    pub fn move_sel(&mut self, dx: i32, dy: i32, count: usize) {
        let cols = self.cols.max(1);
        match self.item {
            // From the field, Down drops into the first tile; the other
            // directions have nowhere to go.
            HomeItem::Search => {
                if dy > 0 && count > 0 {
                    self.item = HomeItem::Tile(0);
                }
            }
            HomeItem::Tile(i) => {
                if dy < 0 {
                    // Up from the top row returns to the search field.
                    self.item = if i < cols {
                        HomeItem::Search
                    } else {
                        HomeItem::Tile(i - cols)
                    };
                } else if dy > 0 {
                    let n = i + cols;
                    if n < count {
                        self.item = HomeItem::Tile(n);
                    }
                } else if dx < 0 && i > 0 {
                    self.item = HomeItem::Tile(i - 1);
                } else if dx > 0 && i + 1 < count {
                    self.item = HomeItem::Tile(i + 1);
                }
            }
        }
    }

    /// Keep the selection valid if the bookmark count shrank (or hit zero).
    pub fn clamp(&mut self, count: usize) {
        if let HomeItem::Tile(i) = self.item {
            self.item = if count == 0 {
                HomeItem::Search
            } else {
                HomeItem::Tile(i.min(count - 1))
            };
        }
    }
}
