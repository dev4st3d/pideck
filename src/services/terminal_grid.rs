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

impl TerminalGrid {
    /// The cell under a point given as fractions of the grid's width and height.
    pub(crate) fn cell_at(&self, x: f32, y: f32) -> usize {
        track_at(&self.row_sizes, y) * usize::from(self.cols) + track_at(&self.col_sizes, x)
    }

    /// Which way a tab dropped at `(x, y)` in a `width` × `height` grid asks
    /// for a new row or column, read from where the point sits in the pane
    /// under it. Leaning to the left or right side opens a column there; the
    /// top or bottom opens a row. Deeper into a side zone wins, so the diagonals
    /// of a pane decide a corner. The pane's middle, and any point that leans
    /// nowhere, is `None`.
    ///
    /// `held` is the target the pointer already chose. Its side gets
    /// `sides.hold` extra reach, so a small drift does not flip it. Whether
    /// the layout can actually grow that way is for [`arrange`] to say.
    pub(crate) fn edge_target(
        &self,
        (x, y): (f32, f32),
        (width, height): (f32, f32),
        sides: SideZones,
        held: Option<DropTarget>,
    ) -> Option<DropTarget> {
        let finite = [x, y, width, height].iter().all(|value| value.is_finite());
        if !finite || width <= 0.0 || height <= 0.0 {
            return None;
        }
        let col = track_at(&self.col_sizes, x / width);
        let row = track_at(&self.row_sizes, y / height);
        let (left, across) = track_span(&self.col_sizes, col);
        let (top, down) = track_span(&self.row_sizes, row);
        let (pane_width, pane_height) = (across * width, down * height);
        let offset_x = x - (left * width + pane_width / 2.0);
        let offset_y = y - (top * height + pane_height / 2.0);
        // On each axis the point leans to one side; `before` counts the tracks
        // that would stay ahead of a new one opened there.
        let before_x = if offset_x < 0.0 { col } else { col + 1 };
        let before_y = if offset_y < 0.0 { row } else { row + 1 };
        let chosen = |axis, before| {
            matches!(
                held,
                Some(DropTarget::Edge { axis: held_axis, before: held_before, .. })
                    if held_axis == axis && held_before == before
            )
        };
        // How far into its side zone the point sits: 0 where the zone starts,
        // 1 on the border, negative in the middle.
        let depth = |offset: f32, extent: f32, held: bool| {
            let band = sides.band(extent);
            let reach = if held { sides.hold } else { 0.0 };
            (offset.abs() - (extent / 2.0 - band) + reach) / band
        };
        let across_depth = depth(offset_x, pane_width, chosen(Axis::Column, before_x));
        let down_depth = depth(offset_y, pane_height, chosen(Axis::Row, before_y));
        if across_depth <= 0.0 && down_depth <= 0.0 {
            return None;
        }
        Some(if across_depth >= down_depth {
            DropTarget::Edge {
                axis: Axis::Column,
                before: before_x,
                cross: row,
            }
        } else {
            DropTarget::Edge {
                axis: Axis::Row,
                before: before_y,
                cross: col,
            }
        })
    }
}

/// How far the side zones of a pane reach in from its borders.
#[derive(Clone, Copy, Debug)]
pub(crate) struct SideZones {
    /// Share of the pane's width or height a zone takes, before the limits.
    pub(crate) fraction: f32,
    pub(crate) min: f32,
    pub(crate) max: f32,
    /// Extra reach for a side that was already chosen.
    pub(crate) hold: f32,
}

impl SideZones {
    /// Depth of a side zone in a pane `extent` long. Two fifths at most, so the
    /// pane always keeps a middle.
    fn band(self, extent: f32) -> f32 {
        (extent * self.fraction)
            .clamp(self.min, self.max.max(self.min))
            .min(extent * 0.4)
    }
}

/// Where a dragged tab lands when it is released.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DropTarget {
    /// Show the tab in this pane; a tab already on screen trades places with it.
    Pane(usize),
    /// Open a new column or row on one side of a pane, or of the whole grid.
    /// `before` counts the tracks on `axis` that stay ahead of the new one,
    /// and `cross` is the row (or column) the tab takes.
    Edge {
        axis: Axis,
        before: usize,
        cross: usize,
    },
    /// Switch to a `cols` × `rows` layout and show the tab in `cell`.
    Layout { cols: u8, rows: u8, cell: usize },
}

/// A layout and the tab in each of its panes, before empty panes get a terminal.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Arrangement {
    pub(crate) shape: (u8, u8),
    pub(crate) cells: Vec<Option<u64>>,
}

impl Arrangement {
    /// The pane `tab` ends up in.
    pub(crate) fn pane_of(&self, tab: u64) -> Option<usize> {
        self.cells.iter().position(|cell| *cell == Some(tab))
    }
}

/// The layout after dropping `tab` on `target`, or `None` when nothing would
/// change. A new row or column takes a tab from the tab strip or from another
/// pane; the pane it leaves closes up if that empties a row or column. Gaps
/// fill from hidden tabs in tab order.
pub(crate) fn arrange(
    target: DropTarget,
    shape: (u8, u8),
    cells: &[Option<u64>],
    tab: u64,
    tabs: &[u64],
) -> Option<Arrangement> {
    if !tabs.contains(&tab) {
        return None;
    }
    match target {
        DropTarget::Pane(cell) => arrange_pane(shape, cells, cell, tab),
        DropTarget::Layout { cols, rows, cell } if (cols, rows) == shape => {
            arrange_pane(shape, cells, cell, tab)
        }
        DropTarget::Layout { cols, rows, cell } => {
            if !SHAPES.contains(&(cols, rows)) {
                return None;
            }
            let len = usize::from(cols) * usize::from(rows);
            // Tabs already on screen keep their reading order around the new one.
            let mut kept = cells.iter().flatten().copied().filter(|id| *id != tab);
            let mut next = vec![None; len];
            next[cell.min(len - 1)] = Some(tab);
            for slot in next.iter_mut().filter(|slot| slot.is_none()) {
                *slot = kept.next();
            }
            reconcile(&mut next, len, tabs, Some(tab));
            Some(Arrangement {
                shape: (cols, rows),
                cells: next,
            })
        }
        DropTarget::Edge {
            axis,
            before,
            cross,
        } => arrange_edge(shape, cells, (axis, before, cross), tab, tabs),
    }
}

/// Open a new row or column ahead of track `before` and show `tab` in it, in
/// the column (or row) `cross`. A tab that is already on screen leaves its
/// pane first, and a row or column that leaves empty closes up, so moving a
/// pane beside another one never costs a terminal.
fn arrange_edge(
    shape: (u8, u8),
    cells: &[Option<u64>],
    (axis, before, cross): (Axis, usize, usize),
    tab: u64,
    tabs: &[u64],
) -> Option<Arrangement> {
    let (cols, rows) = (usize::from(shape.0), usize::from(shape.1));
    let mut table: Vec<Vec<Option<u64>>> = (0..rows)
        .map(|row| {
            (0..cols)
                .map(|col| {
                    let shown = cells.get(row * cols + col).copied().flatten();
                    shown.filter(|id| *id != tab)
                })
                .collect()
        })
        .collect();
    let (mut before, mut cross) = (before, cross);
    let shown = cells.iter().position(|slot| *slot == Some(tab));
    if let Some(from) = shown.filter(|from| *from < cols * rows) {
        let (row, col) = (from / cols, from % cols);
        // Indices past a removed track slide down by one. A pointer over the
        // vacated track itself is over the tab's own pane: nothing to do.
        if table[row].iter().all(Option::is_none) {
            table.remove(row);
            match axis {
                Axis::Row => before -= usize::from(row < before),
                Axis::Column if cross == row => return None,
                Axis::Column => cross -= usize::from(row < cross),
            }
        }
        if table.iter().all(|line| line[col].is_none()) {
            table.iter_mut().for_each(|line| {
                line.remove(col);
            });
            match axis {
                Axis::Column => before -= usize::from(col < before),
                Axis::Row if cross == col => return None,
                Axis::Row => cross -= usize::from(col < cross),
            }
        }
    }
    let (old_rows, old_cols) = (table.len(), table.first().map_or(0, Vec::len));
    if old_rows == 0 || old_cols == 0 {
        return None;
    }
    let grown = grown_shape(
        (u8::try_from(old_cols).ok()?, u8::try_from(old_rows).ok()?),
        axis,
    )?;
    let (cols, rows) = (usize::from(grown.0), usize::from(grown.1));
    let (before, cross) = match axis {
        Axis::Column => (before.min(old_cols), cross.min(old_rows - 1)),
        Axis::Row => (before.min(old_rows), cross.min(old_cols - 1)),
    };
    let mut next = vec![None; cols * rows];
    for row in 0..rows {
        for col in 0..cols {
            // The new track stays empty; tracks after it shift over by one.
            let source = match axis {
                Axis::Column if col == before => continue,
                Axis::Column => (row, col - usize::from(col > before)),
                Axis::Row if row == before => continue,
                Axis::Row => (row - usize::from(row > before), col),
            };
            let kept = table.get(source.0).and_then(|line| line.get(source.1));
            next[row * cols + col] = kept.copied().flatten();
        }
    }
    let landing = match axis {
        Axis::Column => cross * cols + before,
        Axis::Row => before * cols + cross,
    };
    next[landing] = Some(tab);
    reconcile(&mut next, cols * rows, tabs, Some(tab));
    // Dropping a pane back where it came from is not a change.
    (grown != shape || next != cells).then_some(Arrangement {
        shape: grown,
        cells: next,
    })
}

fn arrange_pane(
    shape: (u8, u8),
    cells: &[Option<u64>],
    cell: usize,
    tab: u64,
) -> Option<Arrangement> {
    let mut next = cells.to_vec();
    place(&mut next, cell, tab).then_some(Arrangement { shape, cells: next })
}

/// The shape with one more track on `axis`, if the picker offers it.
fn grown_shape((cols, rows): (u8, u8), axis: Axis) -> Option<(u8, u8)> {
    let grown = match axis {
        Axis::Column => (cols + 1, rows),
        Axis::Row => (cols, rows + 1),
    };
    SHAPES.contains(&grown).then_some(grown)
}

/// The track holding `at`, a fraction of the axis; points past either end use the last or first.
fn track_at(sizes: &[u16], at: f32) -> usize {
    let at = (at.clamp(0.0, 1.0) * f32::from(WHOLE)) as u32;
    let mut end = 0;
    for (index, size) in sizes.iter().enumerate() {
        end += u32::from(*size);
        if at < end {
            return index;
        }
    }
    sizes.len().saturating_sub(1)
}

/// Start and length of track `index`, as fractions of its axis.
fn track_span(sizes: &[u16], index: usize) -> (f32, f32) {
    let start: u16 = sizes.iter().take(index).sum();
    let len = sizes.get(index).copied().unwrap_or_default();
    (
        f32::from(start) / f32::from(WHOLE),
        f32::from(len) / f32::from(WHOLE),
    )
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
    fn points_map_to_the_cell_under_them_by_track_size() {
        let mut grid = TerminalGrid::new(2, 2);
        assert!(grid.move_divider(Axis::Column, 0, 0.25, 0.1));
        assert_eq!(grid.cell_at(0.1, 0.1), 0);
        assert_eq!(grid.cell_at(0.3, 0.1), 1);
        assert_eq!(grid.cell_at(0.1, 0.9), 2);
        assert_eq!(grid.cell_at(0.9, 0.9), 3);
        // Points beyond the grid or not numbers still name a real cell.
        assert_eq!(grid.cell_at(-4.0, -4.0), 0);
        assert_eq!(grid.cell_at(4.0, 4.0), 3);
        assert!(grid.cell_at(f32::NAN, f32::NAN) < 4);
    }

    const AREA: (f32, f32) = (1000.0, 600.0);

    const SIDES: SideZones = SideZones {
        fraction: 0.25,
        min: 40.0,
        max: 160.0,
        hold: 16.0,
    };

    fn edge(grid: &TerminalGrid, x: f32, y: f32) -> Option<DropTarget> {
        grid.edge_target((x, y), AREA, SIDES, None)
    }

    fn column_edge(before: usize, cross: usize) -> Option<DropTarget> {
        Some(DropTarget::Edge {
            axis: Axis::Column,
            before,
            cross,
        })
    }

    fn row_edge(before: usize, cross: usize) -> Option<DropTarget> {
        Some(DropTarget::Edge {
            axis: Axis::Row,
            before,
            cross,
        })
    }

    #[test]
    fn leaning_to_a_side_of_a_pane_asks_for_a_row_or_column_there() {
        let single = TerminalGrid::default();
        assert_eq!(edge(&single, 50.0, 300.0), column_edge(0, 0));
        assert_eq!(edge(&single, 950.0, 300.0), column_edge(1, 0));
        assert_eq!(edge(&single, 500.0, 40.0), row_edge(0, 0));
        assert_eq!(edge(&single, 500.0, 560.0), row_edge(1, 0));
        // The middle of the pane leans nowhere.
        assert_eq!(edge(&single, 500.0, 300.0), None);
        assert_eq!(edge(&single, 300.0, 200.0), None);
        // The zone ends exactly where the middle starts.
        assert_eq!(edge(&single, 159.0, 300.0), column_edge(0, 0));
        assert_eq!(edge(&single, 161.0, 300.0), None);
    }

    #[test]
    fn a_corner_goes_to_the_side_the_pointer_is_deeper_in() {
        let single = TerminalGrid::default();
        assert_eq!(edge(&single, 100.0, 100.0), column_edge(0, 0));
        assert_eq!(edge(&single, 100.0, 60.0), row_edge(0, 0));
        assert_eq!(edge(&single, 900.0, 540.0), row_edge(1, 0));
        assert_eq!(edge(&single, 990.0, 500.0), column_edge(1, 0));
        // Far along one border, only that side is in play.
        assert_eq!(edge(&single, 20.0, 200.0), column_edge(0, 0));
        assert_eq!(edge(&single, 300.0, 10.0), row_edge(0, 0));
    }

    #[test]
    fn the_sides_of_neighbouring_panes_open_the_same_gap() {
        let pair = TerminalGrid::new(2, 1);
        assert_eq!(edge(&pair, 450.0, 300.0), column_edge(1, 0));
        assert_eq!(edge(&pair, 550.0, 300.0), column_edge(1, 0));
        // The outer sides still reach the ends of the grid.
        assert_eq!(edge(&pair, 20.0, 300.0), column_edge(0, 0));
        assert_eq!(edge(&pair, 980.0, 300.0), column_edge(2, 0));
        // A row opens above or below the pane the pointer is over.
        assert_eq!(edge(&pair, 250.0, 580.0), row_edge(1, 0));
        assert_eq!(edge(&pair, 750.0, 20.0), row_edge(0, 1));
        let stack = TerminalGrid::new(1, 2);
        assert_eq!(edge(&stack, 500.0, 280.0), row_edge(1, 0));
        assert_eq!(edge(&stack, 500.0, 320.0), row_edge(1, 0));
        assert_eq!(edge(&stack, 990.0, 500.0), column_edge(1, 1));
    }

    #[test]
    fn a_chosen_side_keeps_extra_reach_and_wins_close_calls() {
        let single = TerminalGrid::default();
        let held = column_edge(0, 0);
        let hold = |point| single.edge_target(point, AREA, SIDES, held);
        // Just outside the zone the chosen side still counts; a fresh one does not.
        assert_eq!(edge(&single, 170.0, 300.0), None);
        assert_eq!(hold((170.0, 300.0)), column_edge(0, 0));
        assert_eq!(hold((190.0, 300.0)), None);
        // In a corner it wins a close call.
        assert_eq!(edge(&single, 100.0, 80.0), row_edge(0, 0));
        assert_eq!(hold((100.0, 80.0)), column_edge(0, 0));
        // It does not claim the opposite side.
        assert_eq!(hold((950.0, 300.0)), column_edge(1, 0));
        // Only an edge target is held.
        let pane = Some(DropTarget::Pane(0));
        assert_eq!(single.edge_target((170.0, 300.0), AREA, SIDES, pane), None);
    }

    #[test]
    fn a_chosen_side_follows_the_pointer_along_it() {
        let stack = TerminalGrid::new(1, 2);
        let held = column_edge(1, 0);
        assert_eq!(
            stack.edge_target((990.0, 500.0), AREA, SIDES, held),
            column_edge(1, 1)
        );
    }

    #[test]
    fn small_panes_keep_a_middle() {
        let single = TerminalGrid::default();
        let size = (100.0, 40.0);
        // A zone takes at most two fifths of the pane, so a middle strip remains.
        assert_eq!(
            single.edge_target((50.0, 5.0), size, SIDES, None),
            row_edge(0, 0)
        );
        assert_eq!(single.edge_target((50.0, 20.0), size, SIDES, None), None);
        assert_eq!(
            single.edge_target((50.0, 5.0), (100.0, 0.0), SIDES, None),
            None
        );
        assert_eq!(single.edge_target((f32::NAN, 5.0), size, SIDES, None), None);
    }

    #[test]
    fn an_edge_drop_opens_a_pane_beside_the_others() {
        let tabs = [1, 2, 3];
        let shape = (1, 1);
        let right = column_edge(1, 0).unwrap();
        let left = column_edge(0, 0).unwrap();
        let result = arrange(right, shape, &[Some(1)], 2, &tabs).unwrap();
        assert_eq!(result.shape, (2, 1));
        assert_eq!(result.cells, [Some(1), Some(2)]);
        assert_eq!(result.pane_of(2), Some(1));
        let result = arrange(left, shape, &[Some(1)], 2, &tabs).unwrap();
        assert_eq!(result.cells, [Some(2), Some(1)]);
        let result = arrange(row_edge(0, 0).unwrap(), shape, &[Some(1)], 3, &tabs).unwrap();
        assert_eq!(result.shape, (1, 2));
        assert_eq!(result.cells, [Some(3), Some(1)]);
    }

    #[test]
    fn an_edge_drop_keeps_the_other_rows_where_they_were() {
        let tabs = [1, 2, 3, 4];
        // Two stacked panes gain a column; the new tab lands beside the lower one.
        let result = arrange(
            column_edge(0, 1).unwrap(),
            (1, 2),
            &[Some(1), Some(2)],
            3,
            &tabs,
        );
        let result = result.unwrap();
        assert_eq!(result.shape, (2, 2));
        // Tab 4 is hidden, so it fills the new column's other pane.
        assert_eq!(result.cells, [Some(4), Some(1), Some(3), Some(2)]);
        // A new row above two side-by-side panes pushes them down.
        let result = arrange(
            row_edge(0, 1).unwrap(),
            (2, 1),
            &[Some(1), Some(2)],
            3,
            &[1, 2, 3],
        );
        let result = result.unwrap();
        assert_eq!(result.shape, (2, 2));
        assert_eq!(result.cells, [None, Some(3), Some(1), Some(2)]);
    }

    #[test]
    fn an_edge_drop_cannot_grow_a_full_layout_or_move_the_only_pane() {
        let tabs = [1, 2, 3, 4];
        let right = column_edge(1, 0).unwrap();
        let full = [Some(1), Some(2), Some(3)];
        assert_eq!(arrange(right, (3, 1), &full, 4, &tabs), None);
        assert_eq!(arrange(right, (1, 1), &[Some(1)], 9, &tabs), None);
        // The only pane on screen has nothing to open beside.
        assert_eq!(arrange(right, (1, 1), &[Some(1)], 1, &tabs), None);
        // A 2 × 2 grid has no row or column to give up and none to add.
        let grid = [Some(1), Some(2), Some(3), Some(4)];
        assert_eq!(arrange(right, (2, 2), &grid, 1, &tabs), None);
    }

    #[test]
    fn a_pane_moved_beside_another_closes_up_where_it_left() {
        let tabs = [1, 2];
        let side_by_side = [Some(1), Some(2)];
        // Under the second pane: the two stack, with the second on top.
        let below = row_edge(1, 1).unwrap();
        let result = arrange(below, (2, 1), &side_by_side, 1, &tabs).unwrap();
        assert_eq!(result.shape, (1, 2));
        assert_eq!(result.cells, [Some(2), Some(1)]);
        // Past the second pane's right side: the two trade places.
        let past = column_edge(2, 0).unwrap();
        let result = arrange(past, (2, 1), &side_by_side, 1, &tabs).unwrap();
        assert_eq!(result.shape, (2, 1));
        assert_eq!(result.cells, [Some(2), Some(1)]);
        // Above the second pane: stacked the other way round.
        let above = row_edge(0, 1).unwrap();
        let result = arrange(above, (2, 1), &side_by_side, 1, &tabs).unwrap();
        assert_eq!(result.shape, (1, 2));
        assert_eq!(result.cells, [Some(1), Some(2)]);
    }

    #[test]
    fn a_pane_dropped_back_beside_itself_changes_nothing() {
        let cells = [Some(1), Some(2)];
        for target in [
            column_edge(0, 0),
            column_edge(1, 0),
            row_edge(0, 0),
            row_edge(1, 0),
        ] {
            assert_eq!(arrange(target.unwrap(), (2, 1), &cells, 1, &[1, 2]), None);
        }
    }

    #[test]
    fn a_pane_moved_out_of_a_row_leaves_room_for_a_hidden_tab() {
        let cells = [Some(1), Some(2), Some(3)];
        let under_third = row_edge(1, 2).unwrap();
        // The first two panes share the top; the free corner stays empty...
        let result = arrange(under_third, (3, 1), &cells, 1, &[1, 2, 3]).unwrap();
        assert_eq!(result.shape, (2, 2));
        assert_eq!(result.cells, [Some(2), Some(3), None, Some(1)]);
        // ...unless a hidden tab can take it.
        let result = arrange(under_third, (3, 1), &cells, 1, &[1, 2, 3, 4]).unwrap();
        assert_eq!(result.cells, [Some(2), Some(3), Some(4), Some(1)]);
    }

    #[test]
    fn a_layout_drop_keeps_visible_tabs_in_reading_order() {
        let tabs = [1, 2, 3, 4];
        let target = DropTarget::Layout {
            cols: 3,
            rows: 1,
            cell: 0,
        };
        let result = arrange(target, (2, 1), &[Some(1), Some(2)], 4, &tabs).unwrap();
        assert_eq!(result.shape, (3, 1));
        assert_eq!(result.cells, [Some(4), Some(1), Some(2)]);
        // Shrinking hides what no longer fits; the dropped tab always stays.
        let target = DropTarget::Layout {
            cols: 2,
            rows: 1,
            cell: 1,
        };
        let result = arrange(
            target,
            (2, 2),
            &[Some(1), Some(2), Some(3), Some(4)],
            4,
            &tabs,
        );
        assert_eq!(result.unwrap().cells, [Some(1), Some(4)]);
        // A tab moved from another pane leaves no copy behind.
        let result = arrange(target, (3, 1), &[Some(1), Some(2), Some(3)], 3, &tabs).unwrap();
        assert_eq!(result.cells, [Some(1), Some(3)]);
    }

    #[test]
    fn a_layout_drop_fills_gaps_from_hidden_tabs_and_rejects_unknown_shapes() {
        let tabs = [1, 2, 3];
        let target = DropTarget::Layout {
            cols: 2,
            rows: 2,
            cell: 3,
        };
        let result = arrange(target, (1, 1), &[Some(1)], 3, &tabs).unwrap();
        assert_eq!(result.cells, [Some(1), Some(2), None, Some(3)]);
        let target = DropTarget::Layout {
            cols: 4,
            rows: 1,
            cell: 0,
        };
        assert_eq!(arrange(target, (1, 1), &[Some(1)], 2, &tabs), None);
    }

    #[test]
    fn dropping_on_a_pane_or_the_current_layout_swaps_or_replaces() {
        let tabs = [1, 2, 3];
        let cells = [Some(1), Some(2)];
        let result = arrange(DropTarget::Pane(1), (2, 1), &cells, 1, &tabs).unwrap();
        assert_eq!(result.shape, (2, 1));
        assert_eq!(result.cells, [Some(2), Some(1)]);
        let same_shape = DropTarget::Layout {
            cols: 2,
            rows: 1,
            cell: 0,
        };
        let result = arrange(same_shape, (2, 1), &cells, 3, &tabs).unwrap();
        assert_eq!(result.cells, [Some(3), Some(2)]);
        assert_eq!(arrange(DropTarget::Pane(0), (2, 1), &cells, 1, &tabs), None);
        assert_eq!(arrange(DropTarget::Pane(5), (2, 1), &cells, 3, &tabs), None);
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
