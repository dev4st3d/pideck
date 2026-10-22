//! Language detection and per-line highlighting on top of GPUI Component's
//! tree-sitter `SyntaxHighlighter`, shared by the file editor and diff view.

use std::ops::Range;
use std::path::Path;

use gpui::HighlightStyle;
use gpui_component::highlighter::{HighlightTheme, SyntaxHighlighter};
use gpui_component::input::Rope;

/// Names a grammar bundled with GPUI Component's `tree-sitter-languages`
/// feature, or `None` for plain text.
pub(crate) fn language_for_path(path: &Path) -> Option<&'static str> {
    let name = path.file_name()?.to_str()?;
    if let Some(language) = language_for_file_name(name) {
        return Some(language);
    }
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    language_for_extension(&extension)
}

fn language_for_file_name(name: &str) -> Option<&'static str> {
    Some(match name {
        "Makefile" | "makefile" | "GNUmakefile" => "make",
        "CMakeLists.txt" => "cmake",
        "Cargo.lock" | "Pipfile" => "toml",
        "Gemfile" | "Rakefile" | "Guardfile" => "ruby",
        ".bashrc" | ".bash_profile" | ".bash_logout" | ".profile" | ".zshrc" => "bash",
        _ => return None,
    })
}

fn language_for_extension(extension: &str) -> Option<&'static str> {
    Some(match extension {
        "rs" => "rust",
        "js" | "mjs" | "cjs" | "jsx" => "javascript",
        "ts" | "mts" | "cts" => "typescript",
        "tsx" => "tsx",
        "json" | "jsonc" => "json",
        "py" | "pyi" | "pyw" => "python",
        "go" => "go",
        "c" | "h" => "c",
        "cpp" | "cc" | "cxx" | "hpp" | "hh" | "hxx" => "cpp",
        "cs" => "csharp",
        "java" => "java",
        "rb" | "rake" | "gemspec" => "ruby",
        "scala" => "scala",
        "swift" => "swift",
        "zig" => "zig",
        "ex" | "exs" => "elixir",
        "html" | "htm" => "html",
        "css" => "css",
        "md" | "markdown" => "markdown",
        "toml" => "toml",
        "yaml" | "yml" => "yaml",
        "sql" => "sql",
        "proto" => "proto",
        "graphql" | "gql" => "graphql",
        "sh" | "bash" | "zsh" => "bash",
        "mk" => "make",
        "cmake" => "cmake",
        "diff" | "patch" => "diff",
        "erb" => "erb",
        "ejs" => "ejs",
        _ => return None,
    })
}

/// Lines of one text version joined for parsing. A line may be absent (for
/// example, the old side of an added line) so line indexes stay aligned
/// with the caller's rows.
pub(crate) struct LineSource {
    text: String,
    lines: Vec<Option<Range<usize>>>,
}

impl LineSource {
    pub(crate) fn new<'a>(lines: impl IntoIterator<Item = Option<&'a str>>) -> Self {
        let mut text = String::new();
        let lines = lines
            .into_iter()
            .map(|line| {
                let line = line?;
                let start = text.len();
                text.push_str(line);
                let range = start..text.len();
                text.push('\n');
                Some(range)
            })
            .collect();
        Self { text, lines }
    }

    /// Parses the whole text; this is the expensive step, so callers run it
    /// off the UI thread.
    pub(crate) fn highlight(self, language: &str) -> LineHighlights {
        let mut highlighter = SyntaxHighlighter::new(language);
        highlighter.update(None, &Rope::from_str(&self.text));
        LineHighlights {
            highlighter,
            lines: self.lines,
        }
    }
}

pub(crate) struct LineHighlights {
    highlighter: SyntaxHighlighter,
    lines: Vec<Option<Range<usize>>>,
}

impl LineHighlights {
    /// Styled byte ranges relative to the start of `line`, in ascending order
    /// and without overlap. Unstyled stretches are omitted.
    pub(crate) fn styles(
        &self,
        line: usize,
        theme: &HighlightTheme,
    ) -> Vec<(Range<usize>, HighlightStyle)> {
        let Some(Some(range)) = self.lines.get(line) else {
            return Vec::new();
        };
        self.highlighter
            .styles(range, theme)
            .into_iter()
            .filter(|(_, style)| *style != HighlightStyle::default())
            .map(|(span, style)| (span.start - range.start..span.end - range.start, style))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui_component::highlighter::LanguageRegistry;

    #[test]
    fn languages_come_from_file_names_then_extensions() {
        let language = |path: &str| language_for_path(Path::new(path));
        assert_eq!(language("src/main.rs"), Some("rust"));
        assert_eq!(language("SRC/MAIN.RS"), Some("rust"));
        assert_eq!(language("web/app.tsx"), Some("tsx"));
        assert_eq!(language("config.jsonc"), Some("json"));
        assert_eq!(language("Makefile"), Some("make"));
        assert_eq!(language("build/CMakeLists.txt"), Some("cmake"));
        assert_eq!(language("Cargo.lock"), Some("toml"));
        assert_eq!(language("notes.txt"), None);
        assert_eq!(language("LICENSE"), None);
        assert_eq!(language("archive.tar.gz"), None);
    }

    #[test]
    fn every_detected_language_has_a_registered_grammar() {
        let names = [
            "Makefile",
            "CMakeLists.txt",
            "Cargo.lock",
            "Gemfile",
            ".bashrc",
            "a.rs",
            "a.js",
            "a.ts",
            "a.tsx",
            "a.json",
            "a.py",
            "a.go",
            "a.c",
            "a.cpp",
            "a.cs",
            "a.java",
            "a.rb",
            "a.scala",
            "a.swift",
            "a.zig",
            "a.ex",
            "a.html",
            "a.css",
            "a.md",
            "a.toml",
            "a.yaml",
            "a.sql",
            "a.proto",
            "a.graphql",
            "a.sh",
            "a.mk",
            "a.cmake",
            "a.diff",
            "a.erb",
            "a.ejs",
        ];
        for name in names {
            let language = language_for_path(Path::new(name)).expect(name);
            assert!(
                LanguageRegistry::singleton().language(language).is_some(),
                "{name}: {language} is not registered"
            );
        }
    }

    #[test]
    fn highlights_stay_relative_to_each_line_and_skip_missing_lines() {
        let source = LineSource::new([
            Some("// héllo"),
            None,
            Some("fn main() { let answer = 42; }"),
        ]);
        let highlights = source.highlight("rust");
        let theme = HighlightTheme::default_dark();

        assert!(highlights.styles(1, &theme).is_empty());
        assert!(highlights.styles(9, &theme).is_empty());

        let comment = highlights.styles(0, &theme);
        assert_eq!(comment.len(), 1);
        assert_eq!(comment[0].0, 0.."// héllo".len());

        let code = "fn main() { let answer = 42; }";
        let spans = highlights.styles(2, &theme);
        let keyword = spans
            .iter()
            .find(|(range, _)| &code[range.clone()] == "fn")
            .expect("`fn` is a keyword");
        let number = spans
            .iter()
            .find(|(range, _)| &code[range.clone()] == "42")
            .expect("`42` is a number");
        assert_ne!(keyword.1, number.1);
        assert!(
            spans
                .windows(2)
                .all(|pair| pair[0].0.end <= pair[1].0.start)
        );
        assert!(spans.iter().all(|(range, _)| range.end <= code.len()));
    }
}
