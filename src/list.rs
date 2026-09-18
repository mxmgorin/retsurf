//! Shared list-highlight arithmetic: the one spelling of a clamped selection
//! step, and the stores' [`ListCursor`] built on it.

/// Step `selected` by `dy` within a `len`-row list, clamped to its ends
/// (0 when the list is empty).
pub fn step(selected: usize, dy: i32, len: usize) -> usize {
    (selected as i32 + dy).clamp(0, (len as i32 - 1).max(0)) as usize
}

/// Highlighted row in a menu list. `reserved` counts the leading non-entry rows
/// (e.g. history's "Clear all"), so one arithmetic serves plain and offset lists;
/// the entry count is passed per call — the owning store holds the data.
pub struct ListCursor {
    selected: usize,
    reserved: usize,
}

impl ListCursor {
    /// A cursor over a list with `reserved` leading non-entry rows (0 for a plain
    /// list).
    pub fn new(reserved: usize) -> Self {
        Self {
            selected: 0,
            reserved,
        }
    }

    pub fn selected(&self) -> usize {
        self.selected
    }

    /// Reset the highlight to the first entry (past the reserved rows), or 0 when
    /// the list is empty.
    pub fn reset(&mut self, len: usize) {
        self.selected = if len == 0 { 0 } else { self.reserved };
    }

    /// Move the highlight by `dy` rows, clamped across the reserved rows plus the
    /// `len` entries. No-op on an empty list.
    pub fn move_sel(&mut self, dy: i32, len: usize) {
        if len == 0 {
            return;
        }
        self.selected = step(self.selected, dy, len + self.reserved);
    }

    /// Clamp the highlight after the list shrank to `len` entries.
    pub fn clamp(&mut self, len: usize) {
        self.selected = self.selected.min((len + self.reserved).saturating_sub(1));
    }

    /// Index into the entry list for the highlighted row, or `None` when a
    /// reserved row (e.g. "Clear all") is highlighted.
    pub fn entry_index(&self) -> Option<usize> {
        self.selected.checked_sub(self.reserved)
    }

    /// Whether a reserved leading row is highlighted. Only meaningful on a
    /// non-empty list (the rows aren't shown otherwise).
    pub fn on_reserved_row(&self, len: usize) -> bool {
        self.reserved != 0 && len != 0 && self.selected < self.reserved
    }
}
