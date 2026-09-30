//! The 3 x 3 image grid (R7): which entry a key moves to, which nine entries are on screen, and
//! which entry Command+1 to Command+9 pick.
//!
//! Entries are laid out in reading order, three to a row. Only three rows show at a time, so the
//! grid scrolls by whole rows and the number badges always name the cell they sit in.

use std::ops::Range;

pub const COLUMNS: usize = 3;
pub const ROWS: usize = 3;
pub const CELLS: usize = COLUMNS * ROWS;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

/// The entry an arrow key moves to. Left and right stay inside the row, up stops at the first
/// row, and down from a column the last row does not reach lands on the last entry.
pub fn step(index: usize, len: usize, direction: Direction) -> usize {
    if len == 0 {
        return 0;
    }
    let index = index.min(len - 1);
    match direction {
        Direction::Left if !index.is_multiple_of(COLUMNS) => index - 1,
        Direction::Right if index % COLUMNS < COLUMNS - 1 && index + 1 < len => index + 1,
        Direction::Up if index >= COLUMNS => index - COLUMNS,
        Direction::Down if index + COLUMNS < len => index + COLUMNS,
        Direction::Down if index / COLUMNS < (len - 1) / COLUMNS => len - 1,
        _ => index,
    }
}

/// The first visible row after `selected` becomes the selection: the view moves only as far as
/// it must, and never past the point where fewer than three rows would be shown.
pub fn first_row_for(selected: usize, first_row: usize, len: usize) -> usize {
    let rows = len.div_ceil(COLUMNS);
    let last_first_row = rows.saturating_sub(ROWS);
    let row = selected / COLUMNS;
    let first_row = if row < first_row {
        row
    } else if row >= first_row + ROWS {
        row + 1 - ROWS
    } else {
        first_row
    };
    first_row.min(last_first_row)
}

/// The entries on screen when the view starts at `first_row`.
pub fn visible(first_row: usize, len: usize) -> Range<usize> {
    let start = (first_row * COLUMNS).min(len);
    start..(start + CELLS).min(len)
}

/// The entry in cell `number` (1 to 9), if the cell holds one.
pub fn entry_for_number(number: usize, first_row: usize, len: usize) -> Option<usize> {
    let cell = number.checked_sub(1).filter(|cell| *cell < CELLS)?;
    let index = first_row * COLUMNS + cell;
    (index < len).then_some(index)
}

/// The view after the wheel or a trackpad scrolled by `rows` rows (positive is down).
pub fn scrolled(first_row: usize, rows: isize, len: usize) -> usize {
    let last_first_row = len.div_ceil(COLUMNS).saturating_sub(ROWS);
    first_row.saturating_add_signed(rows).min(last_first_row)
}

#[cfg(test)]
mod tests {
    use super::*;
    use Direction::*;

    #[test]
    fn arrows_move_in_two_dimensions() {
        // 0 1 2 / 3 4 5 / 6 7 8
        assert_eq!(step(4, 9, Right), 5);
        assert_eq!(step(4, 9, Left), 3);
        assert_eq!(step(4, 9, Up), 1);
        assert_eq!(step(4, 9, Down), 7);
    }

    #[test]
    fn the_edges_hold_still() {
        assert_eq!(step(0, 9, Left), 0);
        assert_eq!(step(2, 9, Right), 2);
        assert_eq!(step(1, 9, Up), 1);
        assert_eq!(step(7, 9, Down), 7);
    }

    #[test]
    fn a_short_last_row_is_reachable_and_does_not_overshoot() {
        // 0 1 2 / 3 4 5 / 6 7
        assert_eq!(step(5, 8, Down), 7);
        assert_eq!(step(4, 8, Down), 7);
        assert_eq!(step(3, 8, Down), 6);
        assert_eq!(step(7, 8, Right), 7);
        assert_eq!(step(6, 8, Right), 7);
        assert_eq!(step(7, 8, Up), 4);
    }

    #[test]
    fn a_single_row_has_nowhere_to_go_down() {
        assert_eq!(step(1, 2, Down), 1);
        assert_eq!(step(0, 1, Right), 0);
        assert_eq!(step(0, 0, Down), 0);
    }

    #[test]
    fn the_view_follows_the_selection_by_whole_rows() {
        // 30 entries make ten rows.
        assert_eq!(first_row_for(4, 0, 30), 0);
        assert_eq!(first_row_for(9, 0, 30), 1);
        assert_eq!(first_row_for(29, 1, 30), 7);
        assert_eq!(first_row_for(3, 5, 30), 1);
        assert_eq!(first_row_for(18, 5, 30), 5);
    }

    #[test]
    fn the_view_never_shows_less_than_three_rows() {
        assert_eq!(first_row_for(0, 4, 9), 0);
        assert_eq!(first_row_for(28, 0, 29), 7);
    }

    #[test]
    fn nine_cells_are_visible_at_most() {
        assert_eq!(visible(0, 30), 0..9);
        assert_eq!(visible(2, 30), 6..15);
        assert_eq!(visible(0, 4), 0..4);
        assert_eq!(visible(3, 4), 4..4);
    }

    #[test]
    fn numbers_name_the_visible_cells() {
        assert_eq!(entry_for_number(1, 0, 30), Some(0));
        assert_eq!(entry_for_number(9, 0, 30), Some(8));
        assert_eq!(entry_for_number(1, 2, 30), Some(6));
        assert_eq!(entry_for_number(9, 2, 30), Some(14));
        assert_eq!(entry_for_number(5, 0, 4), None);
        assert_eq!(entry_for_number(0, 0, 30), None);
        assert_eq!(entry_for_number(10, 0, 30), None);
    }

    #[test]
    fn scrolling_stays_between_the_first_and_the_last_page() {
        assert_eq!(scrolled(0, -1, 30), 0);
        assert_eq!(scrolled(0, 2, 30), 2);
        assert_eq!(scrolled(6, 5, 30), 7);
        assert_eq!(scrolled(0, 3, 9), 0);
    }
}
