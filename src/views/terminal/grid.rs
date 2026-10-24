//! Pane grid for the terminal view: panes, resizable dividers, drag-and-drop,
//! and the layout picker.

use gpui::{
    AnyElement, Bounds, ClickEvent, Context, Corner, CursorStyle, DragMoveEvent, FontWeight,
    IntoElement, KeyDownEvent, MouseButton, Pixels, Render, Rgba, SharedString, Window, anchored,
    deferred, div, prelude::*, px, relative, svg,
};

use super::{MAX_TERMINAL_TABS, TabEntry, TerminalPanelEvent, TerminalView, WorkspaceTab};
use crate::services::terminal_grid::{self, Axis, Direction, SHAPES, TerminalGrid};
use crate::theme;
use crate::theme::terminal_manager as chrome;
use crate::views::terminal_manager::{check_mark, text_tooltip};

/// Labels for [`SHAPES`], in the same order.
pub(super) const SHAPE_LABELS: [&str; SHAPES.len()] = [
    "Single pane",
    "Two columns",
    "Two rows",
    "Three columns",
    "Three rows",
    "2 × 2 grid",
];
/// Share of the grid one arrow-key press moves a divider.
const KEY_STEP: f32 = 0.02;
/// Keyboard moves keep both neighbours at least this wide; drags use pixels.
const KEY_MIN_FRACTION: f32 = 0.15;

/// A tab being moved between panes, from the tab strip or a pane title bar.
#[derive(Clone)]
pub(super) struct PaneDrag {
    pub(super) tab: u64,
    label: SharedString,
    icon: &'static str,
}

impl PaneDrag {
    pub(super) fn new(tab: u64, label: &str, icon: &'static str) -> Self {
        Self {
            tab,
            label: label.to_owned().into(),
            icon,
        }
    }
}

impl Render for PaneDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
            .h(px(chrome::CONTROL_HEIGHT))
            .max_w(px(240.0))
            .px(px(chrome::INSET))
            .flex()
            .items_center()
            .gap(px(chrome::COMPACT_GAP))
            .rounded(px(chrome::CONTROL_RADIUS))
            .border_1()
            .border_color(theme::edge_hard())
            .bg(theme::panel_lift())
            .font_family(chrome::CHROME_FONT)
            .text_size(px(12.0))
            .text_color(theme::bone())
            .child(
                svg()
                    .path(self.icon)
                    .size(px(chrome::ICON_SIZE))
                    .flex_shrink_0()
                    .text_color(theme::focus()),
            )
            .child(div().min_w_0().truncate().child(self.label.clone()))
    }
}

/// A divider being dragged; `boundary` counts the tracks before it.
#[derive(Clone, Copy)]
struct DividerDrag {
    axis: Axis,
    boundary: usize,
}

/// The drag needs a view to follow the cursor, but a divider moves in place.
struct DividerGhost;

impl Render for DividerGhost {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        div()
    }
}

struct PaneSlot {
    index: usize,
    width: f32,
    right_edge: bool,
    bottom_edge: bool,
    focused: bool,
}

pub(super) fn wash(color: Rgba, alpha: f32) -> Rgba {
    Rgba { a: alpha, ..color }
}

/// Where each divider sits, as a fraction of the grid, for every track but the last.
fn boundaries(fractions: &[f32]) -> Vec<f32> {
    fractions
        .iter()
        .take(fractions.len().saturating_sub(1))
        .scan(0.0, |total, fraction| {
            *total += fraction;
            Some(*total)
        })
        .collect()
}

impl TerminalView {
    pub(crate) fn grid_snapshot(&self) -> TerminalGrid {
        self.grid.clone()
    }

    /// Apply a saved layout. Panes fill from the open tabs in tab order, so a
    /// restored grid never starts a terminal that was not saved.
    pub(crate) fn restore_grid(&mut self, grid: TerminalGrid, cx: &mut Context<Self>) {
        self.grid = grid;
        let ids = self.tab_ids();
        self.cells.clear();
        terminal_grid::reconcile(&mut self.cells, self.grid.cell_count(), &ids, None);
        let active = self.tabs.get(self.active).map(|tab| tab.id);
        if let Some(id) = active.filter(|id| !self.cells.contains(&Some(*id)))
            && let Some(last) = self.cells.last_mut()
        {
            *last = Some(id);
        }
        cx.notify();
    }

    pub(super) fn tab_ids(&self) -> Vec<u64> {
        self.tabs.iter().map(|tab| tab.id).collect()
    }

    /// Re-fit the cells to the layout and the open tabs, keeping the active tab on screen.
    pub(super) fn sync_cells(&mut self) {
        let ids = self.tab_ids();
        let active = self.tabs.get(self.active).map(|tab| tab.id);
        terminal_grid::reconcile(&mut self.cells, self.grid.cell_count(), &ids, active);
    }

    pub(super) fn is_shown(&self, id: u64) -> bool {
        self.cells.contains(&Some(id))
    }

    /// The pane holding the active tab.
    pub(super) fn focused_cell(&self) -> usize {
        self.tabs
            .get(self.active)
            .and_then(|tab| self.cells.iter().position(|cell| *cell == Some(tab.id)))
            .unwrap_or(0)
    }

    /// Put `id` on screen: in an empty pane if there is one, else in place of
    /// the focused pane's tab, which keeps running in the tab strip.
    pub(super) fn show_tab(&mut self, id: u64) {
        if self.is_shown(id) {
            return;
        }
        let slot = self
            .cells
            .iter()
            .position(Option::is_none)
            .unwrap_or_else(|| self.focused_cell());
        if let Some(cell) = self.cells.get_mut(slot) {
            *cell = Some(id);
        }
    }

    fn set_grid_shape(&mut self, cols: u8, rows: u8, window: &mut Window, cx: &mut Context<Self>) {
        self.layout_menu_open = false;
        if self.grid.set_shape(cols, rows) {
            self.sync_cells();
            self.fill_empty_panes(cx);
            cx.emit(TerminalPanelEvent::LayoutChanged);
        }
        self.focus(window, cx);
        cx.notify();
    }

    /// A new layout opens with a terminal in every pane no tab fills.
    pub(super) fn fill_empty_panes(&mut self, cx: &mut Context<Self>) {
        while let Some(slot) = self.cells.iter().position(Option::is_none) {
            let Some(id) = self.spawn_terminal(cx) else {
                break;
            };
            self.cells[slot] = Some(id);
        }
    }

    fn set_layout_menu_open(&mut self, open: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.layout_menu_open = open;
        if open {
            self.tab_list_open = false;
            self.layout_menu_selected = SHAPES
                .iter()
                .position(|shape| *shape == self.grid.shape())
                .unwrap_or(0);
            window.focus(&self.layout_menu_focus);
        } else if self.layout_menu_focus.is_focused(window) {
            self.focus(window, cx);
        }
        cx.notify();
    }

    /// Select the tab in `index` when a click lands in its pane.
    fn focus_pane(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.cells.get(index).copied().flatten() else {
            return;
        };
        if self.tabs.get(self.active).is_some_and(|tab| tab.id == id) {
            return;
        }
        self.select_tab(id, window, cx);
    }

    /// Select the pane's tab and move keyboard focus into it.
    fn activate_pane(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.cells.get(index).copied().flatten() else {
            return;
        };
        if self.tabs.get(self.active).is_some_and(|tab| tab.id == id) {
            self.focus(window, cx);
        } else {
            self.select_tab(id, window, cx);
        }
    }

    /// Swap a pane with its neighbour, keeping keyboard focus on the moved title bar.
    fn move_pane(
        &mut self,
        index: usize,
        direction: Direction,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(target) = self.grid.neighbor(index, direction) else {
            return;
        };
        let Some(tab) = self.cells.get(index).copied().flatten() else {
            return;
        };
        terminal_grid::place(&mut self.cells, target, tab);
        self.select_tab(tab, window, cx);
        if let Some(handle) = self.pane_focus.get(target) {
            window.focus(handle);
        }
    }

    fn add_terminal_to_pane(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(id) = self.spawn_terminal(cx) else {
            return;
        };
        if let Some(cell) = self.cells.get_mut(index) {
            *cell = Some(id);
        }
        self.select_tab(id, window, cx);
    }

    fn drag_divider(
        &mut self,
        drag: DividerDrag,
        event: &gpui::MouseMoveEvent,
        grid: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // A release outside the window never reaches the grid; stop on the next move.
        if event.pressed_button != Some(MouseButton::Left) {
            self.finish_resize(cx);
            return;
        }
        let (offset, extent, minimum, cursor) = match drag.axis {
            Axis::Column => (
                event.position.x - grid.left(),
                grid.size.width,
                chrome::MIN_PANE_WIDTH,
                CursorStyle::ResizeLeftRight,
            ),
            Axis::Row => (
                event.position.y - grid.top(),
                grid.size.height,
                chrome::MIN_PANE_HEIGHT,
                CursorStyle::ResizeUpDown,
            ),
        };
        cx.set_active_drag_cursor_style(cursor, window);
        let extent = f32::from(extent);
        if extent <= 0.0 {
            return;
        }
        let position = f32::from(offset) / extent;
        if self
            .grid
            .move_divider(drag.axis, drag.boundary, position, minimum / extent)
        {
            self.grid_resized = true;
            cx.notify();
        }
    }

    /// Report a finished drag once so the new sizes are saved a single time.
    fn finish_resize(&mut self, cx: &mut Context<Self>) {
        if self.grid_resized {
            self.grid_resized = false;
            cx.emit(TerminalPanelEvent::LayoutChanged);
        }
    }

    fn nudge_divider(&mut self, axis: Axis, boundary: usize, delta: f32, cx: &mut Context<Self>) {
        if self
            .grid
            .nudge_divider(axis, boundary, delta, KEY_MIN_FRACTION)
        {
            cx.emit(TerminalPanelEvent::LayoutChanged);
            cx.notify();
        }
    }

    fn equalize_panes(&mut self, axis: Axis, cx: &mut Context<Self>) {
        if self.grid.equalize(axis) {
            cx.emit(TerminalPanelEvent::LayoutChanged);
            cx.notify();
        }
    }

    pub(super) fn pane_grid(&self, entries: &[TabEntry], cx: &mut Context<Self>) -> AnyElement {
        let columns = self.grid.column_fractions();
        let rows = self.grid.row_fractions();
        let focused = self.focused_cell();
        let mut row_elements: Vec<AnyElement> = Vec::with_capacity(rows.len());
        for (row, height) in rows.iter().enumerate() {
            let mut panes: Vec<AnyElement> = Vec::with_capacity(columns.len());
            for (col, width) in columns.iter().enumerate() {
                let index = row * columns.len() + col;
                let slot = PaneSlot {
                    index,
                    width: *width,
                    right_edge: col + 1 < columns.len(),
                    bottom_edge: row + 1 < rows.len(),
                    focused: index == focused,
                };
                let entry = self.cells[index].and_then(|id| entries.iter().find(|e| e.id == id));
                let tab = self.cells[index].and_then(|id| self.tabs.iter().find(|t| t.id == id));
                panes.push(self.pane(slot, entry, tab, cx));
            }
            row_elements.push(
                div()
                    .w_full()
                    .h(relative(*height))
                    .flex()
                    .flex_row()
                    .children(panes)
                    .into_any_element(),
            );
        }
        let column_dividers = boundaries(&columns)
            .into_iter()
            .enumerate()
            .map(|(boundary, at)| self.divider(Axis::Column, boundary, at, cx))
            .collect::<Vec<_>>();
        let row_dividers = boundaries(&rows)
            .into_iter()
            .enumerate()
            .map(|(boundary, at)| self.divider(Axis::Row, boundary, at, cx))
            .collect::<Vec<_>>();
        div()
            .id("pane-grid")
            .relative()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .on_drag_move(
                cx.listener(|view, event: &DragMoveEvent<DividerDrag>, window, cx| {
                    let drag = *event.drag(cx);
                    view.drag_divider(drag, &event.event, event.bounds, window, cx);
                }),
            )
            .capture_any_mouse_up(cx.listener(|view, _, _, cx| view.finish_resize(cx)))
            .on_mouse_up_out(
                MouseButton::Left,
                cx.listener(|view, _, _, cx| view.finish_resize(cx)),
            )
            .child(
                div()
                    .absolute()
                    .top_0()
                    .left_0()
                    .size_full()
                    .flex()
                    .flex_col()
                    .children(row_elements),
            )
            .children(column_dividers)
            .children(row_dividers)
            .into_any_element()
    }

    fn pane(
        &self,
        slot: PaneSlot,
        entry: Option<&TabEntry>,
        tab: Option<&WorkspaceTab>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let PaneSlot {
            index,
            width,
            right_edge,
            bottom_edge,
            focused,
        } = slot;
        let drag = entry.map(|entry| PaneDrag::new(entry.id, &entry.label, entry.icon));
        let tooltip = match entry {
            Some(entry) => format!("{} · Drag to move · Alt+Arrows", entry.tooltip),
            None => "Empty pane".to_owned(),
        };
        let header = div()
            .id(("pane-header", index))
            .debug_selector(move || format!("pane-header-{index}"))
            .track_focus(&self.pane_focus[index])
            .tab_index(0)
            .h(px(chrome::PANE_HEADER_HEIGHT))
            .px(px(chrome::COMPACT_GAP))
            .flex_shrink_0()
            .flex()
            .items_center()
            .gap(px(chrome::COMPACT_GAP))
            .bg(if focused {
                theme::canvas()
            } else {
                theme::floor()
            })
            // Same underline the selected tab uses, so focus reads beyond color.
            .border_b_2()
            .border_color(if focused {
                theme::focus()
            } else {
                gpui::rgba(0)
            })
            .font_family(chrome::CHROME_FONT)
            .text_size(px(12.0))
            .line_height(px(chrome::CHROME_LINE_HEIGHT))
            .text_color(if focused { theme::bone() } else { theme::ash() })
            .when(entry.is_some(), |header| {
                header.cursor(CursorStyle::OpenHand)
            })
            .focus(|header| header.text_color(theme::focus()))
            .tooltip(text_tooltip(tooltip))
            .on_click(cx.listener(move |view, _, window, cx| view.activate_pane(index, window, cx)))
            .on_key_down(cx.listener(move |view, event: &KeyDownEvent, window, cx| {
                if !event.keystroke.modifiers.alt {
                    return;
                }
                let direction = match event.keystroke.key.as_str() {
                    "left" => Direction::Left,
                    "right" => Direction::Right,
                    "up" => Direction::Up,
                    "down" => Direction::Down,
                    _ => return,
                };
                view.move_pane(index, direction, window, cx);
                cx.stop_propagation();
            }))
            .when_some(drag, |header, drag| {
                header.on_drag(drag, |drag, _, _, cx| cx.new(|_| drag.clone()))
            })
            .when_some(entry, |header, entry| {
                let close_id = entry.id;
                header
                    .child(
                        svg()
                            .path(entry.icon)
                            .size(px(chrome::ICON_SIZE))
                            .flex_shrink_0()
                            .text_color(if focused {
                                theme::focus()
                            } else {
                                theme::ash()
                            }),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .truncate()
                            .child(entry.label.clone()),
                    )
                    .when_some(entry.status.clone(), |header, (status, color)| {
                        header.child(
                            div()
                                .flex_shrink_0()
                                .font_family(theme::mono())
                                .text_size(px(chrome::DETAIL_TEXT_SIZE))
                                .text_color(color)
                                .child(status),
                        )
                    })
                    .child(
                        div()
                            .id(("pane-close", index))
                            .size(px(chrome::PANE_CLOSE_SIZE))
                            .flex_shrink_0()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(px(chrome::CONTROL_RADIUS))
                            .cursor_pointer()
                            .tab_index(0)
                            .hover(|button| button.bg(theme::panel_hover()))
                            .focus(|button| button.bg(theme::panel_hover()))
                            .tooltip(text_tooltip("Close pane"))
                            // Keep the title bar from starting a drag or taking the click.
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .on_click(cx.listener(move |view, _, window, cx| {
                                cx.stop_propagation();
                                view.close_tab(close_id, window, cx);
                            }))
                            .child(
                                svg()
                                    .path("icons/close.svg")
                                    .size(px(12.0))
                                    .text_color(theme::ash()),
                            ),
                    )
            })
            .when(entry.is_none(), |header| {
                header.child(div().flex_1().min_w_0().truncate().child("Empty pane"))
            });
        let body = match tab {
            Some(tab) => div()
                .flex_1()
                .min_w_0()
                .min_h_0()
                .child(tab.content.view())
                .into_any_element(),
            None => self.empty_pane(index, cx),
        };
        div()
            .id(("pane", index))
            .debug_selector(move || format!("pane-{index}"))
            .relative()
            .w(relative(width))
            .h_full()
            .min_w_0()
            .min_h_0()
            .flex()
            .flex_col()
            .overflow_hidden()
            .border_color(theme::edge())
            .when(right_edge, |pane| pane.border_r_1())
            .when(bottom_edge, |pane| pane.border_b_1())
            // Capture phase: terminals stop mouse-down before it can bubble here.
            .capture_any_mouse_down(
                cx.listener(move |view, _, window, cx| view.focus_pane(index, window, cx)),
            )
            .child(header)
            .child(body)
            .into_any_element()
    }

    fn empty_pane(&self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let can_add = self.terminal_count() < MAX_TERMINAL_TABS;
        div()
            .flex_1()
            .min_w_0()
            .min_h_0()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(chrome::GAP))
            .bg(theme::canvas())
            .text_color(theme::ash())
            .child(
                svg()
                    .path("icons/terminal.svg")
                    .size(px(chrome::ICON_SIZE))
                    .text_color(theme::ash()),
            )
            .child(div().child("Nothing in this pane"))
            .when(can_add, |pane| {
                pane.child(
                    div()
                        .id(("pane-new-terminal", index))
                        .h(px(chrome::CONTROL_HEIGHT))
                        .px(px(chrome::INSET))
                        .flex()
                        .items_center()
                        .rounded(px(chrome::CONTROL_RADIUS))
                        .border_1()
                        .border_color(theme::edge_soft())
                        .font_family(chrome::CHROME_FONT)
                        .text_size(px(chrome::CHROME_TEXT_SIZE))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme::bone())
                        .tab_index(0)
                        .cursor_pointer()
                        .hover(|button| button.bg(theme::panel_hover()))
                        .focus(|button| button.bg(theme::panel_hover()).text_color(theme::focus()))
                        .on_click(cx.listener(move |view, _, window, cx| {
                            view.add_terminal_to_pane(index, window, cx)
                        }))
                        .child("New terminal"),
                )
            })
            .child(
                div()
                    .text_size(px(chrome::DETAIL_TEXT_SIZE))
                    .text_color(theme::smoke())
                    .child("or drag a tab here"),
            )
            .into_any_element()
    }

    fn divider(&self, axis: Axis, boundary: usize, at: f32, cx: &mut Context<Self>) -> AnyElement {
        let column = axis == Axis::Column;
        let half = chrome::DIVIDER_HIT / 2.0;
        let (before, after) = match axis {
            Axis::Column => ("left", "right"),
            Axis::Row => ("up", "down"),
        };
        div()
            .id((
                if column {
                    "column-divider"
                } else {
                    "row-divider"
                },
                boundary,
            ))
            .debug_selector(move || {
                format!(
                    "{}-divider-{boundary}",
                    if column { "column" } else { "row" }
                )
            })
            .absolute()
            .when(column, |divider| {
                divider
                    .top_0()
                    .bottom_0()
                    .left(relative(at))
                    .ml(px(-half))
                    .w(px(chrome::DIVIDER_HIT))
                    .cursor(CursorStyle::ResizeLeftRight)
            })
            .when(!column, |divider| {
                divider
                    .left_0()
                    .right_0()
                    .top(relative(at))
                    .mt(px(-half))
                    .h(px(chrome::DIVIDER_HIT))
                    .cursor(CursorStyle::ResizeUpDown)
            })
            // Blocks the terminal below so a press here never starts a text selection.
            .occlude()
            .tab_index(0)
            .hover(|divider| divider.bg(wash(theme::focus(), 0.45)))
            .active(|divider| divider.bg(wash(theme::focus(), 0.7)))
            .focus(|divider| divider.bg(theme::focus()))
            .tooltip(text_tooltip(if column {
                "Resize panes · Left/Right arrows · Home to equalize"
            } else {
                "Resize panes · Up/Down arrows · Home to equalize"
            }))
            .on_drag(DividerDrag { axis, boundary }, |_, _, _, cx| {
                cx.new(|_| DividerGhost)
            })
            .on_click(cx.listener(move |view, event: &ClickEvent, _, cx| {
                if event.click_count() == 2 {
                    view.equalize_panes(axis, cx);
                }
            }))
            .on_key_down(cx.listener(move |view, event: &KeyDownEvent, _, cx| {
                let key = event.keystroke.key.as_str();
                if key == "home" {
                    view.equalize_panes(axis, cx);
                } else if key == before {
                    view.nudge_divider(axis, boundary, -KEY_STEP, cx);
                } else if key == after {
                    view.nudge_divider(axis, boundary, KEY_STEP, cx);
                } else {
                    return;
                }
                cx.stop_propagation();
            }))
            .into_any_element()
    }

    pub(super) fn layout_picker(&self, cx: &mut Context<Self>) -> AnyElement {
        let open = self.layout_menu_open;
        let (cols, rows) = self.grid.shape();
        let label = SHAPES
            .iter()
            .position(|shape| *shape == (cols, rows))
            .map_or("Custom grid", |index| SHAPE_LABELS[index]);
        div()
            .relative()
            .flex_shrink_0()
            .on_children_prepainted(cx.processor(|view, bounds: Vec<Bounds<Pixels>>, _, _| {
                view.layout_menu_bounds = bounds.first().copied();
            }))
            .child(
                div()
                    .id("layout-picker")
                    .debug_selector(|| "layout-picker".into())
                    .size(px(chrome::TAB_LIST_BUTTON))
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded(px(chrome::CONTROL_RADIUS))
                    .border_1()
                    .border_color(gpui::rgba(0))
                    .bg(if open {
                        theme::selection()
                    } else {
                        gpui::rgba(0)
                    })
                    .tab_index(0)
                    .cursor_pointer()
                    .hover(|button| button.bg(theme::panel_hover()))
                    .focus(|button| button.border_color(theme::focus()))
                    .tooltip(text_tooltip(format!("Pane layout · {label}")))
                    .on_click(cx.listener(|view, _, window, cx| {
                        view.set_layout_menu_open(!view.layout_menu_open, window, cx)
                    }))
                    .child(layout_glyph(
                        cols,
                        rows,
                        if open { theme::bone() } else { theme::ash() },
                    )),
            )
            .when(open, |wrapper| {
                // A zero-width anchor at the trigger's right edge right-aligns the popup.
                wrapper.child(
                    div()
                        .absolute()
                        .top(px(chrome::TAB_LIST_BUTTON + chrome::MENU_GAP))
                        .right_0()
                        .child(
                            deferred(
                                anchored()
                                    .anchor(Corner::TopRight)
                                    .snap_to_window()
                                    .child(self.layout_menu(cx)),
                            )
                            .with_priority(1),
                        ),
                )
            })
            .into_any_element()
    }

    fn layout_menu(&self, cx: &mut Context<Self>) -> AnyElement {
        let current = self.grid.shape();
        div()
            .id("layout-menu")
            .debug_selector(|| "layout-menu".into())
            .track_focus(&self.layout_menu_focus)
            .tab_group()
            .occlude()
            .w(px(chrome::MENU_WIDTH))
            .p(px(chrome::MENU_INSET))
            .flex()
            .flex_col()
            .gap(px(2.0))
            .font_family(chrome::CHROME_FONT)
            .text_size(px(chrome::CONTROL_TEXT_SIZE))
            .line_height(px(chrome::CONTROL_LINE_HEIGHT))
            .text_color(theme::bone())
            .bg(theme::panel_lift())
            .border_1()
            .border_color(theme::edge_hard())
            .rounded(px(chrome::CONTROL_RADIUS))
            .on_mouse_down_out(
                cx.listener(|view, event: &gpui::MouseDownEvent, window, cx| {
                    // The trigger toggles on its own; closing here would reopen it.
                    if view
                        .layout_menu_bounds
                        .is_some_and(|bounds| bounds.contains(&event.position))
                    {
                        return;
                    }
                    view.set_layout_menu_open(false, window, cx);
                }),
            )
            .on_key_down(cx.listener(|view, event: &KeyDownEvent, window, cx| {
                let count = SHAPES.len();
                match event.keystroke.key.as_str() {
                    "up" => {
                        view.layout_menu_selected = (view.layout_menu_selected + count - 1) % count
                    }
                    "down" => view.layout_menu_selected = (view.layout_menu_selected + 1) % count,
                    "enter" | "space" => {
                        let (cols, rows) = SHAPES[view.layout_menu_selected];
                        view.set_grid_shape(cols, rows, window, cx);
                    }
                    "escape" => view.set_layout_menu_open(false, window, cx),
                    _ => return,
                }
                cx.stop_propagation();
                cx.notify();
            }))
            .children(SHAPES.into_iter().zip(SHAPE_LABELS).enumerate().map(
                |(index, ((cols, rows), label))| {
                    let selected = index == self.layout_menu_selected;
                    div()
                        .id(("layout-choice", index))
                        .debug_selector(move || format!("layout-choice-{index}"))
                        .h(px(chrome::OPTION_ROW_HEIGHT))
                        .px(px(chrome::COMPACT_GAP))
                        .flex()
                        .items_center()
                        .gap(px(chrome::COMPACT_GAP))
                        .rounded(px(chrome::CONTROL_RADIUS))
                        .border_1()
                        .border_color(if selected {
                            theme::focus()
                        } else {
                            gpui::rgba(0)
                        })
                        .when(selected, |row| row.bg(theme::selection()))
                        .cursor_pointer()
                        .hover(|row| row.bg(theme::panel_hover()))
                        .on_click(cx.listener(move |view, _, window, cx| {
                            view.set_grid_shape(cols, rows, window, cx)
                        }))
                        .child(layout_glyph(cols, rows, theme::ash()))
                        .child(div().flex_1().min_w_0().child(label))
                        .when((cols, rows) == current, |row| row.child(check_mark()))
                },
            ))
            .into_any_element()
    }
}

/// A miniature of the grid: a frame with one line per divider.
fn layout_glyph(cols: u8, rows: u8, color: Rgba) -> impl IntoElement {
    div()
        .relative()
        .w(px(chrome::LAYOUT_GLYPH_WIDTH))
        .h(px(chrome::LAYOUT_GLYPH_HEIGHT))
        .flex_shrink_0()
        .rounded(px(2.0))
        .border_1()
        .border_color(color)
        .children((1..cols).map(|col| {
            div()
                .absolute()
                .top_0()
                .bottom_0()
                .left(relative(f32::from(col) / f32::from(cols)))
                .w(px(1.0))
                .bg(color)
        }))
        .children((1..rows).map(|row| {
            div()
                .absolute()
                .left_0()
                .right_0()
                .top(relative(f32::from(row) / f32::from(rows)))
                .h(px(1.0))
                .bg(color)
        }))
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use gpui::{Entity, Modifiers, MouseMoveEvent, TestAppContext, VisualTestContext, point, size};

    use super::*;
    use crate::services::terminal_grid::DropTarget;

    /// A view with `count` dormant terminals in a `cols` × `rows` grid. The
    /// project folder does not exist, so no shell ever starts.
    fn grid_view(
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
        (view, cx)
    }

    fn shown(view: &Entity<TerminalView>, cx: &VisualTestContext) -> Vec<Option<u64>> {
        view.read_with(cx, |view, _| view.cells.clone())
    }

    fn active_id(view: &Entity<TerminalView>, cx: &VisualTestContext) -> u64 {
        view.read_with(cx, |view, _| view.tabs[view.active].id)
    }

    #[test]
    fn divider_positions_accumulate_and_skip_the_last_track() {
        assert_eq!(boundaries(&[0.5, 0.5]), [0.5]);
        assert_eq!(boundaries(&[0.25, 0.25, 0.5]), [0.25, 0.5]);
        assert!(boundaries(&[1.0]).is_empty());
        assert!(boundaries(&[]).is_empty());
    }

    #[gpui::test]
    fn restored_grids_fill_panes_in_tab_order_and_keep_the_active_tab_visible(
        cx: &mut TestAppContext,
    ) {
        let (view, cx) = grid_view(cx, 4, 2, 2);
        assert_eq!(
            shown(&view, cx),
            view.read_with(cx, |view, _| view.tab_ids())
                .into_iter()
                .map(Some)
                .collect::<Vec<_>>()
        );
        view.update(cx, |view, cx| {
            view.restore_layout(4, 3, cx);
            view.restore_grid(TerminalGrid::new(1, 2), cx);
        });
        let cells = shown(&view, cx);
        assert_eq!(cells.len(), 2);
        assert!(cells.contains(&Some(active_id(&view, cx))));
    }

    #[gpui::test]
    fn choosing_a_layout_opens_a_terminal_in_every_empty_pane(cx: &mut TestAppContext) {
        let (view, cx) = grid_view(cx, 1, 1, 1);
        let first = active_id(&view, cx);
        view.update_in(cx, |view, window, cx| {
            view.set_grid_shape(2, 2, window, cx);
            assert_eq!(view.terminal_count(), 4);
            assert!(view.cells.iter().all(Option::is_some));
            assert_eq!(view.cells[0], Some(first));
            assert_eq!(view.tabs[view.active].id, first);
            view.set_grid_shape(1, 1, window, cx);
            // Shrinking hides panes; their terminals keep running in the tab strip.
            assert_eq!(view.terminal_count(), 4);
            assert_eq!(view.cells, [Some(first)]);
            assert_eq!(view.grid_snapshot(), TerminalGrid::new(1, 1));
        });
    }

    #[gpui::test]
    fn growing_the_layout_shows_hidden_tabs_before_creating_terminals(cx: &mut TestAppContext) {
        let (view, cx) = grid_view(cx, 3, 1, 1);
        view.update_in(cx, |view, window, cx| {
            view.set_grid_shape(3, 1, window, cx);
            assert_eq!(view.terminal_count(), 3);
            assert_eq!(
                view.cells,
                view.tab_ids().into_iter().map(Some).collect::<Vec<_>>()
            );
        });
    }

    #[gpui::test]
    fn closing_a_pane_hands_its_place_to_a_hidden_tab_or_stays_empty(cx: &mut TestAppContext) {
        let (view, cx) = grid_view(cx, 3, 2, 1);
        let ids = view.read_with(cx, |view, _| view.tab_ids());
        view.update_in(cx, |view, window, cx| {
            assert_eq!(view.cells, [Some(ids[0]), Some(ids[1])]);
            view.remove_tab(ids[0], window, cx);
            // The focus moves to the other visible pane; the hidden tab fills the gap.
            assert_eq!(view.cells, [Some(ids[2]), Some(ids[1])]);
            assert_eq!(view.tabs[view.active].id, ids[1]);
            view.remove_tab(ids[2], window, cx);
            assert_eq!(view.cells, [None, Some(ids[1])]);
            assert_eq!(view.terminal_count(), 1);
            // A new tab takes the empty pane instead of replacing the focused one.
            view.add_tab(window, cx);
            assert!(view.cells.iter().all(Option::is_some));
            assert!(view.cells.contains(&Some(ids[1])));
        });
    }

    #[gpui::test]
    fn dropping_a_tab_swaps_visible_panes_and_replaces_for_hidden_ones(cx: &mut TestAppContext) {
        let (view, cx) = grid_view(cx, 3, 2, 1);
        let ids = view.read_with(cx, |view, _| view.tab_ids());
        view.update_in(cx, |view, window, cx| {
            view.apply_drop(ids[0], DropTarget::Pane(1), window, cx);
            assert_eq!(view.cells, [Some(ids[1]), Some(ids[0])]);
            assert_eq!(view.tabs[view.active].id, ids[0]);
            view.apply_drop(ids[2], DropTarget::Pane(0), window, cx);
            assert_eq!(view.cells, [Some(ids[2]), Some(ids[0])]);
            assert_eq!(view.tabs[view.active].id, ids[2]);
            // The replaced tab is hidden, not closed.
            assert_eq!(view.tabs.len(), 3);
            assert!(!view.is_shown(ids[1]));
        });
    }

    #[gpui::test]
    fn pane_clicks_and_shortcuts_move_focus_between_panes(cx: &mut TestAppContext) {
        let (view, cx) = grid_view(cx, 4, 2, 2);
        let ids = view.read_with(cx, |view, _| view.tab_ids());
        view.update_in(cx, |view, window, cx| {
            view.focus_pane(3, window, cx);
            assert_eq!(view.tabs[view.active].id, ids[3]);
            assert_eq!(view.focused_cell(), 3);
            view.cycle_tab(false, window, cx);
            assert_eq!(view.focused_cell(), 0);
            view.cycle_tab(true, window, cx);
            assert_eq!(view.focused_cell(), 3);
            // An empty pane has no tab to select.
            view.remove_tab(ids[3], window, cx);
            let before = view.active;
            view.focus_pane(3, window, cx);
            assert_eq!(view.active, before);
        });
    }

    #[gpui::test]
    fn keyboard_moves_swap_a_pane_with_its_neighbour(cx: &mut TestAppContext) {
        let (view, cx) = grid_view(cx, 4, 2, 2);
        let ids = view.read_with(cx, |view, _| view.tab_ids());
        view.update_in(cx, |view, window, cx| {
            view.move_pane(0, Direction::Right, window, cx);
            assert_eq!(view.cells[..2], [Some(ids[1]), Some(ids[0])]);
            assert_eq!(view.tabs[view.active].id, ids[0]);
            view.move_pane(1, Direction::Down, window, cx);
            assert_eq!(
                view.cells,
                [Some(ids[1]), Some(ids[3]), Some(ids[2]), Some(ids[0])]
            );
            // Edges have no neighbour: nothing moves.
            let before = view.cells.clone();
            view.move_pane(3, Direction::Right, window, cx);
            view.move_pane(3, Direction::Down, window, cx);
            assert_eq!(view.cells, before);
        });
    }

    #[gpui::test]
    fn dragging_a_divider_resizes_its_neighbours_and_reports_once(cx: &mut TestAppContext) {
        let (view, cx) = grid_view(cx, 4, 2, 2);
        let bounds = Bounds::new(point(px(100.0), px(50.0)), size(px(1000.0), px(600.0)));
        let drag = |x: f32, pressed: Option<MouseButton>| MouseMoveEvent {
            position: point(px(x), px(300.0)),
            pressed_button: pressed,
            modifiers: Modifiers::default(),
        };
        view.update_in(cx, |view, window, cx| {
            let divider = DividerDrag {
                axis: Axis::Column,
                boundary: 0,
            };
            view.drag_divider(
                divider,
                &drag(400.0, Some(MouseButton::Left)),
                bounds,
                window,
                cx,
            );
            assert_eq!(view.grid.column_fractions(), [0.3, 0.7]);
            assert_eq!(view.grid.row_fractions(), [0.5, 0.5]);
            assert!(view.grid_resized);
            // Pixel minimums keep both panes usable however far the pointer goes.
            view.drag_divider(
                divider,
                &drag(-900.0, Some(MouseButton::Left)),
                bounds,
                window,
                cx,
            );
            assert_eq!(view.grid.column_fractions(), [0.2, 0.8]);
            // A move without the button held means the release was missed.
            view.drag_divider(divider, &drag(600.0, None), bounds, window, cx);
            assert_eq!(view.grid.column_fractions(), [0.2, 0.8]);
            assert!(!view.grid_resized);
        });
    }

    #[gpui::test]
    fn panes_and_dividers_are_laid_out_from_the_saved_proportions(cx: &mut TestAppContext) {
        let (view, cx) = grid_view(cx, 4, 2, 2);
        view.update(cx, |view, cx| {
            view.grid.move_divider(Axis::Column, 0, 0.25, 0.1);
            view.grid.move_divider(Axis::Row, 0, 0.6, 0.1);
            cx.notify();
        });
        cx.simulate_resize(size(px(1000.0), px(700.0)));
        cx.refresh().unwrap();
        cx.run_until_parked();
        let near = |a: Pixels, b: Pixels| (f32::from(a) - f32::from(b)).abs() < 1.5;
        let top_left = cx.debug_bounds("pane-0").unwrap();
        let top_right = cx.debug_bounds("pane-1").unwrap();
        let bottom_left = cx.debug_bounds("pane-2").unwrap();
        let bottom_right = cx.debug_bounds("pane-3").unwrap();
        assert!(near(top_left.size.width, px(250.0)));
        assert!(near(top_right.size.width, px(750.0)));
        assert!(near(top_left.right(), top_right.left()));
        assert!(near(top_left.bottom(), bottom_left.top()));
        assert!(near(bottom_right.right(), px(1000.0)));
        let grid_height = bottom_right.bottom() - top_left.top();
        assert!(near(top_left.size.height, grid_height * 0.6));
        // Dividers straddle the shared edges and are centered on them.
        let column = cx.debug_bounds("column-divider-0").unwrap();
        let row = cx.debug_bounds("row-divider-0").unwrap();
        assert!(near(column.center().x, top_left.right()));
        assert!(near(row.center().y, top_left.bottom()));
        assert!(near(column.size.width, px(chrome::DIVIDER_HIT)));
        assert!(near(row.size.height, px(chrome::DIVIDER_HIT)));
    }

    #[gpui::test]
    fn the_layout_picker_changes_the_grid_from_the_header(cx: &mut TestAppContext) {
        let (view, cx) = grid_view(cx, 1, 1, 1);
        cx.simulate_resize(size(px(1000.0), px(700.0)));
        cx.refresh().unwrap();
        cx.run_until_parked();
        // A single pane renders exactly like the classic tab view.
        assert!(cx.debug_bounds("pane-0").is_none());
        let picker = cx.debug_bounds("layout-picker").unwrap();
        cx.simulate_click(picker.center(), Modifiers::none());
        assert!(view.read_with(cx, |view, _| view.layout_menu_open));
        let choice = cx.debug_bounds("layout-choice-5").unwrap();
        cx.simulate_click(choice.center(), Modifiers::none());
        cx.run_until_parked();
        view.read_with(cx, |view, _| {
            assert!(!view.layout_menu_open);
            assert_eq!(view.grid.shape(), (2, 2));
            assert_eq!(view.terminal_count(), 4);
        });
        for selector in ["pane-0", "pane-1", "pane-2", "pane-3"] {
            assert!(cx.debug_bounds(selector).is_some(), "{selector}");
        }
        let picker = cx.debug_bounds("layout-picker").unwrap();
        cx.simulate_click(picker.center(), Modifiers::none());
        let single = cx.debug_bounds("layout-choice-0").unwrap();
        cx.simulate_click(single.center(), Modifiers::none());
        // `debug_bounds` keeps entries from earlier frames, so only state can show
        // that the extra panes are gone; the tabs behind them keep running.
        view.read_with(cx, |view, _| {
            assert!(!view.layout_menu_open);
            assert_eq!(view.grid, TerminalGrid::default());
            assert_eq!(view.cells.len(), 1);
            assert_eq!(view.terminal_count(), 4);
        });
    }

    #[gpui::test]
    fn the_layout_menu_works_from_the_keyboard(cx: &mut TestAppContext) {
        let (view, cx) = grid_view(cx, 1, 1, 1);
        cx.simulate_resize(size(px(1000.0), px(700.0)));
        cx.refresh().unwrap();
        cx.run_until_parked();
        view.update_in(cx, |view, window, cx| {
            view.set_layout_menu_open(true, window, cx)
        });
        cx.run_until_parked();
        assert!(view.read_with(cx, |view, _| view.layout_menu_selected == 0));
        cx.simulate_keystrokes("down");
        cx.simulate_keystrokes("enter");
        cx.run_until_parked();
        view.read_with(cx, |view, _| {
            assert!(!view.layout_menu_open);
            assert_eq!(view.grid.shape(), (2, 1));
            assert_eq!(view.terminal_count(), 2);
        });
        view.update_in(cx, |view, window, cx| {
            view.set_layout_menu_open(true, window, cx)
        });
        cx.run_until_parked();
        cx.simulate_keystrokes("escape");
        assert!(!view.read_with(cx, |view, _| view.layout_menu_open));
    }

    #[gpui::test]
    fn dragging_a_pane_title_onto_another_pane_swaps_them(cx: &mut TestAppContext) {
        let (view, cx) = grid_view(cx, 4, 2, 2);
        let ids = view.read_with(cx, |view, _| view.tab_ids());
        cx.simulate_resize(size(px(1000.0), px(700.0)));
        cx.refresh().unwrap();
        cx.run_until_parked();
        let header = cx.debug_bounds("pane-header-0").unwrap();
        let target = cx.debug_bounds("pane-3").unwrap();
        let start = point(header.left() + px(60.0), header.center().y);
        cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_move(
            start + point(px(12.0), px(0.0)),
            MouseButton::Left,
            Modifiers::none(),
        );
        cx.simulate_mouse_move(target.center(), MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_up(target.center(), MouseButton::Left, Modifiers::none());
        cx.run_until_parked();
        view.read_with(cx, |view, _| {
            assert_eq!(
                view.cells,
                [Some(ids[3]), Some(ids[1]), Some(ids[2]), Some(ids[0])]
            );
            assert_eq!(view.tabs[view.active].id, ids[0]);
        });
    }

    #[gpui::test]
    fn dragging_a_divider_with_the_pointer_resizes_and_saves_once(cx: &mut TestAppContext) {
        let (view, cx) = grid_view(cx, 2, 2, 1);
        cx.simulate_resize(size(px(1000.0), px(700.0)));
        cx.refresh().unwrap();
        cx.run_until_parked();
        let divider = cx.debug_bounds("column-divider-0").unwrap();
        let start = divider.center();
        cx.simulate_mouse_down(start, MouseButton::Left, Modifiers::none());
        cx.simulate_mouse_move(
            start + point(px(10.0), px(0.0)),
            MouseButton::Left,
            Modifiers::none(),
        );
        cx.simulate_mouse_move(
            point(px(700.0), start.y),
            MouseButton::Left,
            Modifiers::none(),
        );
        cx.run_until_parked();
        let fractions = view.read_with(cx, |view, _| view.grid.column_fractions());
        assert!((fractions[0] - 0.7).abs() < 0.01, "{fractions:?}");
        assert!(view.read_with(cx, |view, _| view.grid_resized));
        cx.simulate_mouse_up(
            point(px(700.0), start.y),
            MouseButton::Left,
            Modifiers::none(),
        );
        cx.run_until_parked();
        assert!(!view.read_with(cx, |view, _| view.grid_resized));
    }

    #[gpui::test]
    fn divider_keys_nudge_and_equalize_one_axis(cx: &mut TestAppContext) {
        let (view, cx) = grid_view(cx, 4, 2, 2);
        view.update(cx, |view, cx| {
            view.nudge_divider(Axis::Row, 0, KEY_STEP, cx);
            assert_eq!(view.grid.row_fractions(), [0.52, 0.48]);
            assert_eq!(view.grid.column_fractions(), [0.5, 0.5]);
            view.equalize_panes(Axis::Row, cx);
            assert_eq!(view.grid.row_fractions(), [0.5, 0.5]);
        });
    }
}
