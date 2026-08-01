//! Block-level markdown rendering.
//!
//! Every block construct gets the same care as the table card: compact
//! rhythm, semantic tokens, hanging structure. Text leaves (paragraphs,
//! headings, code, list items) carry their own `TextLayout` and snapshot so
//! the conversation view can offer drag selection across block boundaries.

use std::ops::Range;

use gpui::{
    AnyElement, FontStyle, FontWeight, HighlightStyle, SharedString, StrikethroughStyle,
    StyledText, TextLayout, TextRun, TextStyle, UnderlineStyle, div, prelude::*, px, relative,
};

use crate::theme;
use crate::views::controls::ClickHandler;

use super::{
    ListMarker, MarkdownBlock, MarkdownListItem, MarkdownStyle, MarkdownTable, ProseBlock,
    TableAlign,
};

/// Vertical rhythm between sibling blocks.
const BLOCK_GAP: f32 = 10.0;
/// Prose leading; tighter than the shell's chat default, easy on long reads.
const PROSE_LEADING: f32 = 1.45;
const CODE_LEADING: f32 = 1.35;
/// Inline code sits a half step below the surrounding font size.
const INLINE_CODE_SCALE: f32 = 0.92;
const QUOTE_BAR_W: f32 = 2.0;
const QUOTE_INDENT: f32 = 10.0;
const MARKER_COL_W: f32 = 18.0;
const LIST_GAP: f32 = 4.0;

/// One selectable text leaf: block index-order selection in the conversation
/// view pairs these snapshots with hit-tested `TextLayout`s.
pub(in crate::views) struct LeafInfo {
    /// Leaves from items of one list join with a single newline on copy.
    pub list_key: Option<usize>,
    pub text: SharedString,
    pub layout: TextLayout,
}

/// `(leaf index, byte offset)` into the render-ordered leaf list.
pub(in crate::views) type LeafPoint = (usize, usize);

pub(in crate::views) struct MarkdownRenderOptions<'a> {
    /// Window text style refined by the surrounding transcript wrapper.
    pub default_style: &'a TextStyle,
    /// `default_style` resolved to pixels for relative inline-code sizing.
    pub base_font_px: f32,
    /// Normalized (start <= end) leaf selection; `None` when collapsed.
    pub selection: Option<(LeafPoint, LeafPoint)>,
    /// Code card slot currently showing its copied acknowledgement.
    pub copied: Option<usize>,
    /// Factory for the copy handler of a code card: `(slot, code)`.
    pub copy_code: &'a dyn Fn(usize, String) -> ClickHandler,
    /// Unique prefix (message block id) for interactive element ids.
    pub id_prefix: &'a str,
}

pub(in crate::views) fn render_document(
    blocks: &[MarkdownBlock],
    options: &MarkdownRenderOptions<'_>,
) -> (AnyElement, Vec<LeafInfo>) {
    let mut renderer = BlockRender {
        options,
        leaves: Vec::new(),
        block_key: 0,
    };
    let element =
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap(px(BLOCK_GAP))
            .children(blocks.iter().enumerate().map(|(index, block)| {
                renderer.render_block(block, options.default_style, index == 0)
            }))
            .into_any_element();
    (element, renderer.leaves)
}

struct BlockRender<'a, 'o> {
    options: &'o MarkdownRenderOptions<'a>,
    leaves: Vec<LeafInfo>,
    /// Stable key per top-level block: copy slots, list identity, element ids.
    block_key: usize,
}

impl BlockRender<'_, '_> {
    fn next_key(&mut self) -> usize {
        let key = self.block_key;
        self.block_key += 1;
        key
    }

    fn render_block(
        &mut self,
        block: &MarkdownBlock,
        style: &TextStyle,
        top_of_flow: bool,
    ) -> AnyElement {
        match block {
            MarkdownBlock::Prose(prose) => self.prose_leaf(prose, style, PROSE_LEADING, None),
            MarkdownBlock::Heading { level, prose } => {
                self.render_heading(*level, prose, style, top_of_flow)
            }
            MarkdownBlock::Code { language, code } => self.render_code(language, code),
            MarkdownBlock::Quote(blocks) => self.render_quote(blocks, style),
            MarkdownBlock::Rule => div()
                .w_full()
                .py(px(2.0))
                .flex()
                .flex_row()
                .items_center()
                .child(div().h(px(1.0)).w_full().bg(theme::edge_soft()))
                .into_any_element(),
            MarkdownBlock::List(items) => self.render_list(items, style),
            MarkdownBlock::Table(table) => render_table(table),
        }
    }

    /// Selection window of the next leaf, clipped to its text.
    fn leaf_selection(&self, len: usize) -> Range<usize> {
        let Some((start, end)) = self.options.selection else {
            return 0..0;
        };
        let index = self.leaves.len();
        if index < start.0 || index > end.0 {
            return 0..0;
        }
        let lo = if index == start.0 {
            start.1.min(len)
        } else {
            0
        };
        let hi = if index == end.0 { end.1.min(len) } else { len };
        lo..hi
    }

    /// Standalone shaped leaf; snapshots the text once (Arc-backed) for copy
    /// and hit testing. The wrapper only carries leading so wrapped lines
    /// inherit rhythm from the block kind.
    fn leaf_text(
        &mut self,
        text: &str,
        runs: Vec<TextRun>,
        leading: f32,
        list_key: Option<usize>,
    ) -> gpui::Div {
        let shared = SharedString::from(text.to_owned());
        let text = StyledText::new(shared.clone()).with_runs(runs);
        self.leaves.push(LeafInfo {
            list_key,
            text: shared,
            layout: text.layout().clone(),
        });
        div().w_full().line_height(relative(leading)).child(text)
    }

    fn prose_leaf(
        &mut self,
        prose: &ProseBlock,
        style: &TextStyle,
        leading: f32,
        list_key: Option<usize>,
    ) -> AnyElement {
        let selection = self.leaf_selection(prose.text.len());
        let runs = text_runs(prose, selection, style, self.options.base_font_px);
        self.leaf_text(&prose.text, runs, leading, list_key)
            .into_any_element()
    }

    fn render_heading(
        &mut self,
        level: u8,
        prose: &ProseBlock,
        style: &TextStyle,
        top_of_flow: bool,
    ) -> AnyElement {
        let (size, weight) = match level {
            1 => (theme::T_WORDMARK, FontWeight::BOLD),
            2 => (theme::T_BODY, FontWeight::SEMIBOLD),
            3 => (theme::T_BODY_SM, FontWeight::SEMIBOLD),
            _ => (theme::T_UI, FontWeight::SEMIBOLD),
        };
        let mut style = style.clone();
        style.font_size = theme::text_size(size).into();
        style.font_weight = weight;
        if level >= 4 {
            style.color = theme::ash().into();
        }

        let selection = self.leaf_selection(prose.text.len());
        let runs = text_runs(prose, selection, &style, self.options.base_font_px);
        div()
            .w_full()
            .flex()
            .flex_col()
            .when(!top_of_flow, |heading| {
                heading.pt(px(match level {
                    1 => 8.0,
                    2 => 6.0,
                    _ => 2.0,
                }))
            })
            // The hairline under h1 anchors a reply the way a deck title does.
            .when(level == 1, |heading| {
                heading
                    .pb(px(6.0))
                    .border_b_1()
                    .border_color(theme::edge_soft())
            })
            .child(self.leaf_text(&prose.text, runs, 1.25, None))
            .into_any_element()
    }

    fn render_code(&mut self, language: &Option<String>, code: &str) -> AnyElement {
        let key = self.next_key();

        let label = match language.as_deref() {
            Some("math") => Some("MATH".to_owned()),
            Some(language) => Some(language.to_uppercase()),
            None => None,
        };
        let copied = self.options.copied == Some(key);
        let copy = (self.options.copy_code)(key, code.to_owned());
        let header = div()
            .w_full()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .gap(px(8.0))
            .px(px(12.0))
            .py(px(4.0))
            .bg(theme::panel_lift())
            .border_b_1()
            .border_color(theme::edge_soft())
            .when_some(label, |header, label| {
                header.child(
                    div()
                        .flex_shrink_0()
                        .font_family(theme::mono())
                        .text_size(theme::text_size(theme::T_TINY))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme::smoke())
                        .child(label),
                )
            })
            .child(
                div()
                    .id(SharedString::from(format!(
                        "{}-code-copy-{key}",
                        self.options.id_prefix
                    )))
                    .ml_auto()
                    .px(px(6.0))
                    .py(px(1.0))
                    .rounded(px(theme::RADIUS_SM))
                    .cursor_pointer()
                    .text_color(if copied { theme::live() } else { theme::ash() })
                    .hover(|button| button.bg(theme::panel_hover()).text_color(theme::bone()))
                    // Keep the press from collapsing transcript selection.
                    .on_mouse_down(gpui::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(copy)
                    .child(
                        div()
                            .font_family(theme::sans())
                            .text_size(theme::text_size(theme::T_TINY))
                            .font_weight(FontWeight::SEMIBOLD)
                            .child(if copied { "Copied" } else { "Copy" }),
                    ),
            );

        let mut card = div()
            .w_full()
            .min_w_0()
            .rounded(px(theme::RADIUS))
            .border_1()
            .border_color(theme::edge_soft())
            .bg(theme::panel())
            .overflow_hidden()
            .child(header);

        if !code.is_empty() {
            let mut style = self.options.default_style.clone();
            style.font_family = theme::mono();
            style.font_size = theme::text_size(theme::T_MONO).into();
            style.color = theme::bone_dim().into();
            let selection = self.leaf_selection(code.len());
            let runs = plain_runs(code.len(), selection, &style);
            card = card.child(
                div()
                    .id(SharedString::from(format!(
                        "{}-code-scroll-{key}",
                        self.options.id_prefix
                    )))
                    .w_full()
                    .min_w_0()
                    .overflow_x_scroll()
                    .scrollbar_width(px(4.0))
                    .px(px(12.0))
                    .py(px(8.0))
                    .whitespace_nowrap()
                    .child(self.leaf_text(code, runs, CODE_LEADING, None)),
            );
        }

        card.into_any_element()
    }

    fn render_quote(&mut self, blocks: &[MarkdownBlock], style: &TextStyle) -> AnyElement {
        let mut style = style.clone();
        style.color = theme::ash().into();
        div()
            .w_full()
            .flex()
            .flex_row()
            .child(
                div()
                    .w(px(QUOTE_BAR_W))
                    .flex_shrink_0()
                    .h_full()
                    .rounded_full()
                    .bg(theme::edge_hard()),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .pl(px(QUOTE_INDENT))
                    .flex()
                    .flex_col()
                    .gap(px(8.0))
                    .children(
                        blocks
                            .iter()
                            .enumerate()
                            .map(|(index, block)| self.render_block(block, &style, index == 0)),
                    ),
            )
            .into_any_element()
    }

    fn render_list(&mut self, items: &[MarkdownListItem], style: &TextStyle) -> AnyElement {
        let key = self.next_key();
        div()
            .w_full()
            .flex()
            .flex_col()
            .gap(px(LIST_GAP))
            .children(
                items
                    .iter()
                    .map(|item| self.render_list_item(item, style, key)),
            )
            .into_any_element()
    }

    fn render_list_item(
        &mut self,
        item: &MarkdownListItem,
        style: &TextStyle,
        list_key: usize,
    ) -> AnyElement {
        let (marker, color) = match &item.marker {
            ListMarker::Bullet => ("•".to_owned(), theme::ash()),
            ListMarker::Ordered(number) => (format!("{number}."), theme::ash()),
            ListMarker::Task { checked: true } => ("☑".to_owned(), theme::live()),
            ListMarker::Task { checked: false } => ("☐".to_owned(), theme::smoke()),
        };

        div()
            .w_full()
            .flex()
            .flex_row()
            .gap(px(8.0))
            .child(
                // Hanging marker column: wrapped lines hang on the content,
                // nested lists indent as structure.
                div()
                    .w(px(MARKER_COL_W))
                    .flex_shrink_0()
                    .flex()
                    .flex_row()
                    .justify_end()
                    .line_height(relative(PROSE_LEADING))
                    .child(
                        div()
                            .font_family(theme::sans())
                            .text_size(theme::text_size(theme::T_UI))
                            .text_color(color)
                            .child(marker),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .gap(px(4.0))
                    .children(item.blocks.iter().enumerate().map(|(index, block)| {
                        match block {
                            // Item paragraphs surface with the list key so a
                            // drag selection copies them newline-joined.
                            MarkdownBlock::Prose(prose) => {
                                self.prose_leaf(prose, style, PROSE_LEADING, Some(list_key))
                            }
                            block => self.render_block(block, style, index == 0),
                        }
                    })),
            )
            .into_any_element()
    }
}

fn plain_runs(len: usize, selection: Range<usize>, style: &TextStyle) -> Vec<TextRun> {
    if len == 0 {
        return Vec::new();
    }
    if selection.is_empty() {
        return vec![style.to_run(len)];
    }
    let mut runs = Vec::with_capacity(3);
    let mut push = |range: Range<usize>, selected: bool| {
        if range.is_empty() {
            return;
        }
        let mut run = style.to_run(range.len());
        if selected {
            run.background_color = Some(theme::data_wash().into());
        }
        runs.push(run);
    };
    push(0..selection.start.min(len), false);
    push(selection.start.min(len)..selection.end.min(len), true);
    push(selection.end.min(len)..len, false);
    runs
}

/// Build text runs in one pass over the span/selection boundary windows:
/// resolves each window's inline flags and selection state together so a
/// render does not fold the span list twice.
pub(in crate::views) fn text_runs(
    prose: &ProseBlock,
    selection: Range<usize>,
    base_style: &TextStyle,
    base_font_px: f32,
) -> Vec<TextRun> {
    let mut boundaries = Vec::with_capacity(prose.spans.len() * 2 + 4);
    boundaries.push(0);
    boundaries.push(prose.text.len());
    for span in &prose.spans {
        boundaries.push(span.range.start);
        boundaries.push(span.range.end);
    }
    if !selection.is_empty() {
        boundaries.push(selection.start);
        boundaries.push(selection.end);
    }
    boundaries.sort_unstable();
    boundaries.dedup();

    let mut runs = Vec::new();
    for window in boundaries.windows(2) {
        let range = window[0]..window[1];
        if range.is_empty() {
            continue;
        }
        let markdown = inline_style_for_range(prose, &range);
        let selected =
            !selection.is_empty() && selection.start <= range.start && selection.end >= range.end;
        let highlight = highlight_style(markdown, selected);
        let fully_default = highlight == HighlightStyle::default() && !markdown.code;
        if fully_default {
            runs.push(base_style.to_run(range.len()));
            continue;
        }
        let mut style = base_style.clone();
        if highlight != HighlightStyle::default() {
            style = style.highlight(highlight);
        }
        if markdown.code {
            style.font_family = theme::mono();
            style.font_size = theme::text_size(base_font_px * INLINE_CODE_SCALE).into();
        }
        runs.push(style.to_run(range.len()));
    }
    runs
}

fn inline_style_for_range(prose: &ProseBlock, range: &Range<usize>) -> MarkdownStyle {
    prose
        .spans
        .iter()
        .filter(|span| span.range.start <= range.start && span.range.end >= range.end)
        .fold(MarkdownStyle::default(), |mut combined, span| {
            combined.strong |= span.style.strong;
            combined.emphasis |= span.style.emphasis;
            combined.code |= span.style.code;
            combined.link |= span.style.link;
            combined.strikethrough |= span.style.strikethrough;
            combined
        })
}

fn highlight_style(markdown: MarkdownStyle, selected: bool) -> HighlightStyle {
    HighlightStyle {
        color: if markdown.code {
            Some(theme::focus().into())
        } else if markdown.link {
            Some(theme::data().into())
        } else {
            None
        },
        font_weight: markdown.strong.then_some(FontWeight::SEMIBOLD),
        font_style: markdown.emphasis.then_some(FontStyle::Italic),
        background_color: if selected {
            Some(theme::data_wash().into())
        } else if markdown.code {
            Some(theme::panel_lift().into())
        } else {
            None
        },
        underline: markdown.link.then_some(UnderlineStyle {
            thickness: px(1.0),
            color: Some(theme::data().into()),
            ..Default::default()
        }),
        strikethrough: markdown.strikethrough.then_some(StrikethroughStyle {
            thickness: px(1.0),
            color: Some(theme::smoke().into()),
        }),
        fade_out: None,
    }
}

// -- Tables ------------------------------------------------------------------
// The reference treatment: bordered card, lifted header strip, zebra rows,
// alignment, mono identifiers, and status chips.

fn render_table(table: &MarkdownTable) -> AnyElement {
    let columns = table.column_count();
    if columns == 0 {
        return div().into_any_element();
    }

    let header = if table.headers.is_empty() {
        None
    } else {
        Some(
            div()
                .w_full()
                .flex()
                .flex_row()
                .items_center()
                .bg(theme::panel_lift())
                .border_b_1()
                .border_color(theme::edge_soft())
                .children((0..columns).map(|column| {
                    table_cell(
                        table.cell(&table.headers, column),
                        table
                            .alignments
                            .get(column)
                            .copied()
                            .unwrap_or(TableAlign::None),
                        column,
                        columns,
                        true,
                        false,
                    )
                })),
        )
    };

    let body_rows = table.rows.iter().enumerate().map(|(row_index, row)| {
        let last = row_index + 1 == table.rows.len();
        let zebra = row_index % 2 == 1;
        div()
            .w_full()
            .flex()
            .flex_row()
            .items_center()
            .when(zebra, |row| row.bg(theme::floor()))
            .when(!last, |row| {
                row.border_b_1().border_color(theme::edge_soft())
            })
            .children((0..columns).map(|column| {
                table_cell(
                    table.cell(row, column),
                    table
                        .alignments
                        .get(column)
                        .copied()
                        .unwrap_or(TableAlign::None),
                    column,
                    columns,
                    false,
                    column == 0,
                )
            }))
    });

    div()
        .w_full()
        .min_w_0()
        .rounded(px(theme::RADIUS))
        .border_1()
        .border_color(theme::edge_soft())
        .bg(theme::panel())
        .overflow_hidden()
        .children(header)
        .children(body_rows)
        .into_any_element()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TableStatusTone {
    Positive,
    Caution,
    Negative,
    Neutral,
}

fn table_status_tone(value: &str) -> Option<TableStatusTone> {
    match value.trim().to_ascii_lowercase().as_str() {
        "yes" | "true" | "ok" | "current" | "ready" | "enabled" => Some(TableStatusTone::Positive),
        "mostly" | "partial" | "partially" | "mixed" | "warn" | "warning" => {
            Some(TableStatusTone::Caution)
        }
        "no" | "false" | "error" | "failed" | "disabled" => Some(TableStatusTone::Negative),
        "n/a" | "na" | "none" | "unknown" | "—" | "-" => Some(TableStatusTone::Neutral),
        _ => None,
    }
}

fn table_cell(
    value: &str,
    align: TableAlign,
    column: usize,
    columns: usize,
    header: bool,
    prefer_mono: bool,
) -> AnyElement {
    let last = column + 1 == columns;
    let mono =
        prefer_mono && !header && value.chars().any(|ch| matches!(ch, '/' | '_' | '.' | ':'));
    let status = (!header).then(|| table_status_tone(value)).flatten();
    // Last column of wider tables stays compact (status chips); body columns share space.
    let compact = columns >= 3 && last;

    let mut cell = div()
        .when(compact, |cell| cell.flex_shrink_0().w(px(104.0)))
        .when(!compact, |cell| {
            cell.flex_1()
                .min_w(px(if column == 0 { 120.0 } else { 140.0 }))
        })
        .min_w_0()
        .px(px(12.0))
        .py(px(if header { 8.0 } else { 9.0 }))
        .flex()
        .flex_row()
        .items_center();

    cell = match align {
        TableAlign::Center => cell.justify_center(),
        TableAlign::Right => cell.justify_end(),
        TableAlign::None | TableAlign::Left => cell.justify_start(),
    };

    if !last {
        cell = cell.border_r_1().border_color(theme::edge_soft());
    }

    if let Some(tone) = status {
        cell.child(status_chip(value, tone)).into_any_element()
    } else {
        cell.child(
            div()
                .w_full()
                .min_w_0()
                .font_family(if mono { theme::mono() } else { theme::sans() })
                .text_size(theme::text_size(if mono {
                    theme::T_MONO
                } else {
                    theme::T_UI_SM
                }))
                .font_weight(if header {
                    FontWeight::SEMIBOLD
                } else {
                    FontWeight::NORMAL
                })
                .text_color(if header {
                    theme::ash()
                } else if mono {
                    theme::bone_dim()
                } else {
                    theme::bone()
                })
                .line_height(relative(1.35))
                .child(value.to_owned()),
        )
        .into_any_element()
    }
}

fn status_chip(label: &str, tone: TableStatusTone) -> impl IntoElement {
    let (fg, bg) = match tone {
        TableStatusTone::Positive => (theme::live(), theme::live_wash()),
        TableStatusTone::Caution => (theme::data(), theme::data_wash()),
        TableStatusTone::Negative => (theme::error(), theme::error_wash()),
        TableStatusTone::Neutral => (theme::smoke(), theme::panel_lift()),
    };

    div()
        .px(px(7.0))
        .py(px(2.0))
        .rounded_full()
        .bg(bg)
        .font_family(theme::sans())
        .text_size(theme::text_size(theme::T_TINY))
        .font_weight(FontWeight::SEMIBOLD)
        .text_color(fg)
        .child(label.to_owned())
}
