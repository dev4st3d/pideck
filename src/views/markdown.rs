use std::ops::Range;

use pulldown_cmark::{Alignment, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct MarkdownStyle {
    pub heading: bool,
    pub heading_level: u8,
    pub strong: bool,
    pub emphasis: bool,
    pub code: bool,
    pub code_block: bool,
    pub link: bool,
    pub quote: bool,
    pub strikethrough: bool,
    pub task_marker: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct MarkdownSpan {
    pub range: Range<usize>,
    pub style: MarkdownStyle,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum TableAlign {
    None,
    Left,
    Center,
    Right,
}

impl From<Alignment> for TableAlign {
    fn from(value: Alignment) -> Self {
        match value {
            Alignment::None => Self::None,
            Alignment::Left => Self::Left,
            Alignment::Center => Self::Center,
            Alignment::Right => Self::Right,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct MarkdownTable {
    pub alignments: Vec<TableAlign>,
    pub headers: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

impl MarkdownTable {
    pub fn column_count(&self) -> usize {
        self.headers.len().max(
            self.rows
                .iter()
                .map(|row| row.cells_len())
                .max()
                .unwrap_or(0),
        )
    }

    pub fn cell<'a>(&'a self, row: &'a [String], column: usize) -> &'a str {
        row.get(column).map(String::as_str).unwrap_or("")
    }

    pub fn to_plain_text(&self) -> String {
        let columns = self.column_count();
        if columns == 0 {
            return String::new();
        }

        let mut lines = Vec::new();
        if !self.headers.is_empty() {
            lines.push(join_row(&self.headers, columns));
        }
        for row in &self.rows {
            lines.push(join_row(row, columns));
        }
        lines.join("\n")
    }
}

trait RowCells {
    fn cells_len(&self) -> usize;
}

impl RowCells for Vec<String> {
    fn cells_len(&self) -> usize {
        self.len()
    }
}

fn join_row(cells: &[String], columns: usize) -> String {
    (0..columns)
        .map(|index| cells.get(index).map(String::as_str).unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\t")
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(super) struct ProseBlock {
    pub text: String,
    pub spans: Vec<MarkdownSpan>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum MarkdownBlock {
    Prose(ProseBlock),
    Table(MarkdownTable),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(super) struct MarkdownDocument {
    pub blocks: Vec<MarkdownBlock>,
}

impl MarkdownDocument {
    pub fn parse(source: &str) -> Self {
        let options = Options::ENABLE_STRIKETHROUGH
            | Options::ENABLE_TASKLISTS
            | Options::ENABLE_TABLES
            | Options::ENABLE_FOOTNOTES
            | Options::ENABLE_MATH;
        let mut builder = DocumentBuilder::default();

        for event in Parser::new_ext(source, options) {
            builder.handle(event);
        }
        builder.finish()
    }

    /// True when the document is a single prose block (supports drag selection).
    pub fn is_selectable_prose(&self) -> bool {
        matches!(self.blocks.as_slice(), [MarkdownBlock::Prose(_)])
    }

    pub fn sole_prose(&self) -> Option<&ProseBlock> {
        match self.blocks.as_slice() {
            [MarkdownBlock::Prose(prose)] => Some(prose),
            _ => None,
        }
    }

    pub fn plain_text(&self) -> String {
        let mut parts = Vec::new();
        for block in &self.blocks {
            match block {
                MarkdownBlock::Prose(prose) if !prose.text.is_empty() => {
                    parts.push(prose.text.clone());
                }
                MarkdownBlock::Table(table) => {
                    let text = table.to_plain_text();
                    if !text.is_empty() {
                        parts.push(text);
                    }
                }
                MarkdownBlock::Prose(_) => {}
            }
        }
        parts.join("\n\n")
    }
}

struct DocumentBuilder {
    blocks: Vec<MarkdownBlock>,
    prose: ProseBlock,
    style: MarkdownStyle,
    quote_depth: usize,
    list_stack: Vec<ListState>,
    at_line_start: bool,
    table: Option<TableState>,
}

impl Default for DocumentBuilder {
    fn default() -> Self {
        Self {
            blocks: Vec::new(),
            prose: ProseBlock::default(),
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
            Event::Text(text) => self.push_text(&text, self.style),
            Event::Code(text) => {
                let mut style = self.style;
                style.code = true;
                self.push_text(&text, style);
            }
            Event::SoftBreak | Event::HardBreak => self.newline(),
            Event::Rule => {
                self.ensure_block_gap();
                let mut style = self.style;
                style.quote = true;
                self.push("────────────────────────", style);
                self.ensure_block_gap();
            }
            Event::TaskListMarker(checked) => {
                let mut style = self.style;
                style.task_marker = true;
                self.push(if checked { "☑ " } else { "☐ " }, style);
            }
            Event::InlineMath(text) => {
                let mut style = self.style;
                style.code = true;
                self.push_text(&text, style);
            }
            Event::DisplayMath(text) => {
                self.ensure_block_gap();
                let mut style = self.style;
                style.code = true;
                style.code_block = true;
                self.push_text(&text, style);
                self.ensure_block_gap();
            }
            Event::Html(text) | Event::InlineHtml(text) => self.handle_html(&text),
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
            Tag::Heading { level, .. } => {
                self.ensure_block_gap();
                self.style.heading = true;
                self.style.heading_level = heading_level(level);
                self.style.strong = true;
                self.ensure_line_prefix();
            }
            Tag::BlockQuote(_) => {
                if self.quote_depth == 0 {
                    self.ensure_block_gap();
                }
                self.quote_depth += 1;
                self.style.quote = true;
            }
            Tag::CodeBlock(_) => {
                self.ensure_block_gap();
                self.style.code = true;
                self.style.code_block = true;
                self.ensure_line_prefix();
            }
            Tag::List(start) => {
                if self.list_stack.is_empty() {
                    self.ensure_block_gap();
                }
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
                self.style.heading_level = 0;
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
                self.style.code_block = false;
                self.ensure_block_gap();
            }
            TagEnd::List(_) => {
                self.list_stack.pop();
                if self.list_stack.is_empty() {
                    self.ensure_block_gap();
                }
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
            }
            _ => {}
        }
        true
    }

    fn push_table(&mut self, table: TableState) {
        if table.rows.is_empty() {
            return;
        }

        let mut headers = Vec::new();
        let mut rows = Vec::new();
        for row in table.rows {
            if row.header && headers.is_empty() {
                headers = row.cells;
            } else {
                rows.push(row.cells);
            }
        }

        if headers.is_empty() && rows.is_empty() {
            return;
        }

        self.flush_prose();
        self.blocks.push(MarkdownBlock::Table(MarkdownTable {
            alignments: table.alignments.into_iter().map(TableAlign::from).collect(),
            headers,
            rows,
        }));
        self.at_line_start = true;
    }

    fn flush_prose(&mut self) {
        while self.prose.text.ends_with('\n') {
            self.prose.text.pop();
        }
        self.prose
            .spans
            .retain(|span| span.range.start < self.prose.text.len());
        for span in &mut self.prose.spans {
            span.range.end = span.range.end.min(self.prose.text.len());
        }

        if self.prose.text.is_empty() {
            self.prose = ProseBlock::default();
            self.at_line_start = true;
            return;
        }

        self.blocks
            .push(MarkdownBlock::Prose(std::mem::take(&mut self.prose)));
        self.at_line_start = true;
    }

    fn ensure_line_prefix(&mut self) {
        if self.at_line_start && self.quote_depth > 0 {
            self.push(&"│ ".repeat(self.quote_depth), self.style);
        }
    }

    fn push_text(&mut self, text: &str, style: MarkdownStyle) {
        if text.is_empty() {
            return;
        }

        for (index, line) in text.split('\n').enumerate() {
            if index > 0 {
                self.newline();
            }
            if line.is_empty() {
                continue;
            }
            let line_start = self.at_line_start;
            self.ensure_line_prefix();
            if line_start && style.code_block {
                self.push("  ", style);
            }
            self.push(line, style);
        }
    }

    fn handle_html(&mut self, html: &str) {
        let normalized = html.trim().to_ascii_lowercase();
        if normalized.starts_with("<br") {
            self.newline();
        } else if normalized.starts_with("</p")
            || normalized.starts_with("</div")
            || normalized.starts_with("</details")
            || normalized.starts_with("</summary")
        {
            self.ensure_block_gap();
        }
    }

    fn ensure_line(&mut self) {
        if !self.prose.text.is_empty() && !self.at_line_start {
            self.newline();
        }
        self.ensure_line_prefix();
    }

    fn ensure_block_gap(&mut self) {
        while self.prose.text.ends_with("\n\n\n") {
            self.prose.text.pop();
        }
        if !self.prose.text.is_empty() {
            if !self.prose.text.ends_with('\n') {
                self.newline();
            }
            if !self.prose.text.ends_with("\n\n") {
                self.newline();
            }
        }
    }

    fn newline(&mut self) {
        self.prose.text.push('\n');
        self.at_line_start = true;
    }

    fn push(&mut self, text: &str, style: MarkdownStyle) {
        if text.is_empty() {
            return;
        }
        let start = self.prose.text.len();
        self.prose.text.push_str(text);
        let end = self.prose.text.len();
        self.at_line_start = text.ends_with('\n');

        if style != MarkdownStyle::default() {
            if let Some(last) = self.prose.spans.last_mut()
                && last.range.end == start
                && last.style == style
            {
                last.range.end = end;
                return;
            }
            self.prose.spans.push(MarkdownSpan {
                range: start..end,
                style,
            });
        }
    }

    fn finish(mut self) -> MarkdownDocument {
        self.flush_prose();
        MarkdownDocument {
            blocks: self.blocks,
        }
    }
}

fn normalize_cell(cell: &str) -> String {
    cell.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn heading_level(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sole_prose(document: &MarkdownDocument) -> &ProseBlock {
        document.sole_prose().expect("single prose block")
    }

    #[test]
    fn renders_core_markdown_as_styled_plain_text() {
        let document = MarkdownDocument::parse(
            "# Result\n\nUse **bold**, *care*, `code`, and [docs](https://example.com).\n\n- one\n- two",
        );
        let prose = sole_prose(&document);

        assert_eq!(
            prose.text,
            "Result\n\nUse bold, care, code, and docs.\n\n• one\n• two"
        );
        assert!(
            prose
                .spans
                .iter()
                .any(|span| { &prose.text[span.range.clone()] == "Result" && span.style.heading })
        );
        assert!(
            prose
                .spans
                .iter()
                .any(|span| { &prose.text[span.range.clone()] == "bold" && span.style.strong })
        );
        assert!(
            prose
                .spans
                .iter()
                .any(|span| { &prose.text[span.range.clone()] == "code" && span.style.code })
        );
        assert!(
            prose
                .spans
                .iter()
                .any(|span| { &prose.text[span.range.clone()] == "docs" && span.style.link })
        );
    }

    #[test]
    fn renders_quotes_tasks_and_ordered_lists() {
        let document = MarkdownDocument::parse(
            "> Think carefully.\n\n1. First\n2. Second\n\n- [x] Done\n- [ ] Next",
        );
        let prose = sole_prose(&document);

        assert_eq!(
            prose.text,
            "│ Think carefully.\n\n1. First\n2. Second\n\n• ☑ Done\n• ☐ Next"
        );
        assert!(prose.spans.iter().any(|span| span.style.quote));
        assert!(prose.spans.iter().any(|span| span.style.task_marker));
    }

    #[test]
    fn preserves_heading_levels_and_distinguishes_code_blocks() {
        let document = MarkdownDocument::parse(
            "# Primary\n\n### Detail\n\nInline `code`.\n\n```rust\nfn main() {\n    println!(\"ok\");\n}\n```",
        );
        let prose = sole_prose(&document);

        assert!(prose.spans.iter().any(|span| {
            &prose.text[span.range.clone()] == "Primary" && span.style.heading_level == 1
        }));
        assert!(prose.spans.iter().any(|span| {
            &prose.text[span.range.clone()] == "Detail" && span.style.heading_level == 3
        }));
        assert!(prose.spans.iter().any(|span| {
            span.style.code_block && prose.text[span.range.clone()].contains("println!")
        }));
        assert!(prose.spans.iter().any(|span| {
            span.style.code && !span.style.code_block && &prose.text[span.range.clone()] == "code"
        }));
    }

    #[test]
    fn hides_raw_html_but_keeps_inner_text() {
        let document = MarkdownDocument::parse("Before <kbd>Ctrl</kbd><br>After");
        let prose = sole_prose(&document);
        assert_eq!(prose.text, "Before Ctrl\nAfter");
        assert!(!prose.text.contains("<kbd>"));
    }

    #[test]
    fn keeps_incomplete_streaming_markdown_readable() {
        let document = MarkdownDocument::parse("Working on **the answer");
        let text = document.plain_text();
        assert!(text.contains("Working on"));
        assert!(text.contains("the answer"));
    }

    #[test]
    fn parses_markdown_tables_as_structured_blocks() {
        let document = MarkdownDocument::parse(
            "Before\n\n| Component | Installed | Latest | Status |\n|:--|--:|:-:|:--|\n| Pi coding agent | 0.80.10 | 0.81.1 | Update available |\n| pi-bar | 0.3.39 | 0.3.39 | Current |\n\nAfter",
        );

        assert_eq!(document.blocks.len(), 3);
        match &document.blocks[0] {
            MarkdownBlock::Prose(prose) => assert_eq!(prose.text, "Before"),
            MarkdownBlock::Table(_) => panic!("expected leading prose"),
        }
        match &document.blocks[1] {
            MarkdownBlock::Table(table) => {
                assert_eq!(
                    table.headers,
                    vec!["Component", "Installed", "Latest", "Status"]
                );
                assert_eq!(table.rows.len(), 2);
                assert_eq!(table.rows[0][0], "Pi coding agent");
                assert_eq!(table.rows[0][1], "0.80.10");
                assert_eq!(table.rows[1][3], "Current");
                assert_eq!(
                    table.alignments,
                    vec![
                        TableAlign::Left,
                        TableAlign::Right,
                        TableAlign::Center,
                        TableAlign::Left
                    ]
                );
            }
            MarkdownBlock::Prose(_) => panic!("expected table block"),
        }
        match &document.blocks[2] {
            MarkdownBlock::Prose(prose) => assert_eq!(prose.text, "After"),
            MarkdownBlock::Table(_) => panic!("expected trailing prose"),
        }

        assert!(
            document
                .plain_text()
                .contains("Pi coding agent\t0.80.10\t0.81.1\tUpdate available")
        );
    }
}
