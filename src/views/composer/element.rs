use std::ops::Range;

use gpui::{
    App, Bounds, ContentMask, Element, ElementId, ElementInputHandler, Entity, GlobalElementId,
    InspectorElementId, IntoElement, LayoutId, PaintQuad, Pixels, Point, Style, TextAlign, TextRun,
    UnderlineStyle, Window, WrappedLine, fill, point, px, relative, size,
};

use super::Composer;
use crate::theme;

const SCROLLBAR_GUTTER: f32 = 10.0;
const CARET_WIDTH: f32 = 2.0;

pub(super) struct ComposerTextElement {
    pub(super) input: Entity<Composer>,
}

#[derive(Clone)]
pub(super) struct EditorLayout {
    pub(super) bounds: Bounds<Pixels>,
    pub(super) line_height: Pixels,
    pub(super) scroll_y: Pixels,
    pub(super) content_height: Pixels,
    pub(super) content_len: usize,
    hard_lines: Vec<HardLine>,
    rows: Vec<VisualRow>,
}

#[derive(Clone)]
struct HardLine {
    start: usize,
    y: Pixels,
    line: WrappedLine,
}

#[derive(Debug, Clone)]
struct VisualRow {
    start: usize,
    end: usize,
    y: Pixels,
    hard_line: usize,
    soft_wrap: bool,
}

pub(super) struct PrepaintState {
    layout: EditorLayout,
    cursor: Option<PaintQuad>,
    selection: Vec<PaintQuad>,
    scrollbar: Option<PaintQuad>,
}

impl EditorLayout {
    pub(super) fn index_for_position(&self, position: Point<Pixels>) -> usize {
        if self.content_len == 0 || self.rows.is_empty() {
            return 0;
        }
        let content_y = position.y - self.bounds.top() + self.scroll_y;
        let row_index = ((content_y / self.line_height).floor() as isize)
            .clamp(0, self.rows.len().saturating_sub(1) as isize) as usize;
        let row = &self.rows[row_index];
        let hard_line = &self.hard_lines[row.hard_line];
        let local_position = point(
            position.x - self.bounds.left(),
            (content_y - hard_line.y).max(Pixels::ZERO),
        );
        let local_index = hard_line
            .line
            .closest_index_for_position(local_position, self.line_height)
            .unwrap_or_else(|index| index);
        (hard_line.start + local_index).min(self.content_len)
    }

    pub(super) fn position_for_offset(&self, offset: usize) -> Point<Pixels> {
        let Some(row) = self.row_for_offset(offset.min(self.content_len)) else {
            return self.bounds.origin;
        };
        point(
            self.bounds.left() + self.x_for_offset(row, offset),
            self.bounds.top() + row.y - self.scroll_y,
        )
    }

    pub(super) fn visual_line_range(&self, offset: usize) -> Range<usize> {
        self.row_for_offset(offset.min(self.content_len))
            .map_or(0..self.content_len, |row| row.start..row.end)
    }

    pub(super) fn offset_for_vertical_move(
        &self,
        offset: usize,
        preferred_x: Pixels,
        direction: isize,
    ) -> usize {
        let Some(current_index) = self.row_index_for_offset(offset.min(self.content_len)) else {
            return offset.min(self.content_len);
        };
        let target_index = (current_index as isize + direction)
            .clamp(0, self.rows.len().saturating_sub(1) as isize)
            as usize;
        let target = &self.rows[target_index];
        let position = point(
            self.bounds.left() + preferred_x,
            self.bounds.top() + target.y - self.scroll_y + self.line_height / 2.0,
        );
        self.index_for_position(position)
    }

    fn row_for_offset(&self, offset: usize) -> Option<&VisualRow> {
        self.row_index_for_offset(offset)
            .and_then(|index| self.rows.get(index))
    }

    fn row_index_for_offset(&self, offset: usize) -> Option<usize> {
        self.rows
            .iter()
            .enumerate()
            .find_map(|(index, row)| {
                (offset < row.end || (offset == row.end && !row.soft_wrap)).then_some(index)
            })
            .or_else(|| self.rows.len().checked_sub(1))
    }

    fn x_for_offset(&self, row: &VisualRow, offset: usize) -> Pixels {
        if offset <= row.start {
            return Pixels::ZERO;
        }
        let hard_line = &self.hard_lines[row.hard_line];
        let local_offset = offset
            .min(row.end)
            .saturating_sub(hard_line.start)
            .min(hard_line.line.len());
        hard_line
            .line
            .position_for_index(local_offset, self.line_height)
            .map_or(Pixels::ZERO, |position| position.x)
    }
}

impl IntoElement for ComposerTextElement {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for ComposerTextElement {
    type RequestLayoutState = ();
    type PrepaintState = PrepaintState;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        let mut style = Style::default();
        style.size.width = relative(1.0).into();
        style.size.height = relative(1.0).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let input = self.input.read(cx);
        let text = input.buffer.text().to_owned();
        let placeholder = input.placeholder.clone();
        let selection = input.buffer.selection().clone();
        let marked_range = input.buffer.marked_range().cloned();
        let cursor_offset = input.buffer.cursor();
        let disabled = input.disabled;
        let mut scroll_y = input.scroll_y;
        let reveal_cursor = input.reveal_cursor;
        let focus_handle = input.focus_handle.clone();

        let text_style = window.text_style();
        let line_height = window.line_height();
        let font_size = text_style.font_size.to_pixels(window.rem_size());
        let wrap_width = (bounds.size.width - px(SCROLLBAR_GUTTER)).max(px(24.0));
        let display_placeholder = text.is_empty();
        let display_text = if display_placeholder {
            placeholder.to_string()
        } else {
            text.clone()
        };

        let mut hard_lines = Vec::new();
        let mut rows = Vec::new();
        let mut hard_start = 0;
        let mut y = Pixels::ZERO;
        for hard_text in display_text.split('\n') {
            let runs = text_runs(
                hard_text,
                hard_start,
                marked_range.as_ref(),
                &text_style,
                display_placeholder,
                disabled,
            );
            let shaped = window
                .text_system()
                .shape_text(
                    hard_text.to_owned().into(),
                    font_size,
                    &runs,
                    Some(wrap_width),
                    None,
                )
                .unwrap_or_default();
            let line = shaped.into_iter().next().unwrap_or_default();
            let hard_line_index = hard_lines.len();
            let mut starts = vec![0];
            for boundary in line.wrap_boundaries() {
                if let Some(run) = line.runs().get(boundary.run_ix)
                    && let Some(glyph) = run.glyphs.get(boundary.glyph_ix)
                    && starts.last() != Some(&glyph.index)
                {
                    starts.push(glyph.index);
                }
            }
            let visual_count = starts.len();
            for (row_index, start) in starts.iter().copied().enumerate() {
                let end = starts.get(row_index + 1).copied().unwrap_or(line.len());
                rows.push(VisualRow {
                    start: if display_placeholder {
                        0
                    } else {
                        hard_start + start
                    },
                    end: if display_placeholder {
                        0
                    } else {
                        hard_start + end
                    },
                    y: y + line_height * row_index,
                    hard_line: hard_line_index,
                    soft_wrap: row_index + 1 < visual_count,
                });
            }
            hard_lines.push(HardLine {
                start: if display_placeholder { 0 } else { hard_start },
                y,
                line,
            });
            y += line_height * visual_count.max(1);
            hard_start = hard_start.saturating_add(hard_text.len()).saturating_add(1);
        }

        let content_height = y.max(line_height);
        let max_scroll = (content_height - bounds.size.height).max(Pixels::ZERO);
        scroll_y = scroll_y.clamp(Pixels::ZERO, max_scroll);
        let mut layout = EditorLayout {
            bounds,
            line_height,
            scroll_y,
            content_height,
            content_len: text.len(),
            hard_lines,
            rows,
        };

        if reveal_cursor {
            let cursor = layout.position_for_offset(cursor_offset);
            let cursor_top = cursor.y - bounds.top() + scroll_y;
            if cursor_top < scroll_y {
                scroll_y = cursor_top.max(Pixels::ZERO);
            } else if cursor_top + line_height > scroll_y + bounds.size.height {
                scroll_y = (cursor_top + line_height - bounds.size.height).min(max_scroll);
            }
            layout.scroll_y = scroll_y;
        }

        let selection_quads = selection_quads(&layout, &selection);
        let cursor = if selection.is_empty() && focus_handle.is_focused(window) && !disabled {
            let position = layout.position_for_offset(cursor_offset);
            Some(fill(
                Bounds::new(position, size(px(CARET_WIDTH), line_height)),
                theme::focus(),
            ))
        } else {
            None
        };
        let scrollbar = scrollbar_quad(&layout, bounds);

        PrepaintState {
            layout,
            cursor,
            selection: selection_quads,
            scrollbar,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let focus_handle = self.input.read(cx).focus_handle.clone();
        window.handle_input(
            &focus_handle,
            ElementInputHandler::new(bounds, self.input.clone()),
            cx,
        );

        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            for selection in prepaint.selection.drain(..) {
                window.paint_quad(selection);
            }
            for hard_line in &prepaint.layout.hard_lines {
                let origin = point(
                    bounds.left(),
                    bounds.top() + hard_line.y - prepaint.layout.scroll_y,
                );
                let _ = hard_line.line.paint(
                    origin,
                    prepaint.layout.line_height,
                    TextAlign::Left,
                    Some(bounds),
                    window,
                    cx,
                );
            }
            if let Some(cursor) = prepaint.cursor.take() {
                window.paint_quad(cursor);
            }
            if let Some(scrollbar) = prepaint.scrollbar.take() {
                window.paint_quad(scrollbar);
            }
        });

        let layout = prepaint.layout.clone();
        self.input.update(cx, |input, _| {
            input.scroll_y = layout.scroll_y;
            input.last_layout = Some(layout);
            input.reveal_cursor = false;
        });
    }
}

fn text_runs(
    text: &str,
    global_start: usize,
    marked: Option<&Range<usize>>,
    style: &gpui::TextStyle,
    placeholder: bool,
    disabled: bool,
) -> Vec<TextRun> {
    let color = if placeholder || disabled {
        theme::smoke().into()
    } else {
        style.color
    };
    let base = TextRun {
        len: text.len(),
        font: style.font(),
        color,
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let Some(marked) = marked else {
        return vec![base];
    };
    let line_end = global_start + text.len();
    let start = marked.start.clamp(global_start, line_end) - global_start;
    let end = marked.end.clamp(global_start, line_end) - global_start;
    if start >= end {
        return vec![base];
    }
    [
        TextRun {
            len: start,
            ..base.clone()
        },
        TextRun {
            len: end - start,
            underline: Some(UnderlineStyle {
                color: Some(color),
                thickness: px(1.0),
                wavy: false,
            }),
            ..base.clone()
        },
        TextRun {
            len: text.len() - end,
            ..base
        },
    ]
    .into_iter()
    .filter(|run| run.len > 0)
    .collect()
}

fn selection_quads(layout: &EditorLayout, selection: &Range<usize>) -> Vec<PaintQuad> {
    if selection.is_empty() {
        return Vec::new();
    }
    let text_right = layout.bounds.right() - px(SCROLLBAR_GUTTER);
    layout
        .rows
        .iter()
        .filter_map(|row| {
            let start = selection.start.max(row.start);
            let end = selection.end.min(row.end);
            let includes_line_break = selection.start <= row.end && selection.end > row.end;
            if start >= end && !includes_line_break {
                return None;
            }
            let left = layout.bounds.left() + layout.x_for_offset(row, start);
            let right = if includes_line_break {
                text_right
            } else {
                layout.bounds.left() + layout.x_for_offset(row, end)
            };
            Some(fill(
                Bounds::from_corners(
                    point(left, layout.bounds.top() + row.y - layout.scroll_y),
                    point(
                        right.max(left + px(2.0)),
                        layout.bounds.top() + row.y - layout.scroll_y + layout.line_height,
                    ),
                ),
                theme::data_wash(),
            ))
        })
        .collect()
}

fn scrollbar_quad(layout: &EditorLayout, bounds: Bounds<Pixels>) -> Option<PaintQuad> {
    if layout.content_height <= bounds.size.height {
        return None;
    }
    let track_height = bounds.size.height;
    let thumb_height = (track_height * (track_height / layout.content_height)).max(px(24.0));
    let max_scroll = layout.content_height - track_height;
    let travel = track_height - thumb_height;
    let top = bounds.top() + travel * (layout.scroll_y / max_scroll);
    Some(fill(
        Bounds::new(
            point(bounds.right() - px(4.0), top),
            size(px(2.0), thumb_height),
        ),
        theme::edge_hard(),
    ))
}
