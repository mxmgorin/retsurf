//! Grid-with-a-field navigation for the start page: a text field above a tile
//! grid.

/// The focused item: the field or a grid tile by index.
#[derive(Clone, Copy, PartialEq, Eq)]
enum GridItem {
    Field,
    Tile(usize),
}

pub struct GridNav {
    item: GridItem,
    /// Columns, recorded by the renderer each frame (it depends on the
    /// available width) so navigation steps by row.
    cols: usize,
}

impl GridNav {
    /// Starts on the field.
    pub fn new() -> Self {
        Self {
            item: GridItem::Field,
            cols: 1,
        }
    }

    pub fn field_focused(&self) -> bool {
        self.item == GridItem::Field
    }

    pub fn focus_field(&mut self) {
        self.item = GridItem::Field;
    }

    /// The focused tile's index, if a tile (not the field) is focused.
    pub fn tile(&self) -> Option<usize> {
        match self.item {
            GridItem::Tile(i) => Some(i),
            GridItem::Field => None,
        }
    }

    /// Record the grid column count from the renderer.
    pub fn set_cols(&mut self, cols: usize) {
        self.cols = cols.max(1);
    }

    /// Move the selection by a dominant-axis step (`dx`/`dy` in -1..=1) across
    /// the field and the `count`-tile grid below it.
    pub fn move_sel(&mut self, dx: i32, dy: i32, count: usize) {
        match self.item {
            // From the field only Down goes anywhere: into the top row.
            GridItem::Field => {
                if count > 0 && dy > 0 {
                    self.item = GridItem::Tile(0);
                }
            }
            GridItem::Tile(i) => {
                if dy < 0 {
                    self.item = match i.checked_sub(self.cols) {
                        Some(up) => GridItem::Tile(up),
                        // Up from the top row crosses into the field.
                        None => GridItem::Field,
                    };
                } else if dy > 0 {
                    let n = i + self.cols;
                    if n < count {
                        self.item = GridItem::Tile(n);
                    }
                } else if dx < 0 && i > 0 {
                    self.item = GridItem::Tile(i - 1);
                } else if dx > 0 && i + 1 < count {
                    self.item = GridItem::Tile(i + 1);
                }
            }
        }
    }

    /// Keep a tile selection valid if the grid shrank (falls back to the field
    /// when it emptied).
    pub fn clamp(&mut self, count: usize) {
        if let GridItem::Tile(i) = self.item {
            self.item = if count == 0 {
                GridItem::Field
            } else {
                GridItem::Tile(i.min(count - 1))
            };
        }
    }
}
