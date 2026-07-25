//! Syntax highlighting for fenced code blocks.
//!
//! Wraps [`syntect`] so `render/markdown.rs` can colour code by language.
//! The heavy defaults (syntax + theme dumps) are loaded exactly once behind
//! `OnceLock`; a highlighter is then cheap to spin up per fence.
//!
//! Highlighting is stateful across the lines of a single block (a `{` opened
//! on one line affects the next), so callers build one [`CodeHighlighter`]
//! per fence and feed it lines in order.
//!
//! Everything degrades gracefully: an unknown language yields `None` from
//! [`CodeHighlighter::for_language`] and the caller falls back to plain text.

use std::sync::OnceLock;

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;
use syntect::easy::HighlightLines;
use syntect::highlighting::{FontStyle, Style as SynStyle, Theme, ThemeSet};
use syntect::parsing::{SyntaxReference, SyntaxSet};

static SYNTAX_SET: OnceLock<SyntaxSet> = OnceLock::new();
static THEME: OnceLock<Theme> = OnceLock::new();

/// Bundled syntax definitions. We split markdown on `\n` and feed lines
/// *without* the trailing newline, so the no-newlines variant is correct.
fn syntax_set() -> &'static SyntaxSet {
    SYNTAX_SET.get_or_init(SyntaxSet::load_defaults_nonewlines)
}

/// A dark theme that reads well on the terminal's (assumed dark) background.
/// We only ever use the foreground colours, so the theme's own background is
/// intentionally ignored — code blocks blend with the surrounding transcript.
fn theme() -> &'static Theme {
    THEME.get_or_init(|| {
        let mut set = ThemeSet::load_defaults();
        set.themes
            .remove("base16-ocean.dark")
            .or_else(|| set.themes.values().next().cloned())
            .expect("syntect ships at least one default theme")
    })
}

/// Map the language tag on a code fence to a token `syntect` recognises.
///
/// LLM output writes fences like ```` ```rust ```` or ```` ```python ````;
/// `syntect` indexes syntaxes by file extension (`rs`, `py`) and exact name
/// (`Rust`), so we normalise the common aliases to an extension first.
fn normalize_lang(lang: &str) -> &str {
    match lang.trim().to_ascii_lowercase().as_str() {
        "rust" => "rs",
        "python" => "py",
        "javascript" | "node" | "mjs" | "cjs" => "js",
        "typescript" => "ts",
        "bash" | "shell" | "zsh" | "console" => "sh",
        "yml" => "yaml",
        "golang" => "go",
        "c++" | "cxx" | "cc" => "cpp",
        "c#" | "csharp" => "cs",
        "rb" | "ruby" => "rb",
        "markdown" => "md",
        "plaintext" | "text" | "txt" => "txt",
        // Anything else: pass the (trimmed) tag straight through — `syntect`
        // resolves the majority of extension/name tokens on its own.
        _ => lang.trim(),
    }
}

fn find_syntax(set: &'static SyntaxSet, lang: &str) -> Option<&'static SyntaxReference> {
    let token = normalize_lang(lang);
    if token.is_empty() {
        return None;
    }
    set.find_syntax_by_token(token)
        .or_else(|| set.find_syntax_by_extension(token))
        .or_else(|| set.find_syntax_by_name(token))
}

/// A per-fence highlighter. Cheap to build; holds the running parse state so
/// consecutive lines highlight in context.
pub struct CodeHighlighter {
    inner: HighlightLines<'static>,
}

impl CodeHighlighter {
    /// Build a highlighter for `lang`, or `None` when the language isn't
    /// recognised (caller should fall back to plain rendering).
    #[must_use]
    pub fn for_language(lang: &str) -> Option<Self> {
        let set = syntax_set();
        let syntax = find_syntax(set, lang)?;
        Some(Self {
            inner: HighlightLines::new(syntax, theme()),
        })
    }

    /// Highlight one line of code into styled spans. Returns `None` if the
    /// underlying parser errors, so the caller can fall back to plain text.
    #[must_use]
    pub fn highlight(&mut self, line: &str) -> Option<Vec<Span<'static>>> {
        let ranges = self.inner.highlight_line(line, syntax_set()).ok()?;
        Some(
            ranges
                .into_iter()
                .map(|(style, text)| Span::styled(text.to_string(), convert_style(style)))
                .collect(),
        )
    }
}

/// Convert a `syntect` style to a Ratatui style. Foreground colour and the
/// bold/italic/underline flags carry over; background is dropped so blocks
/// sit flush with the transcript background.
fn convert_style(style: SynStyle) -> Style {
    let fg = style.foreground;
    let mut out = Style::default().fg(Color::Rgb(fg.r, fg.g, fg.b));
    if style.font_style.contains(FontStyle::BOLD) {
        out = out.add_modifier(Modifier::BOLD);
    }
    if style.font_style.contains(FontStyle::ITALIC) {
        out = out.add_modifier(Modifier::ITALIC);
    }
    if style.font_style.contains(FontStyle::UNDERLINE) {
        out = out.add_modifier(Modifier::UNDERLINED);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_language_builds_highlighter() {
        assert!(CodeHighlighter::for_language("rust").is_some());
        assert!(CodeHighlighter::for_language("rs").is_some());
        assert!(CodeHighlighter::for_language("python").is_some());
        assert!(CodeHighlighter::for_language("json").is_some());
    }

    #[test]
    fn unknown_language_yields_none() {
        assert!(CodeHighlighter::for_language("definitely-not-a-language").is_none());
        assert!(CodeHighlighter::for_language("").is_none());
    }

    #[test]
    fn highlight_splits_into_multiple_styled_spans() {
        let mut h = CodeHighlighter::for_language("rust").unwrap();
        let spans = h.highlight("fn main() {}").unwrap();
        // Keywords vs identifiers vs punctuation should yield >1 span, and
        // the concatenated text must round-trip the input exactly.
        assert!(
            spans.len() > 1,
            "expected multiple spans, got {}",
            spans.len()
        );
        let joined: String = spans.iter().map(|s| s.content.as_ref()).collect();
        assert_eq!(joined, "fn main() {}");
    }

    #[test]
    fn highlight_preserves_content_across_lines() {
        let mut h = CodeHighlighter::for_language("rust").unwrap();
        let l1: String = h
            .highlight("let x = \"unterminated")
            .unwrap()
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        let l2: String = h
            .highlight("still string\";")
            .unwrap()
            .iter()
            .map(|s| s.content.to_string())
            .collect();
        assert_eq!(l1, "let x = \"unterminated");
        assert_eq!(l2, "still string\";");
    }

    #[test]
    fn empty_line_highlights_to_empty() {
        let mut h = CodeHighlighter::for_language("rust").unwrap();
        let joined: String = h
            .highlight("")
            .unwrap()
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        assert_eq!(joined, "");
    }
}
