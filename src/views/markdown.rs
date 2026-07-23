use std::ops::Range;

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct MarkdownStyle {
    pub heading: bool,
    pub strong: bool,
    pub emphasis: bool,
    pub code: bool,
    pub link: bool,
    pub quote: bool,
    pub strikethrough: bool,
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
}

impl Default for DocumentBuilder {
    fn default() -> Self {
        Self {
            document: MarkdownDocument::default(),
            style: MarkdownStyle::default(),
            quote_depth: 0,
            list_stack: Vec::new(),
            at_line_start: true,
        }
    }
}

struct ListState {
    next_number: Option<u64>,
}

impl DocumentBuilder {
    fn handle(&mut self, event: Event<'_>) {
        match event {
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
            Tag::Table(_) => self.ensure_block_gap(),
            Tag::TableRow => self.ensure_line(),
            Tag::TableCell => {
                if !self.at_line_start {
                    self.push("  |  ", self.style);
                }
            }
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
            TagEnd::Item | TagEnd::TableRow => self.newline(),
            TagEnd::Emphasis => self.style.emphasis = false,
            TagEnd::Strong => self.style.strong = false,
            TagEnd::Strikethrough => self.style.strikethrough = false,
            TagEnd::Link => self.style.link = false,
            TagEnd::Table => self.ensure_block_gap(),
            _ => {}
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
}
