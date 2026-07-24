use std::ops::Range;

use pulldown_cmark::{Alignment, Event, Options, Parser, Tag, TagEnd};
use unicode_width::UnicodeWidthStr;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct MarkdownStyle {
    pub heading: bool,
    pub strong: bool,
    pub emphasis: bool,
    pub code: bool,
    pub link: bool,
    pub quote: bool,
    pub strikethrough: bool,
    pub table: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct MarkdownSpan {
    pub range: Range<usize>,
    pub style: MarkdownStyle,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct MarkdownDocument {
    pub text: String,
    pub spans: Vec<MarkdownSpan>,
}

impl MarkdownDocument {
    pub fn parse(source: &str) -> Self {
        let options =
            Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS | Options::ENABLE_TABLES;
        let mut builder = DocumentBuilder::default();

        for event in Parser::new_ext(source, options) {
            builder.handle(event);
        }
        builder.finish()
    }
}

struct DocumentBuilder {
    document: MarkdownDocument,
    style: MarkdownStyle,
    quote_depth: usize,
    list_stack: Vec<ListState>,
    at_line_start: bool,
    table: Option<TableState>,
}

impl Default for DocumentBuilder {
    fn default() -> Self {
        Self {
            document: MarkdownDocument::default(),
            style: MarkdownStyle::default(),
            quote_depth: 0,
            list_stack: Vec::new(),
            at_line_start: true,
            table: None,
        }
    }
}

struct ListState {
    next_number: Option<u64>,
}

struct TableState {
    alignments: Vec<Alignment>,
    rows: Vec<TableRow>,
    current_row: Vec<String>,
    current_cell: String,
    in_head: bool,
}

struct TableRow {
    cells: Vec<String>,
    header: bool,
}

impl DocumentBuilder {
    fn handle(&mut self, event: Event<'_>) {
        if self.handle_table_event(&event) {
            return;
        }

        match event {
            Event::Start(Tag::Table(alignments)) => {
                self.ensure_block_gap();
                self.table = Some(TableState {
                    alignments,
                    rows: Vec::new(),
                    current_row: Vec::new(),
                    current_cell: String::new(),
                    in_head: false,
                });
            }
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(text) => self.push(&text, self.style),
            Event::Code(text) => {
                let mut style = self.style;
                style.code = true;
                self.push(&text, style);
            }
            Event::SoftBreak | Event::HardBreak => self.newline(),
            Event::Rule => {
                self.ensure_block_gap();
                self.push("────────", self.style);
                self.ensure_block_gap();
            }
            Event::TaskListMarker(checked) => {
                self.push(if checked { "[x] " } else { "[ ] " }, self.style);
            }
            Event::InlineMath(text) => self.push(&text, self.style),
            Event::DisplayMath(text) => {
                self.ensure_block_gap();
                self.push(&text, self.style);
                self.ensure_block_gap();
            }
            Event::Html(text) | Event::InlineHtml(text) => self.push(&text, self.style),
            Event::FootnoteReference(name) => {
                self.push("[", self.style);
                self.push(&name, self.style);
                self.push("]", self.style);
            }
        }
    }

    fn start(&mut self, tag: Tag<'_>) {
        match tag {
            Tag::Paragraph => self.ensure_line_prefix(),
            Tag::Heading { .. } => {
                self.ensure_block_gap();
                self.style.heading = true;
                self.style.strong = true;
                self.ensure_line_prefix();
            }
            Tag::BlockQuote(_) => {
                self.ensure_block_gap();
                self.quote_depth += 1;
                self.style.quote = true;
            }
            Tag::CodeBlock(_) => {
                self.ensure_block_gap();
                self.style.code = true;
                self.ensure_line_prefix();
            }
            Tag::List(start) => {
                self.ensure_block_gap();
                self.list_stack.push(ListState { next_number: start });
            }
            Tag::Item => {
                self.ensure_line();
                let depth = self.list_stack.len().saturating_sub(1);
                if depth > 0 {
                    self.push(&"  ".repeat(depth), self.style);
                }
                let marker = self
                    .list_stack
                    .last_mut()
                    .and_then(|list| {
                        list.next_number.as_mut().map(|number| {
                            let marker = format!("{number}. ");
                            *number = number.saturating_add(1);
                            marker
                        })
                    })
                    .unwrap_or_else(|| "• ".to_owned());
                self.push(&marker, self.style);
            }
            Tag::Emphasis => self.style.emphasis = true,
            Tag::Strong => self.style.strong = true,
            Tag::Strikethrough => self.style.strikethrough = true,
            Tag::Link { .. } => self.style.link = true,
            Tag::Image { dest_url, .. } => {
                self.push("Image: ", self.style);
                let mut style = self.style;
                style.link = true;
                self.push(&dest_url, style);
                self.push(" — ", self.style);
            }
            _ => {}
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => self.ensure_block_gap(),
            TagEnd::Heading(_) => {
                self.style.heading = false;
                self.style.strong = false;
                self.ensure_block_gap();
            }
            TagEnd::BlockQuote(_) => {
                self.quote_depth = self.quote_depth.saturating_sub(1);
                self.style.quote = self.quote_depth > 0;
                self.ensure_block_gap();
            }
            TagEnd::CodeBlock => {
                self.style.code = false;
                self.ensure_block_gap();
            }
            TagEnd::List(_) => {
                self.list_stack.pop();
                self.ensure_block_gap();
            }
            TagEnd::Item => self.newline(),
            TagEnd::Emphasis => self.style.emphasis = false,
            TagEnd::Strong => self.style.strong = false,
            TagEnd::Strikethrough => self.style.strikethrough = false,
            TagEnd::Link => self.style.link = false,
            _ => {}
        }
    }

    fn handle_table_event(&mut self, event: &Event<'_>) -> bool {
        let Some(table) = self.table.as_mut() else {
            return false;
        };

        match event {
            Event::Start(Tag::TableHead) => {
                table.current_row.clear();
                table.in_head = true;
            }
            Event::End(TagEnd::TableHead) => {
                table.rows.push(TableRow {
                    cells: std::mem::take(&mut table.current_row),
                    header: true,
                });
                table.in_head = false;
            }
            Event::Start(Tag::TableRow) => table.current_row.clear(),
            Event::Start(Tag::TableCell) => table.current_cell.clear(),
            Event::Text(text) | Event::Code(text) => table.current_cell.push_str(text),
            Event::SoftBreak | Event::HardBreak => table.current_cell.push(' '),
            Event::End(TagEnd::TableCell) => {
                table.current_row.push(normalize_cell(&table.current_cell));
                table.current_cell.clear();
            }
            Event::End(TagEnd::TableRow) => {
                table.rows.push(TableRow {
                    cells: std::mem::take(&mut table.current_row),
                    header: table.in_head,
                });
            }
            Event::End(TagEnd::Table) => {
                let table = self.table.take().expect("active Markdown table");
                self.push_table(table);
                self.ensure_block_gap();
            }
            _ => {}
        }
        true
    }

    fn push_table(&mut self, table: TableState) {
        if table.rows.is_empty() {
            return;
        }

        let column_count = table
            .rows
            .iter()
            .map(|row| row.cells.len())
            .max()
            .unwrap_or(0);
        if column_count == 0 {
            return;
        }

        let widths = (0..column_count)
            .map(|column| {
                table
                    .rows
                    .iter()
                    .filter_map(|row| row.cells.get(column))
                    .map(|cell| UnicodeWidthStr::width(cell.as_str()))
                    .max()
                    .unwrap_or(0)
            })
            .collect::<Vec<_>>();

        let top = table_border('┌', '┬', '┐', &widths);
        let middle = table_border('├', '┼', '┤', &widths);
        let bottom = table_border('└', '┴', '┘', &widths);
        let mut rendered = String::new();
        let mut header_ranges = Vec::new();
        rendered.push_str(&top);
        rendered.push('\n');
        for (index, row) in table.rows.iter().enumerate() {
            let start = rendered.len();
            rendered.push('│');
            for (column, width) in widths.iter().copied().enumerate() {
                let cell = row.cells.get(column).map(String::as_str).unwrap_or("");
                let alignment = table
                    .alignments
                    .get(column)
                    .copied()
                    .unwrap_or(Alignment::None);
                rendered.push('\u{a0}');
                rendered.push_str(&aligned_cell(cell, width, alignment));
                rendered.push('\u{a0}');
                rendered.push('│');
            }
            if row.header {
                header_ranges.push(start..rendered.len());
            }
            rendered.push('\n');
            if row.header && index + 1 < table.rows.len() {
                rendered.push_str(&middle);
                rendered.push('\n');
            }
        }
        rendered.push_str(&bottom);

        let table_start = self.document.text.len();
        let mut style = self.style;
        style.table = true;
        self.push(&rendered, style);
        for range in header_ranges {
            let start = table_start + range.start;
            let end = table_start + range.end;
            let mut header_style = style;
            header_style.strong = true;
            self.document.spans.push(MarkdownSpan {
                range: start..end,
                style: header_style,
            });
        }
    }

    fn ensure_line_prefix(&mut self) {
        if self.at_line_start && self.quote_depth > 0 {
            self.push(&"│ ".repeat(self.quote_depth), self.style);
        }
    }

    fn ensure_line(&mut self) {
        if !self.document.text.is_empty() && !self.at_line_start {
            self.newline();
        }
        self.ensure_line_prefix();
    }

    fn ensure_block_gap(&mut self) {
        while self.document.text.ends_with("\n\n\n") {
            self.document.text.pop();
        }
        if !self.document.text.is_empty() {
            if !self.document.text.ends_with('\n') {
                self.newline();
            }
            if !self.document.text.ends_with("\n\n") {
                self.newline();
            }
        }
    }

    fn newline(&mut self) {
        self.document.text.push('\n');
        self.at_line_start = true;
    }

    fn push(&mut self, text: &str, style: MarkdownStyle) {
        if text.is_empty() {
            return;
        }
        let start = self.document.text.len();
        self.document.text.push_str(text);
        let end = self.document.text.len();
        self.at_line_start = text.ends_with('\n');

        if style != MarkdownStyle::default() {
            if let Some(last) = self.document.spans.last_mut()
                && last.range.end == start
                && last.style == style
            {
                last.range.end = end;
                return;
            }
            self.document.spans.push(MarkdownSpan {
                range: start..end,
                style,
            });
        }
    }

    fn finish(mut self) -> MarkdownDocument {
        while self.document.text.ends_with('\n') {
            self.document.text.pop();
        }
        self.document
            .spans
            .retain(|span| span.range.start < self.document.text.len());
        for span in &mut self.document.spans {
            span.range.end = span.range.end.min(self.document.text.len());
        }
        self.document
    }
}

fn normalize_cell(cell: &str) -> String {
    cell.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn table_border(left: char, join: char, right: char, widths: &[usize]) -> String {
    let mut border = String::new();
    border.push(left);
    for (index, width) in widths.iter().enumerate() {
        border.push_str(&"─".repeat(width + 2));
        border.push(if index + 1 == widths.len() {
            right
        } else {
            join
        });
    }
    border
}

fn aligned_cell(cell: &str, width: usize, alignment: Alignment) -> String {
    let content_width = UnicodeWidthStr::width(cell);
    let remaining = width.saturating_sub(content_width);
    let (left, right) = match alignment {
        Alignment::Center => (remaining / 2, remaining - remaining / 2),
        Alignment::Right => (remaining, 0),
        Alignment::None | Alignment::Left => (0, remaining),
    };
    format!(
        "{}{}{}",
        "\u{a0}".repeat(left),
        cell.replace(' ', "\u{a0}"),
        "\u{a0}".repeat(right)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_core_markdown_as_styled_plain_text() {
        let document = MarkdownDocument::parse(
            "# Result\n\nUse **bold**, *care*, `code`, and [docs](https://example.com).\n\n- one\n- two",
        );

        assert_eq!(
            document.text,
            "Result\n\nUse bold, care, code, and docs.\n\n• one\n• two"
        );
        assert!(
            document.spans.iter().any(|span| {
                &document.text[span.range.clone()] == "Result" && span.style.heading
            })
        );
        assert!(
            document
                .spans
                .iter()
                .any(|span| { &document.text[span.range.clone()] == "bold" && span.style.strong })
        );
        assert!(
            document
                .spans
                .iter()
                .any(|span| { &document.text[span.range.clone()] == "code" && span.style.code })
        );
        assert!(
            document
                .spans
                .iter()
                .any(|span| { &document.text[span.range.clone()] == "docs" && span.style.link })
        );
    }

    #[test]
    fn renders_quotes_tasks_and_ordered_lists() {
        let document = MarkdownDocument::parse(
            "> Think carefully.\n\n1. First\n2. Second\n\n- [x] Done\n- [ ] Next",
        );

        assert_eq!(
            document.text,
            "│ Think carefully.\n\n1. First\n2. Second\n\n• [x] Done\n• [ ] Next"
        );
        assert!(document.spans.iter().any(|span| span.style.quote));
    }

    #[test]
    fn keeps_incomplete_streaming_markdown_readable() {
        let document = MarkdownDocument::parse("Working on **the answer");
        assert!(document.text.contains("Working on"));
        assert!(document.text.contains("the answer"));
    }

    #[test]
    fn renders_markdown_tables_as_aligned_grids() {
        let document = MarkdownDocument::parse(
            "Before\n\n| Component | Installed | Latest | Status |\n|:--|--:|:-:|:--|\n| Pi coding agent | 0.80.10 | 0.81.1 | Update available |\n| pi-bar | 0.3.39 | 0.3.39 | Current |\n\nAfter",
        );

        let expected = concat!(
            "Before\n\n",
            "┌─────────────────┬───────────┬────────┬──────────────────┐\n",
            "│ Component       │ Installed │ Latest │ Status           │\n",
            "├─────────────────┼───────────┼────────┼──────────────────┤\n",
            "│ Pi coding agent │   0.80.10 │ 0.81.1 │ Update available │\n",
            "│ pi-bar          │    0.3.39 │ 0.3.39 │ Current          │\n",
            "└─────────────────┴───────────┴────────┴──────────────────┘\n\n",
            "After",
        );
        assert_eq!(document.text.replace('\u{a0}', " "), expected);
        assert!(document.spans.iter().any(|span| span.style.table));
        assert!(
            document
                .spans
                .iter()
                .any(|span| span.style.table && span.style.strong)
        );
    }
}
