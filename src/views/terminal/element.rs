use gpui::{
    App, Bounds, ContentMask, DispatchPhase, Element, ElementId, ElementInputHandler, Entity,
    FontStyle, FontWeight, GlobalElementId, HighlightStyle, InspectorElementId, IntoElement,
    LayoutId, MouseMoveEvent, MouseUpEvent, Pixels, Point, ShapedLine, StrikethroughStyle, Style,
    TextStyle, UnderlineStyle, Window, fill, point, px, relative, size,
};

use super::{TERMINAL_LINE_HEIGHT, TerminalSession};
use crate::services::terminal::TerminalSize;
use crate::services::terminal_engine::{
    TerminalCell, TerminalColor, TerminalCursorShape, xterm_color,
};
use crate::theme;

pub(super) struct TerminalElement {
    pub(super) session: Entity<TerminalSession>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct TerminalGeometry {
    pub(super) bounds: Bounds<Pixels>,
    pub(super) cell_width: Pixels,
    pub(super) cell_height: Pixels,
    pub(super) cursor_bounds: Bounds<Pixels>,
}

impl TerminalGeometry {
    pub(super) fn cell_at(
        &self,
        position: Point<Pixels>,
        terminal_size: TerminalSize,
    ) -> (usize, usize, bool) {
        let x = (position.x - self.bounds.left()) / self.cell_width;
        let y = (position.y - self.bounds.top()) / self.cell_height;
        let col = (x.floor() as isize)
            .clamp(0, isize::try_from(terminal_size.cols - 1).unwrap_or(0))
            as usize;
        let row = (y.floor() as isize)
            .clamp(0, isize::try_from(terminal_size.rows - 1).unwrap_or(0))
            as usize;
        let right_half = x >= f32::from(terminal_size.cols) || (x > 0.0 && x.fract() >= 0.5);
        (row, col, right_half)
    }

    fn terminal_size(&self) -> TerminalSize {
        TerminalSize::new(
            (self.bounds.size.height / self.cell_height)
                .floor()
                .max(1.0) as u16,
            (self.bounds.size.width / self.cell_width).floor().max(2.0) as u16,
        )
    }
}

struct PaintedCell {
    bounds: Bounds<Pixels>,
    background: gpui::Rgba,
    line: Option<ShapedLine>,
}

pub(super) struct PrepaintState {
    geometry: TerminalGeometry,
    cells: Vec<PaintedCell>,
    cursor: Vec<gpui::PaintQuad>,
    composition: Option<(Point<Pixels>, ShapedLine)>,
    cursor_blinking: bool,
    baseline: Pixels,
}

impl IntoElement for TerminalElement {
    type Element = Self;
    fn into_element(self) -> Self {
        self
    }
}

impl Element for TerminalElement {
    type RequestLayoutState = ();
    type PrepaintState = PrepaintState;

    fn id(&self) -> Option<ElementId> {
        Some("terminal-grid".into())
    }
    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let mut style = Style::default();
        style.size.width = relative(1.0).into();
        style.size.height = relative(1.0).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) -> PrepaintState {
        let session = self.session.read(cx);
        let snapshot = session.engine.snapshot();
        let font_size = theme::text_size(theme::T_MONO).to_pixels(window.rem_size());
        // Measure through the same shaping path and explicit style used for
        // cells, including platform font fallback. Chrome styles must not
        // change the terminal's advance or weight.
        let default_style = TextStyle {
            font_family: theme::mono(),
            font_size: font_size.into(),
            ..TextStyle::default()
        };
        let metrics = window.text_system().shape_line(
            "M".into(),
            font_size,
            &[default_style.to_run(1)],
            None,
        );
        let cell_width = metrics.width.max(px(1.0));
        let cell_height = (font_size * (TERMINAL_LINE_HEIGHT / theme::T_MONO))
            .max(metrics.ascent + metrics.descent)
            .ceil();
        let baseline = (cell_height + metrics.ascent - metrics.descent) / 2.0;
        let cursor_cell = snapshot.cursor.unwrap_or((0, 0));
        let cursor_width = snapshot
            .rows
            .get(cursor_cell.0)
            .and_then(|row| row.get(cursor_cell.1))
            .map_or(cell_width, |cell| {
                if cell.wide {
                    cell_width * 2.0
                } else {
                    cell_width
                }
            });
        let cursor_bounds = Bounds::new(
            point(
                bounds.left() + cell_width * cursor_cell.1,
                bounds.top() + cell_height * cursor_cell.0,
            ),
            size(cursor_width, cell_height),
        );
        let geometry = TerminalGeometry {
            bounds,
            cell_width,
            cell_height,
            cursor_bounds,
        };
        let focused = session.focus_handle.is_focused(window);
        let cursor_visible = !snapshot.cursor_blinking || session.cursor_visible;
        let mut cells = Vec::new();
        for (row, line) in snapshot.rows.iter().enumerate() {
            if cell_height * row >= bounds.size.height {
                break;
            }
            for (col, cell) in line.iter().enumerate() {
                if cell_width * col >= bounds.size.width {
                    break;
                }
                let origin = point(
                    bounds.left() + cell_width * col,
                    bounds.top() + cell_height * row,
                );
                let (mut foreground, mut background) = cell_colors(cell);
                let at_cursor = snapshot.cursor == Some((row, col))
                    || (cell.wide_spacer && col > 0 && snapshot.cursor == Some((row, col - 1)));
                let block_cursor = focused
                    && cursor_visible
                    && at_cursor
                    && snapshot.cursor_shape == TerminalCursorShape::Block;
                if block_cursor {
                    foreground = theme::canvas();
                    background = theme::focus();
                }
                let cell_bounds = Bounds::new(origin, size(cell_width, cell_height));
                let shaped = if cell.wide_spacer
                    || (cell.text.trim().is_empty() && !cell.underline && !cell.strikethrough)
                {
                    None
                } else {
                    let style = styled_cell(cell, &default_style, foreground);
                    let line = window.text_system().shape_line(
                        cell.text.clone().into(),
                        font_size,
                        &[style.to_run(cell.text.len())],
                        None,
                    );
                    Some(line)
                };
                cells.push(PaintedCell {
                    bounds: cell_bounds,
                    background,
                    line: shaped,
                });
            }
        }
        let mut cursor = Vec::new();
        if snapshot.cursor.is_some() && (!focused || cursor_visible) {
            if focused {
                match snapshot.cursor_shape {
                    TerminalCursorShape::Block => {}
                    TerminalCursorShape::Beam => cursor.push(fill(
                        Bounds::new(cursor_bounds.origin, size(px(2.0), cell_height)),
                        theme::focus(),
                    )),
                    TerminalCursorShape::Underline => cursor.push(fill(
                        Bounds::new(
                            point(cursor_bounds.left(), cursor_bounds.bottom() - px(2.0)),
                            size(cursor_width, px(2.0)),
                        ),
                        theme::focus(),
                    )),
                }
            } else {
                for border in [
                    Bounds::new(cursor_bounds.origin, size(cursor_width, px(1.0))),
                    Bounds::new(
                        point(cursor_bounds.left(), cursor_bounds.bottom() - px(1.0)),
                        size(cursor_width, px(1.0)),
                    ),
                    Bounds::new(cursor_bounds.origin, size(px(1.0), cell_height)),
                    Bounds::new(
                        point(cursor_bounds.right() - px(1.0), cursor_bounds.top()),
                        size(px(1.0), cell_height),
                    ),
                ] {
                    cursor.push(fill(border, theme::ash()));
                }
            }
        }
        let composition = if focused && !session.composition.is_empty() {
            let text = session.composition.replace(['\n', '\r'], " ");
            let style = default_style.clone().highlight(HighlightStyle {
                color: Some(theme::bone().into()),
                background_color: Some(theme::panel_lift().into()),
                underline: Some(UnderlineStyle {
                    thickness: px(1.0),
                    color: Some(theme::focus().into()),
                    ..Default::default()
                }),
                ..Default::default()
            });
            Some((
                cursor_bounds.origin,
                window.text_system().shape_line(
                    text.clone().into(),
                    font_size,
                    &[style.to_run(text.len())],
                    None,
                ),
            ))
        } else {
            None
        };
        PrepaintState {
            geometry,
            cells,
            cursor,
            composition,
            cursor_blinking: snapshot.cursor_blinking,
            baseline,
        }
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _: &mut (),
        prepaint: &mut PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus_handle = self.session.read(cx).focus_handle.clone();
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.session.clone()),
            cx,
        );
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            // Paint all cell backgrounds first so the second half of a wide
            // glyph cannot erase the glyph painted in its leading cell.
            for cell in &prepaint.cells {
                window.paint_quad(fill(cell.bounds, cell.background));
            }
            for cell in &prepaint.cells {
                if let Some(line) = &cell.line {
                    // Fallback glyphs may have different ascent/descent. Keep
                    // every cell on the primary font's baseline.
                    let line_baseline =
                        (prepaint.geometry.cell_height + line.ascent - line.descent) / 2.0;
                    let _ = line.paint(
                        cell.bounds.origin + point(px(0.0), prepaint.baseline - line_baseline),
                        prepaint.geometry.cell_height,
                        window,
                        cx,
                    );
                }
            }
            for cursor in prepaint.cursor.drain(..) {
                window.paint_quad(cursor);
            }
            if let Some((origin, line)) = &prepaint.composition {
                let _ = line.paint_background(*origin, prepaint.geometry.cell_height, window, cx);
                let _ = line.paint(*origin, prepaint.geometry.cell_height, window, cx);
            }
        });
        let geometry = prepaint.geometry;
        let geometry_changed = self.session.read(cx).geometry != Some(geometry);
        self.session.update(cx, |session, _| {
            session.geometry = Some(geometry);
            session.cursor_blinking = prepaint.cursor_blinking;
        });
        if geometry_changed {
            let session = self.session.downgrade();
            cx.defer(move |cx| {
                let _ = session.update(cx, |session, cx| {
                    // An old frame must never resize a newly selected layout.
                    if session.geometry != Some(geometry) {
                        return;
                    }
                    session.engine.set_cell_size(
                        f32::from(geometry.cell_width).round().max(1.0) as u16,
                        f32::from(geometry.cell_height).round().max(1.0) as u16,
                    );
                    session.resize(geometry.terminal_size(), cx);
                });
            });
        }
        let session = self.session.clone();
        window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble {
                return;
            }
            let view = session.read(cx);
            if bounds.contains(&event.position) || view.selecting || view.pressed_button.is_some() {
                session.update(cx, |session, cx| session.on_mouse_move(event, window, cx));
            }
        });
        let session = self.session.clone();
        window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
            if phase != DispatchPhase::Bubble {
                return;
            }
            let view = session.read(cx);
            if view.selecting || view.pressed_button.is_some() {
                session.update(cx, |session, cx| session.on_mouse_up(event, window, cx));
            }
        });
    }
}

fn styled_cell(
    cell: &TerminalCell,
    default_style: &TextStyle,
    foreground: gpui::Rgba,
) -> TextStyle {
    default_style.clone().highlight(HighlightStyle {
        color: Some(foreground.into()),
        font_weight: cell.bold.then_some(FontWeight::BOLD),
        font_style: cell.italic.then_some(FontStyle::Italic),
        underline: cell.underline.then_some(UnderlineStyle {
            thickness: px(1.0),
            color: Some(foreground.into()),
            ..Default::default()
        }),
        strikethrough: cell.strikethrough.then_some(StrikethroughStyle {
            thickness: px(1.0),
            color: Some(foreground.into()),
        }),
        ..Default::default()
    })
}

fn cell_colors(cell: &TerminalCell) -> (gpui::Rgba, gpui::Rgba) {
    if cell.selected {
        return (theme::canvas(), theme::focus());
    }
    let (foreground, background) = if cell.inverse {
        (cell.bg, cell.fg)
    } else {
        (cell.fg, cell.bg)
    };
    let mut foreground = terminal_color(foreground);
    if cell.dim {
        foreground.a *= 0.65;
    }
    (foreground, terminal_color(background))
}

fn terminal_color(color: TerminalColor) -> gpui::Rgba {
    let (red, green, blue) = match color {
        TerminalColor::DefaultForeground => return theme::bone_dim(),
        TerminalColor::DefaultBackground => return theme::canvas(),
        TerminalColor::Indexed(index) => {
            if let Some(color) = theme::terminal_ansi(index) {
                return color;
            }
            xterm_color(index)
        }
        TerminalColor::Rgb(red, green, blue) => (red, green, blue),
    };
    gpui::rgb((u32::from(red) << 16) | (u32::from(green) << 8) | u32::from(blue))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn appearance_preserves_explicit_rgb_and_extended_ansi_colors() {
        struct RestoreAppearance(theme::Appearance);
        impl Drop for RestoreAppearance {
            fn drop(&mut self) {
                theme::set_appearance(self.0);
            }
        }
        let _restore = RestoreAppearance(theme::appearance());
        for appearance in theme::Appearance::ALL {
            theme::set_appearance(appearance);
            assert_eq!(
                terminal_color(TerminalColor::Rgb(0x12, 0x34, 0x56)),
                gpui::rgb(0x123456)
            );
            for (index, expected) in [
                (16, 0x000000),
                (21, 0x0000ff),
                (231, 0xffffff),
                (244, 0x808080),
                (255, 0xeeeeee),
            ] {
                assert_eq!(
                    terminal_color(TerminalColor::Indexed(index)),
                    gpui::rgb(expected)
                );
            }
        }
    }

    #[test]
    fn mouse_cells_use_content_origin_measured_advance_and_clamped_edges() {
        let bounds = Bounds::new(point(px(252.0), px(96.0)), size(px(800.0), px(360.0)));
        let geometry = TerminalGeometry {
            bounds,
            cell_width: px(8.0),
            cell_height: px(18.0),
            cursor_bounds: Bounds::new(bounds.origin, size(px(8.0), px(18.0))),
        };
        let terminal = TerminalSize::new(20, 100);
        assert_eq!(
            geometry.cell_at(point(px(270.0), px(133.0)), terminal),
            (2, 2, false)
        );
        assert_eq!(
            geometry.cell_at(point(px(274.0), px(133.0)), terminal),
            (2, 2, true)
        );
        assert_eq!(
            geometry.cell_at(point(px(0.0), px(0.0)), terminal),
            (0, 0, false)
        );
        assert_eq!(
            geometry.cell_at(point(px(2000.0), px(2000.0)), terminal),
            (19, 99, true)
        );
        assert_eq!(geometry.terminal_size(), terminal);
    }
}
