//! Grid-with-a-field navigation for the start page: a tile grid with a text
//! field above or below it, or with none.

/// Which side of the grid the field sits on.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FieldAt {
    Above,
    Below,
}

/// The focused item: the field or a grid tile by index.
#[derive(Clone, Copy, PartialEq, Eq)]
enum GridItem {
    Field,
    Tile(usize),
}

pub struct GridNav {
    item: GridItem,
    /// The field's side, or `None` with no field to focus.
    field: Option<FieldAt>,
    /// The tile a field below was entered from, for Up to return to; `None`
    /// returns to the bottom row's first.
    left: Option<usize>,
    /// Columns, recorded by the renderer each frame (it depends on the
    /// available width) so navigation steps by row.
    cols: usize,
}

impl GridNav {
    /// Starts on the field, or the first tile without one.
    pub fn new(field: Option<FieldAt>) -> Self {
        let mut nav = Self {
            item: GridItem::Field,
            field,
            left: None,
            cols: 1,
        };
        nav.focus_first();
        nav
    }

    /// Show or drop the field; a focus on a dropped field moves to the grid.
    pub fn set_field(&mut self, field: Option<FieldAt>) {
        self.field = field;
        if field.is_none() && self.item == GridItem::Field {
            self.item = GridItem::Tile(0);
        }
    }

    /// Focus the field, or the first tile without one.
    pub fn focus_first(&mut self) {
        self.left = None;
        self.item = match self.field {
            Some(_) => GridItem::Field,
            None => GridItem::Tile(0),
        };
    }

    pub fn field_focused(&self) -> bool {
        self.item == GridItem::Field
    }

    pub fn focus_field(&mut self) {
        self.item = GridItem::Field;
    }

    /// Focus the tile at `index`.
    pub fn select_tile(&mut self, index: usize) {
        self.item = GridItem::Tile(index);
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
    /// the field and the `count`-tile grid.
    pub fn move_sel(&mut self, dx: i32, dy: i32, count: usize) {
        match self.item {
            // From the field only the step into the grid goes anywhere.
            GridItem::Field => match self.field {
                Some(FieldAt::Above) if count > 0 && dy > 0 => self.item = GridItem::Tile(0),
                Some(FieldAt::Below) if count > 0 && dy < 0 => {
                    let bottom_row = (count - 1) / self.cols * self.cols;
                    let back = self.left.unwrap_or(bottom_row);
                    self.item = GridItem::Tile(back.min(count - 1));
                }
                _ => {}
            },
            GridItem::Tile(i) => {
                if dy < 0 {
                    match i.checked_sub(self.cols) {
                        Some(up) => self.item = GridItem::Tile(up),
                        // Up from the top row crosses into a field above.
                        None if self.field == Some(FieldAt::Above) => self.item = GridItem::Field,
                        None => {}
                    }
                } else if dy > 0 {
                    let n = i + self.cols;
                    if n < count {
                        self.item = GridItem::Tile(n);
                    } else if self.field == Some(FieldAt::Below)
                        && i / self.cols == (count - 1) / self.cols
                    {
                        // Down from the bottom row crosses into a field below.
                        self.left = Some(i);
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

    /// Keep a tile selection valid if the grid shrank. An emptied grid falls
    /// back to the field, or keeps the first slot for when one comes back.
    pub fn clamp(&mut self, count: usize) {
        if let GridItem::Tile(i) = self.item {
            self.item = match (count, self.field) {
                (0, Some(_)) => GridItem::Field,
                (0, None) => GridItem::Tile(0),
                _ => GridItem::Tile(i.min(count - 1)),
            };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two rows of three, the second partial: tiles 0..5.
    const COUNT: usize = 5;
    const COLS: usize = 3;

    fn nav(field: Option<FieldAt>) -> GridNav {
        let mut nav = GridNav::new(field);
        nav.set_cols(COLS);
        nav
    }

    #[test]
    fn a_field_above_is_left_up_from_the_top_row() {
        let mut nav = nav(Some(FieldAt::Above));
        nav.move_sel(0, 1, COUNT);
        assert_eq!(nav.tile(), Some(0));
        nav.move_sel(0, -1, COUNT);
        assert!(nav.field_focused());
        nav.move_sel(0, -1, COUNT);
        assert!(nav.field_focused());
    }

    #[test]
    fn a_field_below_returns_up_to_the_tile_it_was_entered_from() {
        let mut nav = nav(Some(FieldAt::Below));
        nav.move_sel(0, 1, COUNT);
        assert!(nav.field_focused(), "Down does nothing from a field below");
        nav.move_sel(0, -1, COUNT);
        assert_eq!(nav.tile(), Some(3), "first entry lands on the bottom row");
        nav.move_sel(1, 0, COUNT);
        assert_eq!(nav.tile(), Some(4));
        nav.move_sel(0, 1, COUNT);
        assert!(nav.field_focused());
        nav.move_sel(0, -1, COUNT);
        assert_eq!(nav.tile(), Some(4));
    }

    #[test]
    fn only_the_bottom_row_crosses_into_a_field_below() {
        let mut nav = nav(Some(FieldAt::Below));
        nav.select_tile(2);
        nav.move_sel(0, 1, COUNT);
        assert_eq!(nav.tile(), Some(2), "no tile under it, but a row is");
    }

    #[test]
    fn up_from_a_field_below_clamps_to_a_shrunk_grid() {
        let mut nav = nav(Some(FieldAt::Below));
        nav.select_tile(4);
        nav.move_sel(0, 1, COUNT);
        nav.move_sel(0, -1, 2);
        assert_eq!(nav.tile(), Some(1));
    }
}
