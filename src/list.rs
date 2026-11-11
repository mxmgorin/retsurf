//! Shared list-highlight arithmetic: the one spelling of a clamped selection
//! step and of a wrapping one, and the stores' [`ListCursor`] built on the first.

/// Step `selected` by `dy` within a `len`-row list, clamped to its ends
/// (0 when the list is empty).
pub fn step(selected: usize, dy: i32, len: usize) -> usize {
    (selected as i32 + dy).clamp(0, (len as i32 - 1).max(0)) as usize
}

/// Step `selected` by `dy` within a `len`-row list, past either end to the
/// other (0 when the list is empty).
pub fn wrap(selected: usize, dy: i32, len: usize) -> usize {
    match len {
        0 => 0,
        _ => (selected as i32 + dy).rem_euclid(len as i32) as usize,
    }
}

/// Step `selected` across a `len`-item grid of `cols` columns: `dx` to the
/// next item in reading order, `dy` a whole row; a step off the grid stays put.
pub fn grid_step(selected: usize, dx: i32, dy: i32, cols: usize, len: usize) -> usize {
    let to = selected as i32 + dx + dy * cols.max(1) as i32;
    match (0..len as i32).contains(&to) {
        true => to as usize,
        false => selected,
    }
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

    /// Put the highlight on row `index` of the reserved rows plus the `len`
    /// entries, clamped to the last.
    pub fn select(&mut self, index: usize, len: usize) {
        self.selected = index.min((len + self.reserved).saturating_sub(1));
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Two rows of three, the second partial: items 0..5.
    const LEN: usize = 5;
    const COLS: usize = 3;

    #[test]
    fn grid_step_moves_by_rows_and_crosses_row_ends_sideways() {
        assert_eq!(grid_step(1, 0, 1, COLS, LEN), 4);
        assert_eq!(grid_step(4, 0, -1, COLS, LEN), 1);
        assert_eq!(grid_step(2, 1, 0, COLS, LEN), 3);
        assert_eq!(grid_step(3, -1, 0, COLS, LEN), 2);
    }

    #[test]
    fn grid_step_off_the_grid_stays_put() {
        assert_eq!(grid_step(2, 0, 1, COLS, LEN), 2, "no item under it");
        assert_eq!(grid_step(1, 0, -1, COLS, LEN), 1);
        assert_eq!(grid_step(0, -1, 0, COLS, LEN), 0);
        assert_eq!(grid_step(4, 1, 0, COLS, LEN), 4);
    }
}
