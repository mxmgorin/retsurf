//! The speed-dial editor: a standalone full-screen overlay (reached from the
//! start page's "Edit" tile) for managing the pinned shortcuts ([`crate::data::dial`]).
//! It shows the pins as a deletable tile grid with a URL field + "Add" button
//! beneath. State here is just the focused item and the field's edit buffer; the
//! dial itself lives in the menu's store, the central router drives the actions,
//! and [`crate::ui::dial_edit`] renders it.

/// The focused item in the editor: a pin tile (deletable), the URL field, or
/// the Add button below it.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum EditItem {
    Tile(usize),
    Field,
    Add,
}

pub struct DialEdit {
    visible: bool,
    item: EditItem,
    /// The URL edit buffer — typed via the OSK, or directly through the egui
    /// text field on a desktop keyboard.
    input: String,
    /// Columns in the tile grid, recorded by the renderer each frame so grid
    /// navigation steps by row (mirrors [`crate::overlay::home::Home`]).
    cols: usize,
}

impl DialEdit {
    pub fn new() -> Self {
        Self {
            visible: false,
            item: EditItem::Field,
            input: String::new(),
            cols: 1,
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
        self.item = EditItem::Field;
        self.input.clear();
    }

    pub fn close(&mut self) {
        self.visible = false;
    }

    #[inline]
    pub fn item(&self) -> EditItem {
        self.item
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

    /// Whether the URL field is the focused item.
    pub fn field_focused(&self) -> bool {
        self.item == EditItem::Field
    }

    /// Focus the URL field (e.g. when the OSK opens to type).
    pub fn focus_field(&mut self) {
        self.item = EditItem::Field;
    }

    /// The focused tile's index, if a tile (not the field / Add) is focused.
    pub fn tile(&self) -> Option<usize> {
        match self.item {
            EditItem::Tile(i) => Some(i),
            _ => None,
        }
    }

    /// Record the grid column count from the renderer.
    pub fn set_cols(&mut self, cols: usize) {
        self.cols = cols.max(1);
    }

    /// Move the selection by a dominant-axis step across the `count`-tile grid
    /// (on top), the URL field, and the Add button (stacked below it).
    pub fn move_sel(&mut self, dx: i32, dy: i32, count: usize) {
        let cols = self.cols.max(1);
        match self.item {
            EditItem::Tile(i) => {
                if dy < 0 {
                    // Up: previous row; the top row has nothing above it.
                    if i >= cols {
                        self.item = EditItem::Tile(i - cols);
                    }
                } else if dy > 0 {
                    // Down: next row, or drop below the grid to the field.
                    let n = i + cols;
                    self.item = if n < count {
                        EditItem::Tile(n)
                    } else {
                        EditItem::Field
                    };
                } else if dx < 0 && i > 0 {
                    self.item = EditItem::Tile(i - 1);
                } else if dx > 0 && i + 1 < count {
                    self.item = EditItem::Tile(i + 1);
                }
            }
            EditItem::Field => {
                if dy < 0 {
                    // Up into the grid: the last tile (none → stay).
                    if count > 0 {
                        self.item = EditItem::Tile(count - 1);
                    }
                } else if dy > 0 {
                    self.item = EditItem::Add;
                }
            }
            EditItem::Add => {
                if dy < 0 {
                    self.item = EditItem::Field;
                }
            }
        }
    }

    /// Keep a tile selection valid if the pin count shrank (after a delete).
    pub fn clamp(&mut self, count: usize) {
        if let EditItem::Tile(i) = self.item {
            self.item = if count == 0 {
                EditItem::Field
            } else {
                EditItem::Tile(i.min(count - 1))
            };
        }
    }
}
