//! Grid-with-a-field navigation for the start page: a tile grid with a text
//! field above it, or with none.

/// The focused item: the field or a grid tile by index.
#[derive(Clone, Copy, PartialEq, Eq)]
enum GridItem {
    Field,
    Tile(usize),
}

pub struct GridNav {
    item: GridItem,
    /// Whether the field is there to be focused.
    field: bool,
    /// Columns, recorded by the renderer each frame (it depends on the
    /// available width) so navigation steps by row.
    cols: usize,
}

impl GridNav {
    /// Starts on the field, or the first tile without one.
    pub fn new(field: bool) -> Self {
        let mut nav = Self {
            item: GridItem::Field,
            field,
            cols: 1,
        };
        nav.focus_first();
        nav
    }

    /// Show or drop the field; a focus on a dropped field moves to the grid.
    pub fn set_field(&mut self, field: bool) {
        self.field = field;
        if !field && self.item == GridItem::Field {
            self.item = GridItem::Tile(0);
        }
    }

    /// Focus the field, or the first tile without one.
    pub fn focus_first(&mut self) {
        self.item = match self.field {
            true => GridItem::Field,
            false => GridItem::Tile(0),
        };
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
                    match i.checked_sub(self.cols) {
                        Some(up) => self.item = GridItem::Tile(up),
                        // Up from the top row crosses into the field.
                        None if self.field => self.item = GridItem::Field,
                        None => {}
                    }
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

    /// Keep a tile selection valid if the grid shrank. An emptied grid falls
    /// back to the field, or keeps the first slot for when one comes back.
    pub fn clamp(&mut self, count: usize) {
        if let GridItem::Tile(i) = self.item {
            self.item = match (count, self.field) {
                (0, true) => GridItem::Field,
                (0, false) => GridItem::Tile(0),
                _ => GridItem::Tile(i.min(count - 1)),
            };
        }
    }
}
