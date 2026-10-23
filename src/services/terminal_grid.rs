//! Terminal pane grid: its shape, track sizes, and which tab fills each cell.

use serde::{Deserialize, Serialize};

/// Most rows or columns a grid may have.
const MAX_TRACKS: u8 = 3;
/// Most panes shown at once; a 2×2 grid is the largest square that fits.
pub(crate) const MAX_CELLS: usize = 4;
/// Track sizes are stored in thousandths so layouts compare and serialize exactly.
const WHOLE: u16 = 1000;
/// Narrowest track a saved layout may contain.
const MIN_TRACK: u16 = 20;

/// Every layout the picker offers, as `(columns, rows)`.
pub(crate) const SHAPES: [(u8, u8); 6] = [(1, 1), (2, 1), (1, 2), (3, 1), (1, 3), (2, 2)];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Direction {
    Left,
    Right,
    Up,
    Down,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Axis {
    Column,
    Row,
}

/// Cells are numbered row by row, so a 2×2 grid is `[top-left, top-right,
/// bottom-left, bottom-right]`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct TerminalGrid {
    cols: u8,
    rows: u8,
    #[serde(default)]
    col_sizes: Vec<u16>,
    #[serde(default)]
    row_sizes: Vec<u16>,
}

impl Default for TerminalGrid {
    fn default() -> Self {
        Self::new(1, 1)
    }
}

impl TerminalGrid {
    pub(crate) fn new(cols: u8, rows: u8) -> Self {
        let (cols, rows) = bound_shape(cols, rows);
        Self {
            cols,
            rows,
            col_sizes: equal_sizes(cols),
            row_sizes: equal_sizes(rows),
        }
    }

    pub(crate) fn shape(&self) -> (u8, u8) {
        (self.cols, self.rows)
    }

    pub(crate) fn cell_count(&self) -> usize {
        usize::from(self.cols) * usize::from(self.rows)
    }

    pub(crate) fn is_single(&self) -> bool {
        self.cell_count() == 1
    }

    /// Fractions of the grid's width taken by each column, left to right.
    pub(crate) fn column_fractions(&self) -> Vec<f32> {
        fractions(&self.col_sizes)
    }

    /// Fractions of the grid's height taken by each row, top to bottom.
    pub(crate) fn row_fractions(&self) -> Vec<f32> {
        fractions(&self.row_sizes)
    }

    /// Change the shape, keeping the sizes of an axis whose track count is
    /// unchanged. Returns whether anything changed.
    pub(crate) fn set_shape(&mut self, cols: u8, rows: u8) -> bool {
        let (cols, rows) = bound_shape(cols, rows);
        if (cols, rows) == self.shape() {
            return false;
        }
        if cols != self.cols {
            self.col_sizes = equal_sizes(cols);
        }
        if rows != self.rows {
            self.row_sizes = equal_sizes(rows);
        }
        self.cols = cols;
        self.rows = rows;
        true
    }

    /// Repair a layout read from disk; invalid sizes fall back to equal tracks.
    pub(crate) fn normalize(&mut self) {
        let (cols, rows) = bound_shape(self.cols, self.rows);
        self.cols = cols;
        self.rows = rows;
        repair_sizes(&mut self.col_sizes, cols);
        repair_sizes(&mut self.row_sizes, rows);
    }

    /// Place the divider after track `boundary` at `position`, a fraction of
    /// the grid's extent. Only the two tracks beside it change, and neither
    /// may shrink below `min_fraction`. Returns whether a size changed.
    pub(crate) fn move_divider(
        &mut self,
        axis: Axis,
        boundary: usize,
        position: f32,
        min_fraction: f32,
    ) -> bool {
        let sizes = match axis {
            Axis::Column => &mut self.col_sizes,
            Axis::Row => &mut self.row_sizes,
        };
        move_boundary(sizes, boundary, position, min_fraction)
    }

    /// Move a divider by `delta` of the grid's extent, as the arrow keys do.
    pub(crate) fn nudge_divider(
        &mut self,
        axis: Axis,
        boundary: usize,
        delta: f32,
        min_fraction: f32,
    ) -> bool {
        let sizes = match axis {
            Axis::Column => &self.col_sizes,
            Axis::Row => &self.row_sizes,
        };
        let current = f32::from(sizes.iter().take(boundary + 1).sum::<u16>()) / f32::from(WHOLE);
        self.move_divider(axis, boundary, current + delta, min_fraction)
    }

    /// Give every track on one axis the same size. Returns whether a size changed.
    pub(crate) fn equalize(&mut self, axis: Axis) -> bool {
        let (sizes, tracks) = match axis {
            Axis::Column => (&mut self.col_sizes, self.cols),
            Axis::Row => (&mut self.row_sizes, self.rows),
        };
        let equal = equal_sizes(tracks);
        let changed = *sizes != equal;
        *sizes = equal;
        changed
    }

    /// The cell next to `cell` in `direction`, if the grid has one there.
    pub(crate) fn neighbor(&self, cell: usize, direction: Direction) -> Option<usize> {
        let (cols, rows) = (usize::from(self.cols), usize::from(self.rows));
        if cell >= cols * rows {
            return None;
        }
        let (col, row) = (cell % cols, cell / cols);
        match direction {
            Direction::Left => col.checked_sub(1).map(|col| row * cols + col),
            Direction::Right => (col + 1 < cols).then(|| row * cols + col + 1),
            Direction::Up => row.checked_sub(1).map(|row| row * cols + col),
            Direction::Down => (row + 1 < rows).then(|| (row + 1) * cols + col),
        }
    }
}

/// Clamp a requested shape to the layouts the picker offers: three tracks per
/// axis and at most [`MAX_CELLS`] panes, trimming the longer axis first.
fn bound_shape(cols: u8, rows: u8) -> (u8, u8) {
    let mut cols = cols.clamp(1, MAX_TRACKS);
    let mut rows = rows.clamp(1, MAX_TRACKS);
    while usize::from(cols) * usize::from(rows) > MAX_CELLS {
        if cols >= rows {
            cols -= 1;
        } else {
            rows -= 1;
        }
    }
    (cols, rows)
}

fn equal_sizes(tracks: u8) -> Vec<u16> {
    let tracks = u16::from(tracks.max(1));
    let mut sizes = vec![WHOLE / tracks; usize::from(tracks)];
    // Rounding leftovers go to the last track so the sizes always sum to the whole.
    if let Some(last) = sizes.last_mut() {
        *last += WHOLE - (WHOLE / tracks) * tracks;
    }
    sizes
}

fn repair_sizes(sizes: &mut Vec<u16>, tracks: u8) {
    let valid = sizes.len() == usize::from(tracks)
        && sizes.iter().all(|size| *size >= MIN_TRACK)
        && sizes.iter().map(|size| u32::from(*size)).sum::<u32>() == u32::from(WHOLE);
    if !valid {
        *sizes = equal_sizes(tracks);
    }
}

fn fractions(sizes: &[u16]) -> Vec<f32> {
    sizes
        .iter()
        .map(|size| f32::from(*size) / f32::from(WHOLE))
        .collect()
}

fn move_boundary(sizes: &mut [u16], boundary: usize, position: f32, min_fraction: f32) -> bool {
    if !position.is_finite() || boundary + 1 >= sizes.len() {
        return false;
    }
    let before: u16 = sizes[..boundary].iter().sum();
    let pair = sizes[boundary] + sizes[boundary + 1];
    let floor = ((min_fraction.max(0.0) * f32::from(WHOLE)).round() as u16)
        .max(MIN_TRACK)
        .min(pair / 2);
    let wanted = (position * f32::from(WHOLE)).round() as i32 - i32::from(before);
    let first = wanted.clamp(i32::from(floor), i32::from(pair - floor)) as u16;
    if first == sizes[boundary] {
        return false;
    }
    sizes[boundary] = first;
    sizes[boundary + 1] = pair - first;
    true
}

/// Fit `cells` to `len` slots. Stale and repeated tabs are cleared, `keep` is
/// guaranteed a slot, and the remaining gaps fill with hidden tabs in tab order.
pub(crate) fn reconcile(cells: &mut Vec<Option<u64>>, len: usize, tabs: &[u64], keep: Option<u64>) {
    cells.resize(len, None);
    let mut seen: Vec<u64> = Vec::with_capacity(len);
    for slot in cells.iter_mut() {
        match *slot {
            Some(id) if tabs.contains(&id) && !seen.contains(&id) => seen.push(id),
            _ => *slot = None,
        }
    }
    if let Some(keep) = keep.filter(|id| tabs.contains(id) && !seen.contains(id)) {
        let slot = cells
            .iter()
            .position(Option::is_none)
            .unwrap_or(len.saturating_sub(1));
        if let Some(cell) = cells.get_mut(slot) {
            *cell = Some(keep);
        }
    }
    let mut hidden = tabs
        .iter()
        .copied()
        .filter(|id| !cells.contains(&Some(*id)))
        .collect::<Vec<_>>()
        .into_iter();
    for slot in cells.iter_mut().filter(|slot| slot.is_none()) {
        match hidden.next() {
            Some(id) => *slot = Some(id),
            None => break,
        }
    }
}

/// Show `tab` in `cell`. A tab already shown elsewhere trades places with the
/// cell's occupant. Returns whether the cells changed.
pub(crate) fn place(cells: &mut [Option<u64>], cell: usize, tab: u64) -> bool {
    let Some(occupant) = cells.get(cell).copied() else {
        return false;
    };
    if occupant == Some(tab) {
        return false;
    }
    match cells.iter().position(|slot| *slot == Some(tab)) {
        Some(from) => cells.swap(from, cell),
        None => cells[cell] = Some(tab),
    }
    true
}

/// The next occupied cell after `current` in reading order, wrapping around.
pub(crate) fn cycle(cells: &[Option<u64>], current: usize, reverse: bool) -> Option<usize> {
    let count = cells.len();
    (1..count)
        .map(|step| {
            if reverse {
                (current + count - step) % count
            } else {
                (current + step) % count
            }
        })
        .find(|index| cells[*index].is_some())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shapes_are_bounded_to_three_tracks_and_four_panes() {
        assert_eq!(bound_shape(0, 0), (1, 1));
        assert_eq!(bound_shape(3, 1), (3, 1));
        assert_eq!(bound_shape(1, 3), (1, 3));
        assert_eq!(bound_shape(2, 2), (2, 2));
        assert_eq!(bound_shape(3, 2), (2, 2));
        assert_eq!(bound_shape(2, 3), (2, 2));
        assert_eq!(bound_shape(9, 9), (2, 2));
        for (cols, rows) in SHAPES {
            assert_eq!(bound_shape(cols, rows), (cols, rows));
        }
    }

    #[test]
    fn new_grids_start_with_equal_tracks_that_sum_to_the_whole() {
        let grid = TerminalGrid::new(3, 1);
        assert_eq!(grid.col_sizes, [333, 333, 334]);
        assert_eq!(grid.row_sizes, [1000]);
        assert_eq!(grid.cell_count(), 3);
        assert!(!grid.is_single());
        assert!(TerminalGrid::default().is_single());
    }

    #[test]
    fn changing_shape_keeps_sizes_of_an_axis_with_the_same_track_count() {
        let mut grid = TerminalGrid::new(2, 1);
        assert!(grid.move_divider(Axis::Column, 0, 0.3, 0.1));
        assert!(grid.set_shape(2, 2));
        assert_eq!(grid.col_sizes, [300, 700]);
        assert_eq!(grid.row_sizes, [500, 500]);
        assert!(grid.set_shape(3, 1));
        assert_eq!(grid.col_sizes, [333, 333, 334]);
        assert!(!grid.set_shape(3, 1));
        assert!(grid.set_shape(9, 9));
        assert_eq!(grid.shape(), (2, 2));
    }

    #[test]
    fn dividers_move_only_their_neighbours_and_respect_the_minimum() {
        let mut grid = TerminalGrid::new(3, 1);
        assert!(grid.move_divider(Axis::Column, 0, 0.2, 0.1));
        assert_eq!(grid.col_sizes, [200, 466, 334]);
        assert!(grid.move_divider(Axis::Column, 1, 0.9, 0.1));
        assert_eq!(grid.col_sizes, [200, 700, 100]);
        assert!(grid.move_divider(Axis::Column, 0, -4.0, 0.1));
        assert_eq!(grid.col_sizes, [100, 800, 100]);
        assert!(!grid.move_divider(Axis::Column, 0, 0.0, 0.1));
        assert!(!grid.move_divider(Axis::Column, 2, 0.5, 0.1));
        assert!(!grid.move_divider(Axis::Column, 0, f32::NAN, 0.1));
        assert_eq!(grid.col_sizes.iter().sum::<u16>(), WHOLE);
    }

    #[test]
    fn an_oversized_minimum_still_leaves_both_tracks_visible() {
        let mut grid = TerminalGrid::new(2, 1);
        // Each side may claim at most half of the pair, so an equal split stays put.
        assert!(!grid.move_divider(Axis::Column, 0, 0.0, 0.9));
        assert!(!grid.move_divider(Axis::Column, 0, 1.0, 0.9));
        assert_eq!(grid.col_sizes, [500, 500]);
    }

    #[test]
    fn nudging_and_equalizing_adjust_one_axis() {
        let mut grid = TerminalGrid::new(2, 2);
        assert!(grid.nudge_divider(Axis::Row, 0, 0.1, 0.1));
        assert_eq!(grid.row_sizes, [600, 400]);
        assert_eq!(grid.col_sizes, [500, 500]);
        assert!(grid.nudge_divider(Axis::Row, 0, -0.2, 0.1));
        assert_eq!(grid.row_sizes, [400, 600]);
        assert!(grid.equalize(Axis::Row));
        assert!(!grid.equalize(Axis::Row));
        assert_eq!(grid.row_sizes, [500, 500]);
    }

    #[test]
    fn saved_layouts_repair_invalid_shapes_and_sizes() {
        let mut grid: TerminalGrid =
            serde_json::from_str(r#"{"cols":3,"rows":3,"col_sizes":[1,2],"row_sizes":[0,1000]}"#)
                .unwrap();
        grid.normalize();
        assert_eq!(grid.shape(), (2, 2));
        assert_eq!(grid.col_sizes, [500, 500]);
        assert_eq!(grid.row_sizes, [500, 500]);
        let mut grid: TerminalGrid =
            serde_json::from_str(r#"{"cols":2,"rows":1,"col_sizes":[250,750]}"#).unwrap();
        grid.normalize();
        assert_eq!(grid.col_sizes, [250, 750]);
        assert_eq!(grid.row_sizes, [1000]);
    }

    #[test]
    fn neighbours_follow_reading_order() {
        let grid = TerminalGrid::new(2, 2);
        assert_eq!(grid.neighbor(0, Direction::Right), Some(1));
        assert_eq!(grid.neighbor(0, Direction::Down), Some(2));
        assert_eq!(grid.neighbor(3, Direction::Left), Some(2));
        assert_eq!(grid.neighbor(3, Direction::Up), Some(1));
        assert_eq!(grid.neighbor(0, Direction::Left), None);
        assert_eq!(grid.neighbor(1, Direction::Right), None);
        assert_eq!(grid.neighbor(2, Direction::Down), None);
        assert_eq!(grid.neighbor(4, Direction::Up), None);
        assert_eq!(TerminalGrid::new(3, 1).neighbor(1, Direction::Up), None);
    }

    #[test]
    fn reconcile_fills_gaps_with_hidden_tabs_in_tab_order() {
        let mut cells = vec![Some(2)];
        reconcile(&mut cells, 3, &[1, 2, 3, 4], Some(2));
        assert_eq!(cells, [Some(2), Some(1), Some(3)]);
        reconcile(&mut cells, 3, &[1, 2, 3], Some(2));
        assert_eq!(cells, [Some(2), Some(1), Some(3)]);
    }

    #[test]
    fn reconcile_drops_closed_and_repeated_tabs_and_leaves_gaps_empty() {
        let mut cells = vec![Some(7), Some(1), Some(1), Some(2)];
        reconcile(&mut cells, 4, &[1, 2], Some(2));
        assert_eq!(cells, [None, Some(1), None, Some(2)]);
        let mut cells = vec![None, None];
        reconcile(&mut cells, 2, &[], None);
        assert_eq!(cells, [None, None]);
    }

    #[test]
    fn shrinking_keeps_the_focused_tab_visible() {
        let mut cells = vec![Some(1), Some(2), Some(3), Some(4)];
        reconcile(&mut cells, 2, &[1, 2, 3, 4], Some(4));
        assert_eq!(cells, [Some(1), Some(4)]);
        reconcile(&mut cells, 1, &[1, 2, 3, 4], Some(4));
        assert_eq!(cells, [Some(4)]);
        let mut cells = vec![Some(1), Some(2)];
        reconcile(&mut cells, 1, &[1, 2], Some(1));
        assert_eq!(cells, [Some(1)]);
    }

    #[test]
    fn placing_a_shown_tab_swaps_and_a_hidden_tab_replaces() {
        let mut cells = vec![Some(1), Some(2), None];
        assert!(place(&mut cells, 1, 1));
        assert_eq!(cells, [Some(2), Some(1), None]);
        assert!(place(&mut cells, 2, 1));
        assert_eq!(cells, [Some(2), None, Some(1)]);
        assert!(place(&mut cells, 0, 9));
        assert_eq!(cells, [Some(9), None, Some(1)]);
        assert!(!place(&mut cells, 0, 9));
        assert!(!place(&mut cells, 5, 1));
    }

    #[test]
    fn cycling_skips_empty_cells_and_wraps() {
        let cells = [Some(1), None, Some(3), Some(4)];
        assert_eq!(cycle(&cells, 0, false), Some(2));
        assert_eq!(cycle(&cells, 3, false), Some(0));
        assert_eq!(cycle(&cells, 0, true), Some(3));
        assert_eq!(cycle(&cells, 2, true), Some(0));
        assert_eq!(cycle(&[Some(1)], 0, false), None);
        assert_eq!(cycle(&[None, None], 0, false), None);
    }
}
