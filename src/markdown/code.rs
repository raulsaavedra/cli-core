use std::collections::HashMap;
use std::io::Cursor;
use std::sync::{Mutex, OnceLock};

use ratatui::style::{Color, Modifier, Style};
use syntect::easy::HighlightLines;
use syntect::highlighting::{FontStyle, Theme as Palette, ThemeSet};
use syntect::parsing::{SyntaxReference, SyntaxSet};

pub(super) type Tokens = Vec<(String, Style)>;
type Cache = Mutex<HashMap<(String, String), Option<Vec<Tokens>>>>;

const NIGHT_OWL: &[u8] = include_bytes!("../../assets/night-owl.tmTheme");
const CACHE_LIMIT: usize = 256;

fn syntaxes() -> &'static SyntaxSet {
    static SYNTAXES: OnceLock<SyntaxSet> = OnceLock::new();
    SYNTAXES.get_or_init(SyntaxSet::load_defaults_newlines)
}

fn palette() -> Option<&'static Palette> {
    static PALETTE: OnceLock<Option<Palette>> = OnceLock::new();
    PALETTE
        .get_or_init(|| ThemeSet::load_from_reader(&mut Cursor::new(NIGHT_OWL)).ok())
        .as_ref()
}

fn syntax(language: &str) -> Option<&'static SyntaxReference> {
    let normalized = language.trim().to_ascii_lowercase();
    let tokens: &[&str] = match normalized.as_str() {
        "" | "text" | "txt" | "plain" => return None,
        "rs" => &["rust"],
        "sh" | "zsh" | "shell" | "console" => &["bash"],
        "js" | "mjs" | "cjs" | "jsx" => &["javascript"],
        "ts" | "tsx" | "typescript" => &["typescript", "javascript"],
        "md" => &["markdown"],
        "yml" => &["yaml"],
        "py" => &["python"],
        "rb" => &["ruby"],
        other => return syntaxes().find_syntax_by_token(other),
    };
    tokens
        .iter()
        .find_map(|token| syntaxes().find_syntax_by_token(token))
}

pub(super) struct Highlighter {
    lines: HighlightLines<'static>,
}

impl Highlighter {
    pub fn new(language: &str) -> Option<Highlighter> {
        Some(Highlighter {
            lines: HighlightLines::new(syntax(language)?, palette()?),
        })
    }

    pub fn line(&mut self, text: &str) -> Option<Tokens> {
        let line = format!("{text}\n");
        let ranges = self.lines.highlight_line(&line, syntaxes()).ok()?;
        Some(
            ranges
                .into_iter()
                .map(|(style, piece)| (piece.trim_end_matches('\n').replace('\t', "    "), style_of(style)))
                .filter(|(piece, _)| !piece.is_empty())
                .collect(),
        )
    }
}

pub(super) fn highlight(language: &str, source: &str) -> Option<Vec<Tokens>> {
    static CACHE: OnceLock<Cache> = OnceLock::new();
    let cache = CACHE.get_or_init(Default::default);
    let key = (language.to_string(), source.to_string());
    if let Some(found) = cache.lock().ok().and_then(|cache| cache.get(&key).cloned()) {
        return found;
    }
    let lines = highlight_now(language, source);
    if let Ok(mut cache) = cache.lock() {
        if cache.len() >= CACHE_LIMIT {
            cache.clear();
        }
        cache.insert(key, lines.clone());
    }
    lines
}

fn highlight_now(language: &str, source: &str) -> Option<Vec<Tokens>> {
    let mut highlighter = Highlighter::new(language)?;
    source.lines().map(|line| highlighter.line(line)).collect()
}

fn style_of(style: syntect::highlighting::Style) -> Style {
    let color = style.foreground;
    let mut out = Style::default().fg(Color::Rgb(color.r, color.g, color.b));
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
    use super::{highlight, Highlighter};

    #[test]
    fn rust_gets_more_than_one_color() {
        let lines = highlight("rust", "fn main() {\n    let answer = 42;\n}").expect("rust highlights");
        assert_eq!(lines.len(), 3);
        let colors: std::collections::HashSet<_> = lines.iter().flatten().map(|(_, style)| style.fg).collect();
        assert!(colors.len() > 2, "{colors:?}");
        let text: String = lines[1].iter().map(|(piece, _)| piece.as_str()).collect();
        assert_eq!(text, "    let answer = 42;");
    }

    #[test]
    fn plain_and_unknown_languages_stay_unhighlighted() {
        assert!(highlight("text", "plain words").is_none());
        assert!(highlight("", "plain words").is_none());
        assert!(highlight("no-such-language", "plain words").is_none());
        assert!(highlight("ts", "const a = 1;").is_some());
        assert!(highlight("sh", "echo hi").is_some());
    }

    #[test]
    fn a_highlighter_carries_state_across_lines() {
        let mut highlighter = Highlighter::new("rust").unwrap();
        let open = highlighter.line("/* a comment").unwrap();
        let inside = highlighter.line("still inside */").unwrap();
        assert_eq!(open.last().map(|(_, style)| style.fg), inside.first().map(|(_, style)| style.fg));
    }
}
