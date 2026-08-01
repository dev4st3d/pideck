//! Structured markdown parsing for the transcript.
//!
//! Block-level constructs (headings, code blocks, quotes, rules, lists,
//! tables) parse into dedicated blocks so the view can give each one real
//! layout — cards, bars, hanging indents — instead of flattened glyphs.
//! Inline constructs (bold, italic, code, links, strikethrough) remain
//! byte-range spans inside [`ProseBlock`]s, which keeps text shapes cheap and
//! lets the view drive selection with one span-sweep pass.

pub(super) mod render;

use std::ops::Range;

use pulldown_cmark::{Alignment, CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};

/// Inline emphasis only — block-level distinctions live in [`MarkdownBlock`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct MarkdownStyle {
    pub strong: bool,
    pub emphasis: bool,
    pub code: bool,
    pub link: bool,
    pub strikethrough: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct MarkdownSpan {
    pub range: Range<usize>,
    pub style: MarkdownStyle,
}

/// Inline-styled text run: the leaf unit the view shapes and selects.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(super) struct ProseBlock {
    pub text: String,
    pub spans: Vec<MarkdownSpan>,
}

impl ProseBlock {
    fn is_empty(&self) -> bool {
        self.text.is_empty()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ListMarker {
    Bullet,
    Ordered(u64),
    Task { checked: bool },
}

impl ListMarker {
    fn prefix(&self) -> String {
        match self {
            ListMarker::Bullet => "• ".to_owned(),
            ListMarker::Ordered(number) => format!("{number}. "),
            ListMarker::Task { checked } => if *checked { "☑ " } else { "☐ " }.to_owned(),
        }
    }
}

/// One list entry. `blocks` holds the item's content: usually a single
/// paragraph, but nested lists, code, or quotes stay structured.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct MarkdownListItem {
    pub marker: ListMarker,
    pub blocks: Vec<MarkdownBlock>,
}

impl MarkdownListItem {
    fn plain_text(&self) -> String {
        let body = blocks_plain_text(&self.blocks, "\n");
        let mut text = self.marker.prefix();
        text.push_str(&body);
        text
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum MarkdownBlock {
    /// One paragraph of inline-styled text.
    Prose(ProseBlock),
    Heading {
        level: u8,
        prose: ProseBlock,
    },
    Code {
        language: Option<String>,
        code: String,
    },
    Quote(Vec<MarkdownBlock>),
    Rule,
    List(Vec<MarkdownListItem>),
    Table(MarkdownTable),
}

impl MarkdownBlock {
    fn plain_text(&self) -> String {
        match self {
            MarkdownBlock::Prose(prose) => prose.text.clone(),
            MarkdownBlock::Heading { prose, .. } => prose.text.clone(),
            MarkdownBlock::Code { code, .. } => code.clone(),
            MarkdownBlock::Quote(blocks) => blocks_plain_text(blocks, "\n\n"),
            MarkdownBlock::Rule => String::new(),
            MarkdownBlock::List(items) => items
                .iter()
                .map(MarkdownListItem::plain_text)
                .collect::<Vec<_>>()
                .join("\n"),
            MarkdownBlock::Table(table) => table.to_plain_text(),
        }
    }
}

fn blocks_plain_text(blocks: &[MarkdownBlock], gap: &str) -> String {
    blocks
        .iter()
        .map(MarkdownBlock::plain_text)
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join(gap)
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
        self.headers
            .len()
            .max(self.rows.iter().map(Vec::len).max().unwrap_or(0))
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

fn join_row(cells: &[String], columns: usize) -> String {
    (0..columns)
        .map(|index| cells.get(index).map(String::as_str).unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\t")
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

    /// True when the document is one short paragraph (fast path with drag
    /// selection over a single shaped text).
    pub fn is_selectable_prose(&self) -> bool {
        matches!(self.blocks.as_slice(), [MarkdownBlock::Prose(_)])
    }

    pub fn sole_prose(&self) -> Option<&ProseBlock> {
        match self.blocks.as_slice() {
            [MarkdownBlock::Prose(prose)] => Some(prose),
            _ => None,
        }
    }

    /// Whole-document text used by copy-all and selection bookkeeping.
    pub fn plain_text(&self) -> String {
        blocks_plain_text(&self.blocks, "\n\n")
    }
}

/// Container frames mirror the event nesting: quotes and list items own a
/// child block vector; a list frame only counts its entries.
enum Frame {
    Quote(Vec<MarkdownBlock>),
    List {
        items: Vec<MarkdownListItem>,
        next_number: Option<u64>,
    },
    Item {
        marker: ListMarker,
        blocks: Vec<MarkdownBlock>,
    },
}

struct CodeSink {
    language: Option<String>,
    code: String,
}

#[derive(Default)]
struct DocumentBuilder {
    blocks: Vec<MarkdownBlock>,
    frames: Vec<Frame>,
    /// Paragraph sink; also collects list-item paragraphs (the item frame
    /// receives blocks on flush).
    prose: ProseBlock,
    heading: Option<(u8, ProseBlock)>,
    code: Option<CodeSink>,
    style: MarkdownStyle,
    table: Option<TableState>,
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
    /// Innermost container receiving finished blocks.
    fn current_blocks(&mut self) -> &mut Vec<MarkdownBlock> {
        for frame in self.frames.iter_mut().rev() {
            match frame {
                Frame::Quote(blocks) | Frame::Item { blocks, .. } => return blocks,
                // Stray content directly inside a list (no item) belongs to the
                // enclosing container per the markdown event model.
                Frame::List { .. } => {}
            }
        }
        &mut self.blocks
    }

    /// Active inline sink: heading text while a heading is open, otherwise the
    /// running paragraph.
    fn sink(&mut self) -> &mut ProseBlock {
        match &mut self.heading {
            Some((_, prose)) => prose,
            None => &mut self.prose,
        }
    }

    fn handle(&mut self, event: Event<'_>) {
        if self.handle_table_event(&event) {
            return;
        }

        match event {
            Event::Start(Tag::Table(alignments)) => {
                self.flush_prose();
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
            Event::Text(text) => {
                if let Some(code) = &mut self.code {
                    code.code.push_str(&text);
                } else {
                    self.push_text(&text, self.style);
                }
            }
            Event::Code(text) => {
                let mut style = self.style;
                style.code = true;
                self.push_text(&text, style);
            }
            Event::SoftBreak | Event::HardBreak => {
                if let Some(code) = &mut self.code {
                    code.code.push('\n');
                } else {
                    self.sink().text.push('\n');
                }
            }
            Event::Rule => {
                self.flush_prose();
                self.push_block(MarkdownBlock::Rule);
            }
            Event::TaskListMarker(checked) => {
                if let Some(Frame::Item { marker, .. }) = self.frames.last_mut() {
                    *marker = ListMarker::Task { checked };
                }
            }
            Event::InlineMath(text) => {
                let mut style = self.style;
                style.code = true;
                self.push_text(&text, style);
            }
            Event::DisplayMath(text) => {
                self.flush_prose();
                self.push_block(MarkdownBlock::Code {
                    language: Some("math".to_owned()),
                    code: text.trim().to_owned(),
                });
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
            Tag::Paragraph => {}
            Tag::Heading { level, .. } => {
                self.flush_prose();
                self.heading = Some((heading_level(level), ProseBlock::default()));
            }
            Tag::BlockQuote(_) => {
                self.flush_prose();
                self.frames.push(Frame::Quote(Vec::new()));
            }
            Tag::CodeBlock(kind) => {
                self.flush_prose();
                let language = match kind {
                    CodeBlockKind::Fenced(language) => {
                        let language = language.trim();
                        (!language.is_empty()).then(|| language.to_owned())
                    }
                    CodeBlockKind::Indented => None,
                };
                self.code = Some(CodeSink {
                    language,
                    code: String::new(),
                });
            }
            Tag::List(next_number) => {
                self.flush_prose();
                self.frames.push(Frame::List {
                    items: Vec::new(),
                    next_number,
                });
            }
            Tag::Item => {
                self.flush_prose();
                let marker = self
                    .frames
                    .iter_mut()
                    .rev()
                    .find(|frame| matches!(frame, Frame::List { .. }))
                    .and_then(|frame| match frame {
                        Frame::List { next_number, .. } => next_number.as_mut().map(|number| {
                            let marker = ListMarker::Ordered(*number);
                            *number = number.saturating_add(1);
                            marker
                        }),
                        _ => None,
                    })
                    .unwrap_or(ListMarker::Bullet);
                self.frames.push(Frame::Item {
                    marker,
                    blocks: Vec::new(),
                });
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
            TagEnd::Paragraph => self.flush_prose(),
            TagEnd::Heading(_) => {
                if let Some((level, prose)) = self.heading.take() {
                    self.push_block(MarkdownBlock::Heading { level, prose });
                }
            }
            TagEnd::BlockQuote(_) => {
                self.flush_prose();
                if let Some(Frame::Quote(blocks)) = self.frames.pop() {
                    if !blocks.is_empty() {
                        self.push_block(MarkdownBlock::Quote(blocks));
                    }
                }
            }
            TagEnd::CodeBlock => {
                if let Some(sink) = self.code.take() {
                    self.push_block(MarkdownBlock::Code {
                        language: sink.language,
                        code: sink.code.trim_end_matches('\n').to_owned(),
                    });
                }
            }
            TagEnd::List(_) => {
                self.flush_prose();
                if let Some(Frame::List { items, .. }) = self.frames.pop() {
                    if !items.is_empty() {
                        self.push_block(MarkdownBlock::List(items));
                    }
                }
            }
            TagEnd::Item => {
                self.flush_prose();
                if let Some(Frame::Item { marker, blocks }) = self.frames.pop() {
                    for frame in self.frames.iter_mut().rev() {
                        if let Frame::List { items, .. } = frame {
                            items.push(MarkdownListItem { marker, blocks });
                            break;
                        }
                    }
                }
            }
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

        self.push_block(MarkdownBlock::Table(MarkdownTable {
            alignments: table.alignments.into_iter().map(TableAlign::from).collect(),
            headers,
            rows,
        }));
    }

    /// Close the running paragraph into the current container, dropping
    /// trailing soft whitespace and dead spans.
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

        if self.prose.is_empty() {
            self.prose = ProseBlock::default();
            return;
        }

        let prose = std::mem::take(&mut self.prose);
        self.current_blocks().push(MarkdownBlock::Prose(prose));
    }

    fn push_block(&mut self, block: MarkdownBlock) {
        self.current_blocks().push(block);
    }

    /// Split source text on raw newlines; codes/embedded line feeds show up in
    /// pasted content even though pulldown usually reports breaks separately.
    fn push_text(&mut self, text: &str, style: MarkdownStyle) {
        for (index, line) in text.split('\n').enumerate() {
            if index > 0 {
                self.sink().text.push('\n');
            }
            self.push(line, style);
        }
    }

    fn handle_html(&mut self, html: &str) {
        let normalized = html.trim().to_ascii_lowercase();
        if normalized.starts_with("<br") {
            self.sink().text.push('\n');
        } else if normalized.starts_with("</p")
            || normalized.starts_with("</div")
            || normalized.starts_with("</details")
            || normalized.starts_with("</summary")
        {
            self.flush_prose();
        }
    }

    fn push(&mut self, text: &str, style: MarkdownStyle) {
        if text.is_empty() {
            return;
        }
        let sink = self.sink();
        let start = sink.text.len();
        sink.text.push_str(text);
        let end = sink.text.len();

        if style != MarkdownStyle::default() {
            if let Some(last) = sink.spans.last_mut()
                && last.range.end == start
                && last.style == style
            {
                last.range.end = end;
                return;
            }
            sink.spans.push(MarkdownSpan {
                range: start..end,
                style,
            });
        }
    }

    fn finish(mut self) -> MarkdownDocument {
        // Unclosed constructs in streamed output still render as their block.
        if let Some(sink) = self.code.take() {
            let code = sink.code.trim_end_matches('\n').to_owned();
            if !code.is_empty() {
                self.current_blocks().push(MarkdownBlock::Code {
                    language: sink.language,
                    code,
                });
            }
        }
        if let Some((level, mut prose)) = self.heading.take() {
            trim_prose_end(&mut prose);
            if !prose.is_empty() {
                self.current_blocks()
                    .push(MarkdownBlock::Heading { level, prose });
            }
        }
        self.flush_prose();
        while let Some(frame) = self.frames.pop() {
            match frame {
                Frame::Quote(blocks) if !blocks.is_empty() => {
                    self.push_block(MarkdownBlock::Quote(blocks));
                }
                Frame::List { items, .. } if !items.is_empty() => {
                    self.push_block(MarkdownBlock::List(items));
                }
                Frame::Item { marker, blocks } => {
                    let target = self.frames.iter_mut().rev().find_map(|frame| match frame {
                        Frame::List { items, .. } => Some(items),
                        _ => None,
                    });
                    if let Some(items) = target {
                        items.push(MarkdownListItem { marker, blocks });
                    } else {
                        // Defensive: an unclosed item without its list keeps
                        // its content at the enclosing container level.
                        self.current_blocks().extend(blocks);
                    }
                }
                _ => {}
            }
        }
        MarkdownDocument {
            blocks: self.blocks,
        }
    }
}

fn trim_prose_end(prose: &mut ProseBlock) {
    while prose.text.ends_with('\n') {
        prose.text.pop();
    }
    prose
        .spans
        .retain(|span| span.range.start < prose.text.len());
    for span in &mut prose.spans {
        span.range.end = span.range.end.min(prose.text.len());
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
    fn inline_styles_become_spans_in_a_paragraph() {
        let document = MarkdownDocument::parse(
            "Use **bold**, *care*, `code`, and [docs](https://example.com), ~~gone~~.",
        );
        let prose = sole_prose(&document);

        assert_eq!(prose.text, "Use bold, care, code, and docs, gone.");
        let has = |needle: &str, probe: fn(&MarkdownStyle) -> bool| {
            prose
                .spans
                .iter()
                .any(|span| &prose.text[span.range.clone()] == needle && probe(&span.style))
        };
        assert!(has("bold", |style| style.strong));
        assert!(has("care", |style| style.emphasis));
        assert!(has("code", |style| style.code));
        assert!(has("docs", |style| style.link));
        assert!(has("gone", |style| style.strikethrough));
    }

    #[test]
    fn headings_are_structured_blocks_with_levels() {
        let document = MarkdownDocument::parse("# Primary\n\n### Detail\n\nBody text.");

        assert_eq!(document.blocks.len(), 3);
        match &document.blocks[0] {
            MarkdownBlock::Heading { level, prose } => {
                assert_eq!(*level, 1);
                assert_eq!(prose.text, "Primary");
            }
            block => panic!("expected heading, got {block:?}"),
        }
        match &document.blocks[1] {
            MarkdownBlock::Heading { level, prose } => {
                assert_eq!(*level, 3);
                assert_eq!(prose.text, "Detail");
            }
            block => panic!("expected heading, got {block:?}"),
        }
        assert!(matches!(&document.blocks[2], MarkdownBlock::Prose(_)));
        assert_eq!(document.plain_text(), "Primary\n\nDetail\n\nBody text.");
    }

    #[test]
    fn code_blocks_keep_language_and_raw_text() {
        let document = MarkdownDocument::parse(
            "Before\n\n```rust\nfn main() {\n    println!(\"ok\");\n}\n```\n\nInline `code`.",
        );

        assert_eq!(document.blocks.len(), 3);
        match &document.blocks[1] {
            MarkdownBlock::Code { language, code } => {
                assert_eq!(language.as_deref(), Some("rust"));
                assert_eq!(code, "fn main() {\n    println!(\"ok\");\n}");
            }
            block => panic!("expected code block, got {block:?}"),
        }
    }

    #[test]
    fn unclosed_fence_streams_as_a_code_block() {
        let document = MarkdownDocument::parse("Working:\n\n```ts\nconst answer = 42;");

        assert!(matches!(
            &document.blocks[1],
            MarkdownBlock::Code { language, code }
                if language.as_deref() == Some("ts") && code == "const answer = 42;"
        ));
    }

    #[test]
    fn quotes_nest_blocks_recursively() {
        let document = MarkdownDocument::parse("> Think **carefully**.\n>\n> More.\n\nOutside.");

        assert_eq!(document.blocks.len(), 2);
        match &document.blocks[0] {
            MarkdownBlock::Quote(blocks) => {
                assert_eq!(blocks.len(), 2);
                match &blocks[0] {
                    MarkdownBlock::Prose(prose) => {
                        assert_eq!(prose.text, "Think carefully.");
                        assert!(prose.spans.iter().any(|span| {
                            &prose.text[span.range.clone()] == "carefully" && span.style.strong
                        }));
                    }
                    block => panic!("expected quote prose, got {block:?}"),
                }
            }
            block => panic!("expected quote, got {block:?}"),
        }
        assert_eq!(
            document.plain_text(),
            "Think carefully.\n\nMore.\n\nOutside."
        );
    }

    #[test]
    fn lists_carry_markers_and_nested_structure() {
        let document =
            MarkdownDocument::parse("1. First\n2. Second\n   - nested\n\n- [x] Done\n- [ ] Next");

        assert_eq!(document.blocks.len(), 2);
        match &document.blocks[0] {
            MarkdownBlock::List(items) => {
                assert_eq!(items.len(), 2);
                assert_eq!(items[0].marker, ListMarker::Ordered(1));
                assert_eq!(items[1].marker, ListMarker::Ordered(2));
                match &items[1].blocks[1] {
                    MarkdownBlock::List(nested) => {
                        assert_eq!(nested[0].marker, ListMarker::Bullet);
                    }
                    block => panic!("expected nested list, got {block:?}"),
                }
            }
            block => panic!("expected list, got {block:?}"),
        }
        match &document.blocks[1] {
            MarkdownBlock::List(items) => {
                assert_eq!(items[0].marker, ListMarker::Task { checked: true });
                assert_eq!(items[1].marker, ListMarker::Task { checked: false });
            }
            block => panic!("expected task list, got {block:?}"),
        }
        assert_eq!(
            document.plain_text(),
            "1. First\n2. Second\n• nested\n\n☑ Done\n☐ Next"
        );
    }

    #[test]
    fn rule_is_a_structured_block_skipped_in_plain_text() {
        let document = MarkdownDocument::parse("Above\n\n---\n\nBelow");

        assert_eq!(document.blocks.len(), 3);
        assert!(matches!(&document.blocks[1], MarkdownBlock::Rule));
        assert_eq!(document.plain_text(), "Above\n\nBelow");
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
            block => panic!("expected leading prose, got {block:?}"),
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
            block => panic!("expected table block, got {block:?}"),
        }
        match &document.blocks[2] {
            MarkdownBlock::Prose(prose) => assert_eq!(prose.text, "After"),
            block => panic!("expected trailing prose, got {block:?}"),
        }

        assert!(
            document
                .plain_text()
                .contains("Pi coding agent\t0.80.10\t0.81.1\tUpdate available")
        );
    }
}
