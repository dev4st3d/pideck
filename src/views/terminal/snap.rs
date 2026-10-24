//! Drag-to-snap for tabs. While a tab is dragged over the pane area, the side of
//! the pane under the pointer opens a new row or column there, a tray offers
//! whole layouts, and a preview shows the result.
//!
//! Which way to split is read from where the pointer sits in its pane: leaning
//! to the left or right opens a column, to the top or bottom a row, and the
//! middle replaces or swaps the pane. Any pane works, not only the outer edge of
//! the area, and a tab that is already on screen can be moved this way too.
//!
//! The pane under the pointer is the default aim. A side zone and the tray only
//! take over once the pointer lingers in them, so sweeping across the area never
//! changes the layout by accident; once taken over they reach a little further,
//! so a small drift does not drop the choice. Releasing applies exactly the
//! target the preview shows.

use gpui::{
    AnyElement, Bounds, Context, DragMoveEvent, IntoElement, MouseButton, MouseMoveEvent,
    MouseUpEvent, Pixels, Point, Task, Window, div, prelude::*, px, relative,
};

use super::TerminalView;
use super::grid::{PaneDrag, SHAPE_LABELS, wash};
use crate::services::terminal_grid::{Arrangement, DropTarget, SHAPES, SideZones, arrange};
use crate::theme;
use crate::theme::terminal_manager as chrome;

/// How deep the zones along the sides of a pane reach.
const SIDES: SideZones = SideZones {
    fraction: chrome::SNAP_SIDE_FRACTION,
    min: chrome::SNAP_SIDE_MIN,
    max: chrome::SNAP_SIDE_MAX,
    hold: chrome::SNAP_HOLD,
};

/// Border of the layout tray.
const TRAY_BORDER: f32 = 1.0;
/// Room kept beside the tray so it never touches the sides of a narrow pane area.
const TRAY_MARGIN: f32 = 16.0;
/// Pane area that must stay below the tray so the preview remains readable.
const TRAY_CLEARANCE: f32 = 120.0;

/// A zone the pointer can be deliberately aiming at, as opposed to passing over.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Aim {
    /// A side zone of a pane, which opens a row or column there.
    Edge,
    /// The layout tray.
    Tray,
}

/// What lies under the pointer, before it is known whether the pointer means it.
struct Probe {
    /// The zone the pointer is in, if it is in one that must be aimed at.
    aim: Option<Aim>,
    /// What a release would do if the pointer does mean this spot.
    target: Option<DropTarget>,
}

/// A tab being dragged over the pane area, and what releasing it here would do.
pub(super) struct SnapDrag {
    tab: u64,
    bounds: Bounds<Pixels>,
    /// The pointer, relative to the pane area.
    pointer: Point<Pixels>,
    /// What a release does now. Exactly what the preview shows.
    target: Option<DropTarget>,
    /// The zone the pointer has lingered in. Moves inside it take effect at once.
    engaged: Option<Aim>,
    /// Engages the zone the pointer is in once it has stayed there long enough.
    /// Dropping the task cancels it, so leaving the zone needs no bookkeeping.
    dwell: Option<(Aim, Task<()>)>,
}

impl SnapDrag {
    fn point(&self) -> (f32, f32) {
        (f32::from(self.pointer.x), f32::from(self.pointer.y))
    }

    fn area(&self) -> (f32, f32) {
        (
            f32::from(self.bounds.size.width),
            f32::from(self.bounds.size.height),
        )
    }
}

/// The layout tray's box inside a pane area, in logical pixels.
struct Tray {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

/// Width and height of the tray: a row of thumbnails above a caption.
fn tray_size() -> (f32, f32) {
    let count = SHAPES.len() as f32;
    let inset = TRAY_BORDER + chrome::SNAP_TRAY_PAD;
    let width =
        count * chrome::SNAP_THUMB_WIDTH + (count - 1.0) * chrome::SNAP_THUMB_GAP + 2.0 * inset;
    let height =
        chrome::SNAP_THUMB_HEIGHT + chrome::SNAP_TRAY_GAP + chrome::SNAP_TRAY_FOOTER + 2.0 * inset;
    (width, height)
}

/// The tray for a pane area of `area` pixels, hidden when the area is too small for it.
fn tray(area: (f32, f32)) -> Option<Tray> {
    let (width, height) = tray_size();
    let fits = area.0 >= width + 2.0 * TRAY_MARGIN
        && area.1 >= chrome::SNAP_TRAY_TOP + height + TRAY_CLEARANCE;
    fits.then(|| Tray {
        x: (area.0 - width) / 2.0,
        y: chrome::SNAP_TRAY_TOP,
        width,
        height,
    })
}

/// What the pointer is over in the tray: `None` outside it, `Some(None)` on its
/// padding, and the layout cell under it otherwise. `reach` widens the tray on
/// every side; the extra margin counts as padding.
fn tray_hit(point: (f32, f32), area: (f32, f32), reach: f32) -> Option<Option<DropTarget>> {
    let tray = tray(area)?;
    let (x, y) = (point.0 - tray.x, point.1 - tray.y);
    if x < -reach || y < -reach || x >= tray.width + reach || y >= tray.height + reach {
        return None;
    }
    let inset = TRAY_BORDER + chrome::SNAP_TRAY_PAD;
    let cell = SHAPES
        .iter()
        .enumerate()
        .find_map(|(index, &(cols, rows))| {
            let left = inset + index as f32 * (chrome::SNAP_THUMB_WIDTH + chrome::SNAP_THUMB_GAP);
            let (across, down) = (x - left, y - inset);
            let inside = (0.0..chrome::SNAP_THUMB_WIDTH).contains(&across)
                && (0.0..chrome::SNAP_THUMB_HEIGHT).contains(&down);
            inside.then(|| {
                let col = (across / chrome::SNAP_THUMB_WIDTH * f32::from(cols)) as usize;
                let row = (down / chrome::SNAP_THUMB_HEIGHT * f32::from(rows)) as usize;
                DropTarget::Layout {
                    cols,
                    rows,
                    cell: row.min(usize::from(rows) - 1) * usize::from(cols)
                        + col.min(usize::from(cols) - 1),
                }
            })
        });
    Some(cell)
}

/// Where `cell` sits in a `shape` layout, in words.
fn position_label(shape: (u8, u8), cell: usize) -> &'static str {
    match (shape, cell) {
        ((1, 1), _) => "Whole area",
        ((2, 1), 0) => "Left half",
        ((2, 1), _) => "Right half",
        ((1, 2), 0) => "Top half",
        ((1, 2), _) => "Bottom half",
        ((3, 1), 0) => "Left third",
        ((3, 1), 1) => "Middle third",
        ((3, 1), _) => "Right third",
        ((1, 3), 0) => "Top third",
        ((1, 3), 1) => "Middle third",
        ((1, 3), _) => "Bottom third",
        ((2, 2), 0) => "Top left",
        ((2, 2), 1) => "Top right",
        ((2, 2), 2) => "Bottom left",
        ((2, 2), _) => "Bottom right",
        _ => "Pane",
    }
}

fn layout_label(cols: u8, rows: u8) -> &'static str {
    SHAPES
        .iter()
        .position(|shape| *shape == (cols, rows))
        .map_or("Custom grid", |index| SHAPE_LABELS[index])
}

/// Start and length of track `index` as fractions of its axis.
fn span(fractions: &[f32], index: usize) -> (f32, f32) {
    let start = fractions.iter().take(index).sum();
    (start, fractions.get(index).copied().unwrap_or_default())
}

impl TerminalView {
    fn arrangement(&self, target: DropTarget, tab: u64) -> Option<Arrangement> {
        arrange(target, self.grid.shape(), &self.cells, tab, &self.tab_ids())
    }

    /// Show `tab` as `target` describes: reshape the grid, move the tab in, and
    /// open a terminal in any pane that is left empty, as the layout menu does.
    pub(super) fn apply_drop(
        &mut self,
        tab: u64,
        target: DropTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(next) = self.arrangement(target, tab) else {
            return;
        };
        self.grid.set_shape(next.shape.0, next.shape.1);
        self.cells = next.cells;
        self.fill_empty_panes(cx);
        self.select_tab(tab, window, cx);
    }

    /// Dropping on the pane at `point`, if that would change anything.
    fn pane_under(&self, point: (f32, f32), area: (f32, f32), tab: u64) -> Option<DropTarget> {
        let target = DropTarget::Pane(self.grid.cell_at(point.0 / area.0, point.1 / area.1));
        self.arrangement(target, tab).is_some().then_some(target)
    }

    /// What lies under the pointer, and whether it is in a zone that must be aimed at.
    fn probe(&self, snap: &SnapDrag) -> Probe {
        let (point, area) = (snap.point(), snap.area());
        let valid = |target: DropTarget| {
            self.arrangement(target, snap.tab)
                .is_some()
                .then_some(target)
        };
        // The tray the pointer already lingered in keeps a little extra reach.
        let tray_reach = if snap.engaged == Some(Aim::Tray) {
            chrome::SNAP_HOLD
        } else {
            0.0
        };
        if let Some(over) = tray_hit(point, area, tray_reach) {
            // Crossing the gap between two thumbnails keeps the layout last aimed at.
            let kept = snap.target.filter(|target| {
                snap.engaged == Some(Aim::Tray) && matches!(target, DropTarget::Layout { .. })
            });
            return Probe {
                aim: Some(Aim::Tray),
                target: over.or(kept).and_then(valid),
            };
        }
        // A side only counts when the layout can take a row or column there;
        // otherwise the pane is simply the target, as in a full 2 × 2 grid.
        let held = snap.target.filter(|_| snap.engaged == Some(Aim::Edge));
        let side = self.grid.edge_target(point, area, SIDES, held);
        if let Some(side) = side.and_then(valid) {
            return Probe {
                aim: Some(Aim::Edge),
                target: Some(side),
            };
        }
        Probe {
            aim: None,
            target: self.pane_under(point, area, snap.tab),
        }
    }

    /// Work out what a release would do from where the pointer is, and start or
    /// cancel the timer that lets a side zone or the tray take over. Reports
    /// whether the target changed.
    fn refresh_snap(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(mut snap) = self.snap.take() else {
            return false;
        };
        let probe = self.probe(&snap);
        // Leaving a zone ends its hold; aiming goes back to the pane underneath.
        snap.engaged = snap.engaged.filter(|aim| probe.aim == Some(*aim));
        let previous = snap.target;
        snap.target = if probe.aim.is_none() || snap.engaged.is_some() {
            probe.target
        } else {
            // The pointer is only passing through: stay on the pane below until it lingers.
            self.pane_under(snap.point(), snap.area(), snap.tab)
        };
        let waiting = probe.aim.filter(|aim| snap.engaged != Some(*aim));
        snap.dwell = match (waiting, snap.dwell.take()) {
            (Some(aim), Some((pending, task))) if pending == aim => Some((pending, task)),
            (Some(aim), _) => Some((aim, Self::dwell_timer(aim, cx))),
            (None, _) => None,
        };
        let changed = snap.target != previous;
        self.snap = Some(snap);
        changed
    }

    fn dwell_timer(aim: Aim, cx: &mut Context<Self>) -> Task<()> {
        cx.spawn(async move |view, cx| {
            cx.background_executor().timer(chrome::SNAP_DWELL).await;
            let _ = view.update(cx, |view, cx| view.engage_snap(aim, cx));
        })
    }

    /// The pointer stayed in `aim`'s zone long enough: take its target.
    fn engage_snap(&mut self, aim: Aim, cx: &mut Context<Self>) {
        let Some(snap) = self.snap.as_mut() else {
            return;
        };
        if !snap
            .dwell
            .as_ref()
            .is_some_and(|(pending, _)| *pending == aim)
        {
            return;
        }
        snap.engaged = Some(aim);
        if self.refresh_snap(cx) {
            cx.notify();
        }
    }

    fn track_snap(
        &mut self,
        tab: u64,
        event: &MouseMoveEvent,
        bounds: Bounds<Pixels>,
        cx: &mut Context<Self>,
    ) {
        // A release outside the window never arrives; the next move without the button ends the drag.
        if event.pressed_button != Some(MouseButton::Left) || !bounds.contains(&event.position) {
            self.clear_snap(cx);
            return;
        }
        let pointer = event.position - bounds.origin;
        let fresh = self.snap.as_ref().is_none_or(|snap| snap.tab != tab);
        if fresh {
            self.snap = Some(SnapDrag {
                tab,
                bounds,
                pointer,
                target: None,
                engaged: None,
                dwell: None,
            });
        } else if let Some(snap) = self.snap.as_mut() {
            snap.bounds = bounds;
            snap.pointer = pointer;
        }
        if self.refresh_snap(cx) || fresh {
            cx.notify();
        }
    }

    fn clear_snap(&mut self, cx: &mut Context<Self>) {
        if self.snap.take().is_some() {
            cx.notify();
        }
    }

    fn finish_snap(&mut self, event: &MouseUpEvent, window: &mut Window, cx: &mut Context<Self>) {
        let Some(snap) = self.snap.take() else {
            return;
        };
        cx.notify();
        // Without a live drag this is an ordinary click, and the state is left over.
        if !cx.has_active_drag()
            || event.button != MouseButton::Left
            || !snap.bounds.contains(&event.position)
        {
            return;
        }
        // The preview was the promise: apply what it showed, not a fresh guess.
        if let Some(target) = snap.target {
            self.apply_drop(snap.tab, target, window, cx);
        }
    }

    /// Wrap the panes (or the single tab) in the area that receives snap drags.
    pub(super) fn snap_host(&self, content: AnyElement, cx: &mut Context<Self>) -> AnyElement {
        div()
            .id("terminal-body")
            .debug_selector(|| "terminal-body".into())
            .relative()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .flex()
            .flex_col()
            .on_drag_move(cx.listener(|view, event: &DragMoveEvent<PaneDrag>, _, cx| {
                let tab = event.drag(cx).tab;
                view.track_snap(tab, &event.event, event.bounds, cx);
            }))
            // Capture phase, so a terminal underneath cannot swallow the release.
            .capture_any_mouse_up(cx.listener(|view, event: &MouseUpEvent, window, cx| {
                view.finish_snap(event, window, cx)
            }))
            // A divider occludes the area beneath it, so a release on one counts as outside.
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|view, event: &MouseUpEvent, window, cx| {
                    view.finish_snap(event, window, cx)
                }),
            )
            .child(content)
            .children(self.snap_overlay(cx))
            .into_any_element()
    }

    fn snap_overlay(&self, cx: &Context<Self>) -> Option<AnyElement> {
        let snap = self.snap.as_ref().filter(|_| cx.has_active_drag())?;
        let area = (
            f32::from(snap.bounds.size.width),
            f32::from(snap.bounds.size.height),
        );
        let preview = snap
            .target
            .and_then(|target| self.snap_preview(target, snap.tab));
        Some(
            div()
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .children(preview)
                .children(tray(area).map(|_| self.snap_tray(snap.target)))
                .into_any_element(),
        )
    }

    /// The layout the release would produce, with the tab's pane highlighted.
    fn snap_preview(&self, target: DropTarget, tab: u64) -> Option<AnyElement> {
        let next = self.arrangement(target, tab)?;
        let landing = next.pane_of(tab)?;
        let reshaped = next.shape != self.grid.shape();
        // A row or column drop redraws the layout even when its shape stays the same.
        let redrawn = reshaped || matches!(target, DropTarget::Edge { .. });
        let mut layout = self.grid.clone();
        layout.set_shape(next.shape.0, next.shape.1);
        let columns = layout.column_fractions();
        let rows = layout.row_fractions();
        let label = if redrawn {
            position_label(next.shape, landing)
        } else if self.is_shown(tab) {
            "Swap places"
        } else {
            "Show here"
        };
        // Moving within the layout highlights one pane; a new layout outlines them all.
        let tiles = (0..columns.len() * rows.len())
            .filter(|cell| redrawn || *cell == landing)
            .map(|cell| {
                let (left, width) = span(&columns, cell % columns.len());
                let (top, height) = span(&rows, cell / columns.len());
                let target = cell == landing;
                snap_tile(left, top, width, height, target.then_some(label))
            });
        Some(
            div()
                .debug_selector(|| "snap-preview".into())
                .absolute()
                .top_0()
                .left_0()
                .size_full()
                .children(tiles)
                .into_any_element(),
        )
    }

    fn snap_tray(&self, hovered: Option<DropTarget>) -> AnyElement {
        let current = self.grid.shape();
        let (width, height) = tray_size();
        let footer = match hovered {
            Some(DropTarget::Layout { cols, rows, cell }) => format!(
                "{} · {}",
                layout_label(cols, rows),
                position_label((cols, rows), cell)
            ),
            Some(DropTarget::Pane(_) | DropTarget::Edge { .. }) | None => {
                "Drop on a layout to snap the tab there".to_owned()
            }
        };
        div()
            .absolute()
            .top(px(chrome::SNAP_TRAY_TOP))
            .left_0()
            .right_0()
            .flex()
            .justify_center()
            .child(
                div()
                    .debug_selector(|| "snap-tray".into())
                    .w(px(width))
                    .h(px(height))
                    .p(px(chrome::SNAP_TRAY_PAD))
                    .flex()
                    .flex_col()
                    .gap(px(chrome::SNAP_TRAY_GAP))
                    .rounded(px(chrome::SNAP_TILE_RADIUS))
                    .border_1()
                    .border_color(theme::edge_hard())
                    .bg(theme::panel_lift())
                    .font_family(chrome::CHROME_FONT)
                    .text_size(px(chrome::DETAIL_TEXT_SIZE))
                    .text_color(theme::ash())
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .gap(px(chrome::SNAP_THUMB_GAP))
                            .children(SHAPES.into_iter().map(|(cols, rows)| {
                                snap_thumbnail(cols, rows, (cols, rows) == current, hovered)
                            })),
                    )
                    .child(
                        div()
                            .h(px(chrome::SNAP_TRAY_FOOTER))
                            .flex()
                            .items_center()
                            .truncate()
                            .child(footer),
                    ),
            )
            .into_any_element()
    }
}

/// One pane of the previewed layout; the target is filled and named.
fn snap_tile(
    left: f32,
    top: f32,
    width: f32,
    height: f32,
    label: Option<&'static str>,
) -> AnyElement {
    let target = label.is_some();
    div()
        .absolute()
        .left(relative(left))
        .top(relative(top))
        .w(relative(width))
        .h(relative(height))
        .p(px(chrome::SNAP_TILE_INSET))
        .child(
            div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .rounded(px(chrome::SNAP_TILE_RADIUS))
                .when(target, |tile| {
                    tile.border_2()
                        .border_color(theme::focus())
                        .bg(wash(theme::focus(), 0.2))
                })
                .when(!target, |tile| {
                    tile.border_1()
                        .border_color(theme::edge_hard())
                        .bg(wash(theme::canvas(), 0.6))
                })
                .children(label.map(|label| {
                    div()
                        .h(px(chrome::CONTROL_HEIGHT))
                        .px(px(chrome::INSET))
                        .flex()
                        .items_center()
                        .rounded(px(chrome::CONTROL_RADIUS))
                        .border_1()
                        .border_color(theme::focus())
                        .bg(theme::panel_lift())
                        .font_family(chrome::CHROME_FONT)
                        .text_size(px(chrome::CONTROL_TEXT_SIZE))
                        .text_color(theme::bone())
                        .child(label)
                })),
        )
        .into_any_element()
}

/// A tray entry: a miniature of one layout whose panes are each a drop target.
fn snap_thumbnail(
    cols: u8,
    rows: u8,
    current: bool,
    hovered: Option<DropTarget>,
) -> impl IntoElement {
    div()
        .relative()
        .w(px(chrome::SNAP_THUMB_WIDTH))
        .h(px(chrome::SNAP_THUMB_HEIGHT))
        .flex_shrink_0()
        .children((0..usize::from(cols) * usize::from(rows)).map(move |cell| {
            let lit = hovered == Some(DropTarget::Layout { cols, rows, cell });
            let (col, row) = (cell % usize::from(cols), cell / usize::from(cols));
            div()
                .absolute()
                .left(relative(col as f32 / f32::from(cols)))
                .top(relative(row as f32 / f32::from(rows)))
                .w(relative(1.0 / f32::from(cols)))
                .h(relative(1.0 / f32::from(rows)))
                .p(px(1.5))
                .child(
                    div()
                        .size_full()
                        .rounded(px(2.0))
                        .border_1()
                        .border_color(if lit {
                            theme::focus()
                        } else if current {
                            theme::ash()
                        } else {
                            theme::edge_hard()
                        })
                        .bg(if lit {
                            theme::focus()
                        } else {
                            wash(theme::bone(), 0.1)
                        }),
                )
        }))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::Duration;

    use gpui::{Entity, Modifiers, TestAppContext, VisualTestContext, point, size};

    use super::*;
    use crate::services::terminal_grid::{Axis, TerminalGrid};

    const AREA: (f32, f32) = (1000.0, 600.0);

    #[test]
    fn the_tray_is_centred_below_the_top_band_and_hides_in_small_areas() {
        let tray = tray(AREA).unwrap();
        assert_eq!(tray.y, chrome::SNAP_TRAY_TOP);
        assert!((tray.x * 2.0 + tray.width - AREA.0).abs() < 0.01);
        assert!(tray.height < AREA.1);
        assert!(super::tray((tray.width, AREA.1)).is_none());
        assert!(super::tray((AREA.0, 150.0)).is_none());
    }

    #[test]
    fn tray_cells_resolve_to_the_layout_and_pane_under_the_pointer() {
        let tray = tray(AREA).unwrap();
        let inset = TRAY_BORDER + chrome::SNAP_TRAY_PAD;
        let thumbnail = |index: usize| {
            let left =
                tray.x + inset + index as f32 * (chrome::SNAP_THUMB_WIDTH + chrome::SNAP_THUMB_GAP);
            (left, tray.y + inset)
        };
        let (left, top) = thumbnail(0);
        assert_eq!(
            tray_hit((left + 2.0, top + 2.0), AREA, 0.0),
            Some(Some(DropTarget::Layout {
                cols: 1,
                rows: 1,
                cell: 0
            }))
        );
        // Two columns: the right half of the second thumbnail is cell 1.
        let (left, top) = thumbnail(1);
        let right = left + chrome::SNAP_THUMB_WIDTH - 2.0;
        assert_eq!(
            tray_hit((right, top + 2.0), AREA, 0.0),
            Some(Some(DropTarget::Layout {
                cols: 2,
                rows: 1,
                cell: 1
            }))
        );
        // 2 × 2: the lower right quarter of the last thumbnail is cell 3.
        let (left, top) = thumbnail(5);
        assert_eq!(
            tray_hit(
                (
                    left + chrome::SNAP_THUMB_WIDTH - 1.0,
                    top + chrome::SNAP_THUMB_HEIGHT - 1.0
                ),
                AREA,
                0.0
            ),
            Some(Some(DropTarget::Layout {
                cols: 2,
                rows: 2,
                cell: 3
            }))
        );
        // The gap between thumbnails is still the tray, but picks nothing.
        let (left, top) = thumbnail(1);
        assert_eq!(
            tray_hit((left - chrome::SNAP_THUMB_GAP / 2.0, top + 2.0), AREA, 0.0),
            Some(None)
        );
        assert_eq!(tray_hit((5.0, 5.0), AREA, 0.0), None);
        assert_eq!(tray_hit((tray.x + tray.width, tray.y), AREA, 0.0), None);
    }

    #[test]
    fn reach_widens_the_tray_with_padding_only() {
        let tray = tray(AREA).unwrap();
        let beside = (tray.x - 6.0, tray.y + tray.height / 2.0);
        assert_eq!(tray_hit(beside, AREA, 0.0), None);
        assert_eq!(tray_hit(beside, AREA, 8.0), Some(None));
        assert_eq!(tray_hit(beside, AREA, 4.0), None);
        let above = (tray.x + tray.width / 2.0, tray.y - 7.0);
        assert_eq!(tray_hit(above, AREA, 8.0), Some(None));
    }

    #[test]
    fn positions_are_named_for_every_layout_cell() {
        for (cols, rows) in SHAPES {
            for cell in 0..usize::from(cols) * usize::from(rows) {
                assert_ne!(position_label((cols, rows), cell), "Pane", "{cols}x{rows}");
            }
        }
        assert_eq!(position_label((2, 1), 1), "Right half");
        assert_eq!(position_label((2, 2), 2), "Bottom left");
        assert_eq!(layout_label(3, 1), "Three columns");
        assert_eq!(layout_label(9, 9), "Custom grid");
    }

    #[test]
    fn spans_accumulate_track_fractions() {
        assert_eq!(span(&[0.25, 0.75], 0), (0.0, 0.25));
        assert_eq!(span(&[0.25, 0.75], 1), (0.25, 0.75));
        assert_eq!(span(&[0.25, 0.75], 4), (1.0, 0.0));
    }

    /// A view with `count` dormant terminals in a `cols` × `rows` grid. The
    /// project folder does not exist, so no shell ever starts.
    fn snap_view(
        cx: &mut TestAppContext,
        count: usize,
        cols: u8,
        rows: u8,
    ) -> (Entity<TerminalView>, &mut VisualTestContext) {
        let (view, cx) =
            cx.add_window_view(|_, cx| TerminalView::new(PathBuf::from("synthetic-project"), cx));
        view.update(cx, |view, cx| {
            view.restore_layout(count, 0, cx);
            view.restore_grid(TerminalGrid::new(cols, rows), cx);
        });
        cx.simulate_resize(size(px(1000.0), px(700.0)));
        cx.refresh().unwrap();
        cx.run_until_parked();
        (view, cx)
    }

    fn ids(view: &Entity<TerminalView>, cx: &VisualTestContext) -> Vec<u64> {
        view.read_with(cx, |view, _| view.tab_ids())
    }

    fn target(view: &Entity<TerminalView>, cx: &VisualTestContext) -> Option<DropTarget> {
        view.read_with(cx, |view, _| {
            view.snap.as_ref().and_then(|snap| snap.target)
        })
    }

    /// Press on the tab with `selector`, then move to `to` without releasing.
    fn drag_tab(cx: &mut VisualTestContext, selector: &'static str, to: Point<Pixels>) {
        let start = cx.debug_bounds(selector).unwrap().center();
        cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_move(
            start + point(px(14.0), px(0.0)),
            MouseButton::Left,
            Modifiers::none(),
        );
        cx.simulate_mouse_move(to, MouseButton::Left, Modifiers::none());
    }

    /// Let `by` pass with the pointer at rest.
    fn wait(cx: &mut VisualTestContext, by: Duration) {
        cx.executor().advance_clock(by);
        cx.run_until_parked();
    }

    /// Rest long enough for a side zone or the tray to take over.
    fn linger(cx: &mut VisualTestContext) {
        wait(cx, chrome::SNAP_DWELL + Duration::from_millis(1));
    }

    fn drag_to(cx: &mut VisualTestContext, to: Point<Pixels>) {
        cx.simulate_mouse_move(to, MouseButton::Left, Modifiers::none());
    }

    /// A spot on the `index`th tray thumbnail, `across` pixels from its left
    /// side and `down` from its top.
    fn thumbnail_spot(body: Bounds<Pixels>, index: usize, across: f32, down: f32) -> Point<Pixels> {
        let area = (f32::from(body.size.width), f32::from(body.size.height));
        let tray = tray(area).unwrap();
        let inset = TRAY_BORDER + chrome::SNAP_TRAY_PAD;
        let left =
            tray.x + inset + index as f32 * (chrome::SNAP_THUMB_WIDTH + chrome::SNAP_THUMB_GAP);
        body.origin + point(px(left + across), px(tray.y + inset + down))
    }

    fn right_edge_spot(body: Bounds<Pixels>, inset: f32) -> Point<Pixels> {
        point(body.right() - px(inset), body.center().y)
    }

    /// A spot `from_right` pixels in from the right of the area and `from_bottom` up from its bottom.
    fn corner_spot(body: Bounds<Pixels>, from_right: f32, from_bottom: f32) -> Point<Pixels> {
        point(
            body.right() - px(from_right),
            body.bottom() - px(from_bottom),
        )
    }

    /// How deep a side zone reaches into a pane `extent` long.
    fn side_band(extent: Pixels) -> f32 {
        (f32::from(extent) * chrome::SNAP_SIDE_FRACTION)
            .clamp(chrome::SNAP_SIDE_MIN, chrome::SNAP_SIDE_MAX)
    }

    #[gpui::test]
    fn dragging_a_hidden_tab_to_the_right_border_opens_a_pane_there(cx: &mut TestAppContext) {
        let (view, cx) = snap_view(cx, 2, 1, 1);
        let ids = ids(&view, cx);
        let body = cx.debug_bounds("terminal-body").unwrap();
        let edge = point(body.right() - px(10.0), body.center().y);
        drag_tab(cx, "terminal-tab-2", edge);
        linger(cx);
        assert_eq!(
            target(&view, cx),
            Some(DropTarget::Edge {
                axis: Axis::Column,
                before: 1,
                cross: 0
            })
        );
        // The preview outlines the new layout; nothing changes until release.
        assert!(cx.debug_bounds("snap-preview").is_some());
        assert_eq!(view.read_with(cx, |view, _| view.grid.shape()), (1, 1));
        cx.simulate_mouse_up(edge, MouseButton::Left, Modifiers::none());
        cx.run_until_parked();
        view.read_with(cx, |view, _| {
            assert_eq!(view.grid.shape(), (2, 1));
            assert_eq!(view.cells, [Some(ids[0]), Some(ids[1])]);
            assert_eq!(view.tabs[view.active].id, ids[1]);
            assert!(view.snap.is_none());
        });
    }

    #[gpui::test]
    fn dragging_to_the_left_border_of_a_split_adds_a_third_column(cx: &mut TestAppContext) {
        let (view, cx) = snap_view(cx, 3, 2, 1);
        let ids = ids(&view, cx);
        let body = cx.debug_bounds("terminal-body").unwrap();
        let edge = point(body.left() + px(10.0), body.center().y);
        drag_tab(cx, "terminal-tab-3", edge);
        linger(cx);
        cx.simulate_mouse_up(edge, MouseButton::Left, Modifiers::none());
        cx.run_until_parked();
        view.read_with(cx, |view, _| {
            assert_eq!(view.grid.shape(), (3, 1));
            assert_eq!(view.cells, [Some(ids[2]), Some(ids[0]), Some(ids[1])]);
        });
    }

    #[gpui::test]
    fn the_middle_of_a_pane_still_replaces_or_swaps(cx: &mut TestAppContext) {
        let (view, cx) = snap_view(cx, 3, 2, 1);
        let ids = ids(&view, cx);
        let body = cx.debug_bounds("terminal-body").unwrap();
        let middle = point(body.left() + body.size.width * 0.75, body.center().y);
        drag_tab(cx, "terminal-tab-3", middle);
        assert_eq!(target(&view, cx), Some(DropTarget::Pane(1)));
        cx.simulate_mouse_up(middle, MouseButton::Left, Modifiers::none());
        cx.run_until_parked();
        view.read_with(cx, |view, _| {
            assert_eq!(view.grid.shape(), (2, 1));
            assert_eq!(view.cells, [Some(ids[0]), Some(ids[2])]);
            // The replaced tab is hidden, not closed.
            assert_eq!(view.tabs.len(), 3);
        });
    }

    #[gpui::test]
    fn the_only_pane_on_screen_has_nowhere_to_go_beside_itself(cx: &mut TestAppContext) {
        let (view, cx) = snap_view(cx, 2, 1, 1);
        let body = cx.debug_bounds("terminal-body").unwrap();
        let edge = point(body.right() - px(10.0), body.center().y);
        drag_tab(cx, "terminal-tab-1", edge);
        linger(cx);
        // It is already in the only pane, so there is nowhere new to put it.
        assert_eq!(target(&view, cx), None);
        cx.simulate_mouse_up(edge, MouseButton::Left, Modifiers::none());
        cx.run_until_parked();
        view.read_with(cx, |view, _| {
            assert_eq!(view.grid.shape(), (1, 1));
            assert_eq!(view.terminal_count(), 2);
        });
    }

    #[gpui::test]
    fn the_lean_of_the_pointer_picks_rows_or_columns(cx: &mut TestAppContext) {
        let (view, cx) = snap_view(cx, 2, 1, 1);
        let body = cx.debug_bounds("terminal-body").unwrap();
        // In the bottom right corner, close to the bottom: a row below.
        drag_tab(cx, "terminal-tab-2", corner_spot(body, 60.0, 20.0));
        linger(cx);
        assert_eq!(
            target(&view, cx),
            Some(DropTarget::Edge {
                axis: Axis::Row,
                before: 1,
                cross: 0
            })
        );
        // Sliding to where the right side is deeper switches to a column, with no wait.
        drag_to(cx, corner_spot(body, 20.0, 60.0));
        assert_eq!(
            target(&view, cx),
            Some(DropTarget::Edge {
                axis: Axis::Column,
                before: 1,
                cross: 0
            })
        );
    }

    #[gpui::test]
    fn a_side_between_two_panes_opens_a_column_there_from_either_pane(cx: &mut TestAppContext) {
        let (view, cx) = snap_view(cx, 3, 2, 1);
        let ids = ids(&view, cx);
        let body = cx.debug_bounds("terminal-body").unwrap();
        let gap = Some(DropTarget::Edge {
            axis: Axis::Column,
            before: 1,
            cross: 0,
        });
        let before_gap = point(body.center().x - px(10.0), body.center().y);
        let after_gap = point(body.center().x + px(10.0), body.center().y);
        drag_tab(cx, "terminal-tab-3", before_gap);
        linger(cx);
        assert_eq!(target(&view, cx), gap);
        drag_to(cx, after_gap);
        assert_eq!(target(&view, cx), gap);
        cx.simulate_mouse_up(after_gap, MouseButton::Left, Modifiers::none());
        cx.run_until_parked();
        view.read_with(cx, |view, _| {
            assert_eq!(view.grid.shape(), (3, 1));
            assert_eq!(view.cells, [Some(ids[0]), Some(ids[2]), Some(ids[1])]);
        });
    }

    #[gpui::test]
    fn dropping_a_visible_tab_below_another_stacks_them(cx: &mut TestAppContext) {
        let (view, cx) = snap_view(cx, 2, 2, 1);
        let ids = ids(&view, cx);
        let body = cx.debug_bounds("terminal-body").unwrap();
        // The lower part of the right pane, which holds the second tab.
        let below = point(
            body.left() + body.size.width * 0.75,
            body.bottom() - px(20.0),
        );
        drag_tab(cx, "terminal-tab-1", below);
        linger(cx);
        assert_eq!(
            target(&view, cx),
            Some(DropTarget::Edge {
                axis: Axis::Row,
                before: 1,
                cross: 1
            })
        );
        cx.simulate_mouse_up(below, MouseButton::Left, Modifiers::none());
        cx.run_until_parked();
        view.read_with(cx, |view, _| {
            // The vacated column closed up: no new terminal, and the first tab moved under the second.
            assert_eq!(view.grid.shape(), (1, 2));
            assert_eq!(view.cells, [Some(ids[1]), Some(ids[0])]);
            assert_eq!(view.terminal_count(), 2);
            assert_eq!(view.tabs[view.active].id, ids[0]);
        });
    }

    #[gpui::test]
    fn a_full_grid_has_no_sides_to_open_and_falls_back_to_the_pane(cx: &mut TestAppContext) {
        let (view, cx) = snap_view(cx, 4, 2, 2);
        let body = cx.debug_bounds("terminal-body").unwrap();
        let spot = point(
            body.right() - px(10.0),
            body.top() + body.size.height * 0.25,
        );
        drag_tab(cx, "terminal-tab-1", spot);
        linger(cx);
        assert_eq!(target(&view, cx), Some(DropTarget::Pane(1)));
        // Nothing is waiting to take over, since nothing could.
        assert!(view.read_with(cx, |view, _| view.snap.as_ref().unwrap().dwell.is_none()));
    }

    #[gpui::test]
    fn dropping_on_a_tray_layout_snaps_the_tab_into_that_cell(cx: &mut TestAppContext) {
        let (view, cx) = snap_view(cx, 2, 1, 1);
        let ids = ids(&view, cx);
        let body = cx.debug_bounds("terminal-body").unwrap();
        let area = (f32::from(body.size.width), f32::from(body.size.height));
        let tray = tray(area).unwrap();
        // Three columns is the fourth thumbnail; aim at its right-hand third.
        let spot = thumbnail_spot(
            body,
            3,
            chrome::SNAP_THUMB_WIDTH - 4.0,
            chrome::SNAP_THUMB_HEIGHT / 2.0,
        );
        drag_tab(cx, "terminal-tab-2", spot);
        linger(cx);
        let layout = DropTarget::Layout {
            cols: 3,
            rows: 1,
            cell: 2,
        };
        assert_eq!(target(&view, cx), Some(layout));
        // The tray is drawn where the pointer maths says it is.
        let drawn = cx.debug_bounds("snap-tray").unwrap();
        assert!((f32::from(drawn.left() - body.left()) - tray.x).abs() < 1.5);
        assert!((f32::from(drawn.top() - body.top()) - tray.y).abs() < 1.5);
        cx.simulate_mouse_up(spot, MouseButton::Left, Modifiers::none());
        cx.run_until_parked();
        view.read_with(cx, |view, _| {
            assert_eq!(view.grid.shape(), (3, 1));
            assert_eq!(view.cells[0], Some(ids[0]));
            assert_eq!(view.cells[2], Some(ids[1]));
            // The middle pane had no tab to show, so it opened a terminal.
            assert!(view.cells[1].is_some());
            assert_eq!(view.terminal_count(), 3);
        });
    }

    #[gpui::test]
    fn a_border_takes_over_only_after_the_pointer_lingers(cx: &mut TestAppContext) {
        let (view, cx) = snap_view(cx, 2, 1, 1);
        let body = cx.debug_bounds("terminal-body").unwrap();
        drag_tab(cx, "terminal-tab-2", right_edge_spot(body, 10.0));
        // Arriving is not enough: the pane underneath stays the aim.
        assert_eq!(target(&view, cx), Some(DropTarget::Pane(0)));
        wait(cx, chrome::SNAP_DWELL - Duration::from_millis(50));
        assert_eq!(target(&view, cx), Some(DropTarget::Pane(0)));
        wait(cx, Duration::from_millis(51));
        assert_eq!(
            target(&view, cx),
            Some(DropTarget::Edge {
                axis: Axis::Column,
                before: 1,
                cross: 0
            })
        );
    }

    #[gpui::test]
    fn releasing_before_the_pointer_lingers_drops_on_the_pane_underneath(cx: &mut TestAppContext) {
        let (view, cx) = snap_view(cx, 2, 1, 1);
        let ids = ids(&view, cx);
        let body = cx.debug_bounds("terminal-body").unwrap();
        let edge = right_edge_spot(body, 10.0);
        drag_tab(cx, "terminal-tab-2", edge);
        cx.simulate_mouse_up(edge, MouseButton::Left, Modifiers::none());
        cx.run_until_parked();
        view.read_with(cx, |view, _| {
            // The hidden tab took over the only pane; no pane was opened.
            assert_eq!(view.grid.shape(), (1, 1));
            assert_eq!(view.cells, [Some(ids[1])]);
            assert!(view.snap.is_none());
        });
    }

    #[gpui::test]
    fn sweeping_through_a_border_band_never_takes_over(cx: &mut TestAppContext) {
        let (view, cx) = snap_view(cx, 2, 1, 1);
        let body = cx.debug_bounds("terminal-body").unwrap();
        drag_tab(cx, "terminal-tab-2", right_edge_spot(body, 10.0));
        wait(cx, chrome::SNAP_DWELL / 2);
        drag_to(cx, right_edge_spot(body, 300.0));
        // The timer died with the zone, so waiting changes nothing.
        assert!(view.read_with(cx, |view, _| view.snap.as_ref().unwrap().dwell.is_none()));
        linger(cx);
        assert_eq!(target(&view, cx), Some(DropTarget::Pane(0)));
        // Back in the zone, the wait starts over rather than resuming.
        drag_to(cx, right_edge_spot(body, 10.0));
        wait(cx, chrome::SNAP_DWELL / 2 + Duration::from_millis(10));
        assert_eq!(target(&view, cx), Some(DropTarget::Pane(0)));
    }

    #[gpui::test]
    fn an_engaged_border_holds_a_little_past_its_band(cx: &mut TestAppContext) {
        let (view, cx) = snap_view(cx, 2, 1, 1);
        let body = cx.debug_bounds("terminal-body").unwrap();
        let edge = Some(DropTarget::Edge {
            axis: Axis::Column,
            before: 1,
            cross: 0,
        });
        // Coming from the middle, a spot just outside the zone is not an aim.
        let band = side_band(body.size.width);
        let outside = band + 8.0;
        drag_tab(cx, "terminal-tab-2", right_edge_spot(body, outside));
        linger(cx);
        assert_eq!(target(&view, cx), Some(DropTarget::Pane(0)));
        // Once engaged, the same spot is still part of the side.
        drag_to(cx, right_edge_spot(body, 10.0));
        linger(cx);
        assert_eq!(target(&view, cx), edge);
        drag_to(cx, right_edge_spot(body, outside));
        assert_eq!(target(&view, cx), edge);
        // Far enough away, the hold ends at once, with no wait.
        drag_to(cx, right_edge_spot(body, band + chrome::SNAP_HOLD + 8.0));
        assert_eq!(target(&view, cx), Some(DropTarget::Pane(0)));
        // And the side needs lingering on again.
        drag_to(cx, right_edge_spot(body, 10.0));
        assert_eq!(target(&view, cx), Some(DropTarget::Pane(0)));
    }

    #[gpui::test]
    fn an_engaged_tray_keeps_its_layout_across_the_gaps_and_switches_at_once(
        cx: &mut TestAppContext,
    ) {
        let (view, cx) = snap_view(cx, 2, 1, 1);
        let body = cx.debug_bounds("terminal-body").unwrap();
        let half = chrome::SNAP_THUMB_HEIGHT / 2.0;
        let two_columns = DropTarget::Layout {
            cols: 2,
            rows: 1,
            cell: 1,
        };
        drag_tab(
            cx,
            "terminal-tab-2",
            thumbnail_spot(body, 1, chrome::SNAP_THUMB_WIDTH - 4.0, half),
        );
        // Passing over the tray is not a choice yet.
        assert_eq!(target(&view, cx), Some(DropTarget::Pane(0)));
        linger(cx);
        assert_eq!(target(&view, cx), Some(two_columns));
        // The gap between thumbnails keeps the last layout instead of blanking the preview.
        let gap = chrome::SNAP_THUMB_WIDTH + chrome::SNAP_THUMB_GAP / 2.0;
        drag_to(cx, thumbnail_spot(body, 1, gap, half));
        assert_eq!(target(&view, cx), Some(two_columns));
        // The next thumbnail is picked immediately.
        drag_to(cx, thumbnail_spot(body, 3, 4.0, half));
        assert_eq!(
            target(&view, cx),
            Some(DropTarget::Layout {
                cols: 3,
                rows: 1,
                cell: 0
            })
        );
        // Leaving the tray entirely goes back to the pane underneath.
        drag_to(cx, body.center());
        assert_eq!(target(&view, cx), Some(DropTarget::Pane(0)));
    }

    #[gpui::test]
    fn leaving_the_pane_area_cancels_the_snap_and_a_release_there_does_nothing(
        cx: &mut TestAppContext,
    ) {
        let (view, cx) = snap_view(cx, 2, 1, 1);
        let body = cx.debug_bounds("terminal-body").unwrap();
        let inside = point(body.right() - px(10.0), body.center().y);
        drag_tab(cx, "terminal-tab-2", inside);
        assert!(view.read_with(cx, |view, _| view.snap.is_some()));
        let strip = cx.debug_bounds("terminal-tab-1").unwrap().center();
        cx.simulate_mouse_move(strip, MouseButton::Left, Modifiers::none());
        assert!(view.read_with(cx, |view, _| view.snap.is_none()));
        cx.simulate_mouse_up(strip, MouseButton::Left, Modifiers::none());
        cx.run_until_parked();
        assert_eq!(view.read_with(cx, |view, _| view.grid.shape()), (1, 1));
    }

    #[gpui::test]
    fn a_plain_click_after_an_interrupted_drag_does_not_drop(cx: &mut TestAppContext) {
        let (view, cx) = snap_view(cx, 2, 1, 1);
        let body = cx.debug_bounds("terminal-body").unwrap();
        let inside = point(body.right() - px(10.0), body.center().y);
        drag_tab(cx, "terminal-tab-2", inside);
        // The release never arrived; a move without the button ends the drag.
        cx.simulate_mouse_move(inside, None::<MouseButton>, Modifiers::none());
        assert!(view.read_with(cx, |view, _| view.snap.is_none()));
        cx.simulate_click(inside, Modifiers::none());
        cx.run_until_parked();
        assert_eq!(view.read_with(cx, |view, _| view.grid.shape()), (1, 1));
    }
}
