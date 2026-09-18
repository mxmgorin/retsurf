//! Grid-with-a-field navigation shared by the start page and the speed-dial
//! editor: a tile grid with a text field on one side of it.

/// The focused item: the companion field or a grid tile by index.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum GridItem {
    Field,
    Tile(usize),
}

/// Which side of the grid the field sits on — it decides which vertical step
/// crosses between them.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum FieldSide {
    Above,
    Below,
}

pub struct GridNav {
    item: GridItem,
    field: FieldSide,
    /// Columns, recorded by the renderer each frame (it depends on the
    /// available width) so navigation steps by row.
    cols: usize,
}

impl GridNav {
    /// Starts on the field, which both screens focus on entry.
    pub fn new(field: FieldSide) -> Self {
        Self {
            item: GridItem::Field,
            field,
            cols: 1,
        }
    }

    pub fn item(&self) -> GridItem {
        self.item
    }

    pub fn field_focused(&self) -> bool {
        self.item == GridItem::Field
    }

    pub fn focus_field(&mut self) {
        self.item = GridItem::Field;
    }

    /// Focus a grid tile, so the selection can follow a pin that moved.
    pub fn select_tile(&mut self, slot: usize) {
        self.item = GridItem::Tile(slot);
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
    /// the `count`-tile grid and its field.
    pub fn move_sel(&mut self, dx: i32, dy: i32, count: usize) {
        match self.item {
            // From the field, the step toward the grid enters its nearest row;
            // the other directions have nowhere to go.
            GridItem::Field => {
                if count == 0 {
                    return;
                }
                match self.field {
                    FieldSide::Above if dy > 0 => self.item = GridItem::Tile(0),
                    FieldSide::Below if dy < 0 => self.item = GridItem::Tile(count - 1),
                    _ => {}
                }
            }
            GridItem::Tile(i) => {
                if dy < 0 {
                    if i >= self.cols {
                        self.item = GridItem::Tile(i - self.cols);
                    } else if self.field == FieldSide::Above {
                        // Up from the top row crosses into the field.
                        self.item = GridItem::Field;
                    }
                } else if dy > 0 {
                    let n = i + self.cols;
                    if n < count {
                        self.item = GridItem::Tile(n);
                    } else if self.field == FieldSide::Below {
                        // Down past the last row crosses into the field.
                        self.item = GridItem::Field;
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
