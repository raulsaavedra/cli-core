use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::OnceLock;

use syntect::easy::HighlightLines;
use syntect::highlighting::{Theme, ThemeSet};
use syntect::parsing::{SyntaxReference, SyntaxSet};
use syntect::util::as_24_bit_terminal_escaped;
use unicode_width::UnicodeWidthStr;

const OCCURRENCE_MARKER_START: char = '\u{1e}';
const OCCURRENCE_MARKER_END: char = '\u{1f}';
const OCCURRENCE_LINK: char = 'L';
const OCCURRENCE_FILE_REF: char = 'F';
const OCCURRENCE_START: char = 'S';
const OCCURRENCE_END: char = 'E';

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// A heading extracted from the markdown.
#[derive(Debug, Clone)]
pub struct Heading {
    pub level: usize,
    pub text: String,
    /// Zero-based line index in the rendered plain-text output.
    pub line: usize,
}

/// A link extracted from the markdown.
#[derive(Debug, Clone)]
pub struct Link {
    pub text: String,
    pub href: String,
}

/// A rendered occurrence of a link in the markdown output.
#[derive(Debug, Clone)]
pub struct LinkOccurrence {
    pub link: Link,
    /// Zero-based line index in the rendered plain-text output.
    pub line: usize,
    /// Zero-based display column where the rendered link label starts.
    pub start_col: usize,
    /// Zero-based display column immediately after the rendered link label.
    pub end_col: usize,
}

/// A file reference extracted from the markdown and rendered as a
/// jump-to-editor chip. Authored as `@file:path[:line[:col]]` (bare) or
/// `@file[label](path[:line[:col]])` (labeled). Interactive viewers open it.
#[derive(Debug, Clone)]
pub struct FileRef {
    /// Path as authored; may be absolute or relative.
    pub path: String,
    pub line: Option<usize>,
    pub col: Option<usize>,
    /// Explicit display label from the `@file[label](...)` form.
    pub label: Option<String>,
}

/// A rendered occurrence of a file reference in the markdown output.
#[derive(Debug, Clone)]
pub struct FileRefOccurrence {
    pub file_ref: FileRef,
    /// Zero-based line index in the rendered plain-text output.
    pub line: usize,
    /// Zero-based display column where the rendered file-ref chip starts.
    pub start_col: usize,
    /// Zero-based display column immediately after the rendered file-ref chip.
    pub end_col: usize,
}

impl FileRef {
    /// Display text for chips and pickers: the explicit label, else the file
    /// basename with `:line` when present.
    pub fn display(&self) -> String {
        if let Some(label) = &self.label {
            return label.clone();
        }
        let base = self
            .path
            .rsplit(['/', '\\'])
            .next()
            .filter(|s| !s.is_empty())
            .unwrap_or(&self.path);
        match self.line {
            Some(line) => format!("{base}:{line}"),
            None => base.to_string(),
        }
    }
}

/// Result of rendering markdown for terminal display.
#[derive(Debug, Clone)]
pub struct RenderResult {
    /// ANSI-styled rendered markdown joined by newlines.
    pub rendered: String,
    /// Individual rendered lines (may contain ANSI).
    pub lines: Vec<String>,
    /// Plain-text lines (ANSI stripped, trailing spaces trimmed).
    pub plain: Vec<String>,
    pub headings: Vec<Heading>,
    pub links: Vec<Link>,
    pub link_occurrences: Vec<LinkOccurrence>,
    pub file_refs: Vec<FileRef>,
    pub file_ref_occurrences: Vec<FileRefOccurrence>,
}

// ---------------------------------------------------------------------------
// ANSI helpers
// ---------------------------------------------------------------------------

/// Strip all ANSI escape sequences from a string.
fn strip_ansi(s: &str) -> String {
    strip_ansi_inner(s, false)
}

fn strip_ansi_preserving_occurrence_markers(s: &str) -> String {
    strip_ansi_inner(s, true)
}

fn strip_ansi_inner(s: &str, preserve_occurrence_markers: bool) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.char_indices().peekable();
    while let Some((_i, ch)) = chars.next() {
        if ch == OCCURRENCE_MARKER_START {
            if preserve_occurrence_markers {
                result.push(ch);
            }
            let mut complete = false;
            while let Some((_, c)) = chars.next() {
                if preserve_occurrence_markers {
                    result.push(c);
                }
                if c == OCCURRENCE_MARKER_END {
                    complete = true;
                    break;
                }
            }
            if !preserve_occurrence_markers && !complete {
                result.push(ch);
            }
        } else if ch == '\x1b' {
            // Check if next char is '['
            if let Some(&(_, next_ch)) = chars.peek() {
                if next_ch == '[' {
                    chars.next(); // skip '['
                                  // Skip until 'm'
                    while let Some((_, c)) = chars.next() {
                        if c == 'm' {
                            break;
                        }
                    }
                    continue;
                }
            }
            result.push(ch);
        } else {
            result.push(ch);
        }
    }
    result
}

/// Compute the visible (display) width of a string, ignoring ANSI escapes.
fn visible_width(s: &str) -> usize {
    UnicodeWidthStr::width(strip_ansi(s).as_str())
}

fn occurrence_marker(kind: char, edge: char, index: usize) -> String {
    format!("{OCCURRENCE_MARKER_START}{kind}{edge}{index}{OCCURRENCE_MARKER_END}")
}

fn wrap_occurrence_marker(kind: char, index: usize, rendered: String) -> String {
    format!(
        "{}{}{}",
        occurrence_marker(kind, OCCURRENCE_START, index),
        rendered,
        occurrence_marker(kind, OCCURRENCE_END, index)
    )
}

/// Check if a line is visually blank (only whitespace after ANSI stripping).
fn is_blank_line(line: &str) -> bool {
    strip_ansi(line).trim().is_empty()
}

/// Truncate a string to at most `max` visible characters, preserving ANSI codes.
fn truncate_to_width(s: &str, max: usize) -> String {
    let mut vis = 0;
    let mut out = String::new();
    let mut chars = s.char_indices().peekable();
    while let Some((_i, ch)) = chars.next() {
        if ch == OCCURRENCE_MARKER_START {
            out.push(ch);
            while let Some((_, c)) = chars.next() {
                out.push(c);
                if c == OCCURRENCE_MARKER_END {
                    break;
                }
            }
            continue;
        }
        if ch == '\x1b' {
            // Check if next char is '['
            if let Some(&(_, next_ch)) = chars.peek() {
                if next_ch == '[' {
                    out.push(ch);
                    let (_, bracket) = chars.next().unwrap();
                    out.push(bracket);
                    // Copy until 'm'
                    while let Some((_, c)) = chars.next() {
                        out.push(c);
                        if c == 'm' {
                            break;
                        }
                    }
                    continue;
                }
            }
        }
        if vis >= max {
            break;
        }
        out.push(ch);
        vis += 1;
    }
    out
}

// ---------------------------------------------------------------------------
// ANSI formatting helpers (raw escape codes, matching chalk output)
// ---------------------------------------------------------------------------

fn ansi_bold(text: &str) -> String {
    format!("\x1b[1m{text}\x1b[22m")
}

fn ansi_italic(text: &str) -> String {
    format!("\x1b[3m{text}\x1b[23m")
}

fn ansi_underline(text: &str) -> String {
    format!("\x1b[4m{text}\x1b[24m")
}

fn ansi_dim(text: &str) -> String {
    format!("\x1b[2m{text}\x1b[22m")
}

fn ansi_strikethrough(text: &str) -> String {
    format!("\x1b[9m{text}\x1b[29m")
}

fn ansi_cyan(text: &str) -> String {
    format!("\x1b[36m{text}\x1b[39m")
}

fn ansi_256(color: u8, text: &str) -> String {
    format!("\x1b[38;5;{color}m{text}\x1b[39m")
}

fn ansi_bold_underline_256(color: u8, text: &str) -> String {
    format!("\x1b[1m\x1b[4m\x1b[38;5;{color}m{text}\x1b[39m\x1b[24m\x1b[22m")
}

fn ansi_bold_256(color: u8, text: &str) -> String {
    format!("\x1b[1m\x1b[38;5;{color}m{text}\x1b[39m\x1b[22m")
}

// ---------------------------------------------------------------------------
// Code block highlighting
// ---------------------------------------------------------------------------

static SYNTAX_SET: OnceLock<SyntaxSet> = OnceLock::new();
static THEME_SET: OnceLock<ThemeSet> = OnceLock::new();
static LOCAL_HIGHLIGHT_THEME: OnceLock<Option<Theme>> = OnceLock::new();

fn syntax_set() -> &'static SyntaxSet {
    SYNTAX_SET.get_or_init(SyntaxSet::load_defaults_newlines)
}

fn theme_set() -> &'static ThemeSet {
    THEME_SET.get_or_init(ThemeSet::load_defaults)
}

fn local_highlight_theme() -> &'static Option<Theme> {
    LOCAL_HIGHLIGHT_THEME.get_or_init(|| {
        let home = std::env::var_os("HOME").map(PathBuf::from)?;
        [
            home.join(".config/yazi/night-owl.tmTheme"),
            home.join("src/config/yazi/night-owl.tmTheme"),
            home.join("src/config/bat/themes/night-owl.tmTheme"),
        ]
        .into_iter()
        .find_map(|path| ThemeSet::get_theme(path).ok())
    })
}

fn highlight_theme() -> Option<&'static Theme> {
    if let Some(theme) = local_highlight_theme().as_ref() {
        return Some(theme);
    }

    let themes = &theme_set().themes;
    themes
        .get("base16-ocean.dark")
        .or_else(|| themes.get("InspiredGitHub"))
        .or_else(|| themes.values().next())
}

fn is_plain_text_code_lang(lang: &str) -> bool {
    let normalized = lang.trim().to_ascii_lowercase();
    matches!(normalized.as_str(), "text" | "txt" | "plain")
}

fn is_math_code_lang(lang: &str) -> bool {
    let normalized = lang.trim().to_ascii_lowercase();
    matches!(normalized.as_str(), "math" | "equation" | "equations")
}

fn syntax_for_lang<'a>(lang: &str, syntaxes: &'a SyntaxSet) -> Option<&'a SyntaxReference> {
    let normalized = lang.trim().to_ascii_lowercase();
    let candidate = match normalized.as_str() {
        "rs" => "rust",
        "sh" | "zsh" | "shell" => "bash",
        "js" | "mjs" | "cjs" => "javascript",
        "ts" | "tsx" => "typescript",
        "md" => "markdown",
        "yml" => "yaml",
        "" => return None,
        other => other,
    };

    syntaxes.find_syntax_by_token(candidate)
}

fn highlighted_code_lines(lang: &str, code: &str) -> Option<Vec<String>> {
    let syntaxes = syntax_set();
    let syntax = syntax_for_lang(lang, syntaxes)?;
    let theme = highlight_theme()?;
    let mut highlighter = HighlightLines::new(syntax, theme);
    let mut lines = Vec::new();

    for code_line in code.split('\n') {
        let ranges = highlighter.highlight_line(code_line, syntaxes).ok()?;
        lines.push(format!(
            "{}\x1b[0m",
            as_24_bit_terminal_escaped(&ranges, false)
        ));
    }

    Some(lines)
}

fn render_math_lines(code: &str) -> Vec<String> {
    let plans: Vec<MathLinePlan> = code.split('\n').map(plan_math_line).collect();
    let reason_columns = math_reason_columns(&plans);

    plans
        .into_iter()
        .zip(reason_columns)
        .map(|(plan, reason_column)| format!("  {}", render_math_plan(plan, reason_column)))
        .collect()
}

enum MathLinePlan {
    Rendered(String),
    WithReason { main: String, reason: String },
}

fn plan_math_line(line: &str) -> MathLinePlan {
    let trimmed = line.trim_start();
    let leading_len = line.len() - trimmed.len();
    let leading = &line[..leading_len];

    for label in ["Answer:", "Result:", "Solution:"] {
        if let Some(rest) = trimmed.strip_prefix(label) {
            return MathLinePlan::Rendered(format!(
                "{leading}{}{}",
                ansi_bold_256(114, label),
                render_math_tokens(rest, MathTone::Answer)
            ));
        }
    }

    if trimmed.is_empty() {
        return MathLinePlan::Rendered(String::new());
    }

    if split_math_control_label(trimmed).is_some() {
        return MathLinePlan::Rendered(format!("{leading}{}", render_math_segments(trimmed)));
    }

    if let Some((core, note)) = split_trailing_note(trimmed) {
        return MathLinePlan::WithReason {
            main: format!("{leading}{}", render_math_chain(core.trim_end(), true)),
            reason: render_math_tokens(note.trim_start(), MathTone::Reason),
        };
    }

    MathLinePlan::Rendered(format!("{leading}{}", render_math_segments(trimmed)))
}

fn math_reason_columns(plans: &[MathLinePlan]) -> Vec<Option<usize>> {
    let mut columns = vec![None; plans.len()];
    let mut idx = 0;

    while idx < plans.len() {
        let MathLinePlan::WithReason { .. } = &plans[idx] else {
            idx += 1;
            continue;
        };

        let start = idx;
        let mut column = 0;
        while let Some(MathLinePlan::WithReason { main, .. }) = plans.get(idx) {
            column = column.max(visible_width(main));
            idx += 1;
        }

        for slot in columns.iter_mut().take(idx).skip(start) {
            *slot = Some(column);
        }
    }

    columns
}

fn render_math_plan(plan: MathLinePlan, reason_column: Option<usize>) -> String {
    match plan {
        MathLinePlan::Rendered(rendered) => rendered,
        MathLinePlan::WithReason { main, reason } => {
            let column = reason_column.unwrap_or_else(|| visible_width(&main));
            let gap = column.saturating_sub(visible_width(&main)) + 4;
            format!("{main}{}{reason}", " ".repeat(gap))
        }
    }
}

#[derive(Clone, Copy)]
enum MathTone {
    Expression,
    Reason,
    Annotation,
    Answer,
}

fn render_math_segments(text: &str) -> String {
    if let Some((label, after)) = split_math_control_label(text) {
        return format!(
            "{}{}{}",
            render_math_label(label.trim()),
            ansi_256(244, ": "),
            render_math_annotation(after.trim_start())
        );
    }

    if let Some((before, after)) = split_expression_annotation(text) {
        return format!(
            "{}{}{}",
            render_math_chain(before.trim_end(), true),
            "   ",
            render_math_annotation(after.trim_start())
        );
    }

    if looks_like_prose_math_line(text) {
        return render_math_tokens(text, MathTone::Reason);
    }

    if has_math_syntax(text) {
        render_math_chain(text, should_pop_final_math(text))
    } else {
        ansi_dim(text)
    }
}

fn render_math_label(label: &str) -> String {
    ansi_bold_256(81, label)
}

fn render_math_annotation(text: &str) -> String {
    if let Some((before, marker, after)) = split_last_relation(text) {
        let (answer, tail) = split_answer_tail(after);
        return format!(
            "{}{}{}{}",
            render_math_tokens(before, MathTone::Annotation),
            ansi_bold_256(114, marker),
            render_math_tokens(answer, MathTone::Answer),
            render_math_tokens(tail, MathTone::Annotation)
        );
    }

    render_math_tokens(text, MathTone::Annotation)
}

fn render_math_chain(text: &str, pop_final: bool) -> String {
    if pop_final {
        if let Some((before, marker, after)) = split_last_relation(text) {
            let (answer, tail) = split_answer_tail(after);
            return format!(
                "{}{}{}{}",
                render_math_tokens(before, MathTone::Expression),
                ansi_bold_256(114, marker),
                render_math_tokens(answer, MathTone::Answer),
                render_math_tokens(tail, MathTone::Annotation)
            );
        }
    }

    render_math_tokens(text, MathTone::Expression)
}

fn split_math_control_label(text: &str) -> Option<(&str, &str)> {
    let (before, after) = text.split_once(':')?;
    let label = before.trim();
    if label.is_empty() {
        return None;
    }

    if is_math_control_label(label) {
        return Some((before, after));
    }

    None
}

fn is_math_control_label(label: &str) -> bool {
    if matches!(label, "Evaluate" | "Answer" | "Result" | "Solution") {
        return true;
    }

    let Some(rest) = label.strip_prefix("Step ") else {
        return false;
    };

    rest.split_whitespace()
        .next()
        .is_some_and(|step| step.chars().all(|ch| ch.is_ascii_digit()))
}

fn split_expression_annotation(text: &str) -> Option<(&str, &str)> {
    let (before, after) = text.split_once(':')?;
    if has_math_syntax(before) {
        return Some((before, after));
    }

    None
}

fn split_last_relation(text: &str) -> Option<(&str, &str, &str)> {
    let (idx, marker) = last_relation(text)?;
    let marker_end = idx + marker.len();
    Some((&text[..idx], &text[idx..marker_end], &text[marker_end..]))
}

fn last_relation(text: &str) -> Option<(usize, &'static str)> {
    let mut last = None;
    let mut skip_until = 0;

    for (idx, _) in text.char_indices() {
        if idx < skip_until {
            continue;
        }
        if let Some(marker) = relation_at(text, idx) {
            last = Some((idx, marker));
            skip_until = idx + marker.len();
        }
    }

    last
}

fn relation_at(text: &str, idx: usize) -> Option<&'static str> {
    let rest = &text[idx..];
    for marker in ["->", "<=", ">=", "!=", "=", "<", ">"] {
        if rest.starts_with(marker) {
            return Some(marker);
        }
    }

    None
}

fn relation_count(text: &str) -> usize {
    let mut count = 0;
    let mut skip_until = 0;

    for (idx, _) in text.char_indices() {
        if idx < skip_until {
            continue;
        }
        if let Some(marker) = relation_at(text, idx) {
            count += 1;
            skip_until = idx + marker.len();
        }
    }

    count
}

fn first_relation(text: &str) -> Option<(usize, &'static str)> {
    let mut skip_until = 0;

    for (idx, _) in text.char_indices() {
        if idx < skip_until {
            continue;
        }
        if let Some(marker) = relation_at(text, idx) {
            return Some((idx, marker));
        }
        skip_until = idx;
    }

    None
}

fn split_answer_tail(text: &str) -> (&str, &str) {
    let mut seen_answer = false;
    let mut prev_space = false;

    for (idx, ch) in text.char_indices() {
        if ch.is_whitespace() {
            if seen_answer && prev_space {
                return text.split_at(idx - 1);
            }
            prev_space = true;
            continue;
        }

        if seen_answer && matches!(ch, '(' | ',' | ';') {
            return text.split_at(idx);
        }

        seen_answer = true;
        prev_space = false;
    }

    (text, "")
}

fn split_trailing_note(text: &str) -> Option<(&str, &str)> {
    let mut run_start = None;
    let mut run_len = 0;
    let mut candidate = None;

    for (idx, ch) in text.char_indices() {
        if ch.is_whitespace() {
            if run_start.is_none() {
                run_start = Some(idx);
            }
            run_len += 1;
            continue;
        }

        if let Some(start) = run_start {
            if run_len >= 2 {
                let before = text[..start].trim_end();
                let after = text[idx..].trim_start();
                if is_trailing_math_note(before, after) {
                    candidate = Some((start, idx));
                }
            }
        }

        run_start = None;
        run_len = 0;
    }

    let (space_start, note_start) = candidate?;
    Some((&text[..space_start], &text[note_start..]))
}

fn is_trailing_math_note(before: &str, after: &str) -> bool {
    if before.is_empty() || after.is_empty() || !has_math_syntax(before) {
        return false;
    }

    if after.starts_with(['=', '<', '>', '+', '*', '/', '^']) || after.starts_with("->") {
        return false;
    }

    after.starts_with('(')
        || after
            .chars()
            .next()
            .is_some_and(|ch| ch.is_ascii_alphabetic())
}

fn should_pop_final_math(text: &str) -> bool {
    !is_math_guide_line(text) && (text.contains("->") || relation_count(text) >= 2)
}

fn looks_like_prose_math_line(text: &str) -> bool {
    let Some((idx, _)) = first_relation(text) else {
        return false;
    };
    let before_relation = &text[..idx];
    let prose_word_count = before_relation
        .split_whitespace()
        .filter(|word| {
            let trimmed = word.trim_matches(|ch: char| !ch.is_ascii_alphabetic());
            trimmed.len() > 1 && trimmed.chars().all(|ch| ch.is_ascii_alphabetic())
        })
        .count();

    prose_word_count >= 2
}

fn is_math_guide_line(text: &str) -> bool {
    let trimmed = text.trim();
    trimmed.contains("----") || trimmed.starts_with("...")
}

fn has_math_syntax(text: &str) -> bool {
    text.chars().any(|ch| {
        ch.is_ascii_digit() || matches!(ch, '=' | '<' | '>' | '+' | '*' | '/' | '^' | '|')
    }) || text.contains("->")
}

fn render_math_tokens(text: &str, tone: MathTone) -> String {
    let mut out = String::new();
    let mut chars = text.chars().peekable();

    while let Some(ch) = chars.peek().copied() {
        if ch.is_ascii_whitespace() {
            out.push(ch);
            chars.next();
            continue;
        }

        if ch == '.' {
            let mut lookahead = chars.clone();
            lookahead.next();
            if !lookahead.peek().is_some_and(|c| c.is_ascii_digit()) {
                let token = take_while(&mut chars, |c| c == '.');
                out.push_str(&ansi_256(244, &token));
                continue;
            }
        }

        if ch.is_ascii_digit() || ch == '.' {
            let token = take_while(&mut chars, |c| c.is_ascii_digit() || c == '.');
            out.push_str(&style_math_number(&token, tone));
            continue;
        }

        if ch.is_ascii_alphabetic() {
            let token = take_while(&mut chars, |c| {
                c.is_ascii_alphanumeric() || matches!(c, '_' | '\'')
            });
            out.push_str(&style_math_ident(&token, tone));
            continue;
        }

        if matches!(ch, '=' | '<' | '>') {
            let token = take_while(&mut chars, |c| matches!(c, '=' | '<' | '>'));
            out.push_str(&style_math_relation(&token, tone));
            continue;
        }

        if ch == '-' {
            chars.next();
            if chars.peek() == Some(&'>') {
                chars.next();
                out.push_str(&style_math_relation("->", tone));
            } else if chars.peek() == Some(&'-') {
                let mut token = String::from("-");
                token.push_str(&take_while(&mut chars, |c| c == '-'));
                out.push_str(&ansi_256(244, &token));
            } else {
                out.push_str(&style_math_operator("-", tone));
            }
            continue;
        }

        if matches!(ch, '+' | '*' | '/' | '^') {
            chars.next();
            out.push_str(&style_math_operator(&ch.to_string(), tone));
            continue;
        }

        if matches!(ch, '(' | ')' | '[' | ']' | '{' | '}' | '|') {
            chars.next();
            out.push_str(&style_math_grouping(&ch.to_string(), tone));
            continue;
        }

        if matches!(ch, ':' | ',' | ';') {
            chars.next();
            out.push_str(&ansi_256(244, &ch.to_string()));
            continue;
        }

        chars.next();
        match tone {
            MathTone::Expression => out.push(ch),
            MathTone::Reason => out.push_str(&ansi_dim(&ch.to_string())),
            MathTone::Annotation => out.push_str(&ansi_dim(&ch.to_string())),
            MathTone::Answer => out.push_str(&ansi_bold_256(220, &ch.to_string())),
        }
    }

    out
}

fn style_math_number(token: &str, tone: MathTone) -> String {
    match tone {
        MathTone::Expression => ansi_bold(token),
        MathTone::Reason => ansi_256(110, token),
        MathTone::Annotation => ansi_256(81, token),
        MathTone::Answer => ansi_bold_256(220, token),
    }
}

fn style_math_ident(token: &str, tone: MathTone) -> String {
    match tone {
        MathTone::Expression => token.to_string(),
        MathTone::Reason => ansi_dim(token),
        MathTone::Annotation => ansi_dim(token),
        MathTone::Answer => ansi_bold_256(220, token),
    }
}

fn style_math_operator(token: &str, tone: MathTone) -> String {
    match tone {
        MathTone::Expression => ansi_256(114, token),
        MathTone::Reason => ansi_256(245, token),
        MathTone::Annotation => ansi_256(81, token),
        MathTone::Answer => ansi_bold_256(220, token),
    }
}

fn style_math_relation(token: &str, tone: MathTone) -> String {
    match tone {
        MathTone::Expression | MathTone::Answer => ansi_bold_256(114, token),
        MathTone::Reason => ansi_256(245, token),
        MathTone::Annotation => ansi_256(81, token),
    }
}

fn style_math_grouping(token: &str, tone: MathTone) -> String {
    match tone {
        MathTone::Expression => ansi_256(250, token),
        MathTone::Reason => ansi_256(244, token),
        MathTone::Annotation => ansi_256(244, token),
        MathTone::Answer => ansi_bold_256(220, token),
    }
}

fn take_while<F>(chars: &mut std::iter::Peekable<std::str::Chars<'_>>, mut predicate: F) -> String
where
    F: FnMut(char) -> bool,
{
    let mut token = String::new();
    while let Some(ch) = chars.peek().copied() {
        if !predicate(ch) {
            break;
        }
        token.push(ch);
        chars.next();
    }
    token
}

fn render_code_lines(lang: &str, code: &str) -> Vec<String> {
    if is_math_code_lang(lang) {
        return render_math_lines(code);
    }

    if is_plain_text_code_lang(lang) {
        return code
            .split('\n')
            .map(|code_line| format!("  {code_line}"))
            .collect();
    }

    match highlighted_code_lines(lang, code) {
        Some(lines) => lines.into_iter().map(|line| format!("  {line}")).collect(),
        None => code
            .split('\n')
            .map(|code_line| format!("  {}", ansi_dim(code_line)))
            .collect(),
    }
}

// ---------------------------------------------------------------------------
// Inline rendering — processes bold, italic, code, links, strikethrough
// ---------------------------------------------------------------------------

/// Render inline markdown to ANSI-styled text. Also collects links.
fn render_inline(
    text: &str,
    links: &mut Vec<Link>,
    seen_links: &mut HashSet<String>,
    link_occurrences: &mut Vec<Link>,
    file_refs: &mut Vec<FileRef>,
    seen_file_refs: &mut HashSet<String>,
    file_ref_occurrences: &mut Vec<FileRef>,
) -> String {
    let mut text = text.to_string();

    // 1. Protect code spans — replace with placeholders, render after.
    let mut code_spans: Vec<String> = Vec::new();
    text = regex_replace_all(&text, r"`([^`]+)`", |caps: &[&str]| {
        let idx = code_spans.len();
        code_spans.push(ansi_cyan(caps[1]));
        format!("\x00CS{idx}\x00")
    });

    // 1b. Protect file refs (@file:path[:line[:col]]) as jump-to-editor chips.
    let mut file_chips: Vec<String> = Vec::new();
    text = replace_file_refs(
        &text,
        file_refs,
        seen_file_refs,
        file_ref_occurrences,
        &mut file_chips,
    );

    // 2. Links: [text](url), then bare URLs.
    text = regex_replace_all_with_links(&text, links, seen_links, link_occurrences);
    text = replace_bare_urls(&text, links, seen_links, link_occurrences);

    // 3. Bold + italic: ***text*** or ___text___
    text = regex_replace_all(&text, r"\*{3}(.+?)\*{3}", |caps: &[&str]| {
        ansi_bold(&ansi_italic(caps[1]))
    });
    text = regex_replace_all(&text, r"_{3}(.+?)_{3}", |caps: &[&str]| {
        ansi_bold(&ansi_italic(caps[1]))
    });

    // 4. Bold: **text** or __text__
    text = regex_replace_all(&text, r"\*{2}(.+?)\*{2}", |caps: &[&str]| {
        ansi_bold(caps[1])
    });
    text = regex_replace_all(&text, r"_{2}(.+?)_{2}", |caps: &[&str]| ansi_bold(caps[1]));

    // 5. Italic: *text* or _text_ (avoid matching inside words for _)
    text = regex_replace_all(&text, r"(?<!\w)\*(.+?)\*(?!\*)", |caps: &[&str]| {
        ansi_italic(caps[1])
    });
    text = regex_replace_all(&text, r"(?<!\w)_(.+?)_(?!\w)", |caps: &[&str]| {
        ansi_italic(caps[1])
    });

    // 6. Strikethrough: ~~text~~
    text = regex_replace_all(&text, r"~~(.+?)~~", |caps: &[&str]| {
        ansi_strikethrough(caps[1])
    });

    // Restore code spans.
    for (i, span) in code_spans.iter().enumerate() {
        let placeholder = format!("\x00CS{i}\x00");
        text = text.replace(&placeholder, span);
    }

    // Restore file-ref chips.
    for (i, chip) in file_chips.iter().enumerate() {
        let placeholder = format!("\x00FR{i}\x00");
        text = text.replace(&placeholder, chip);
    }

    text
}

/// Scan for `@file` references — `@file:path[:line[:col]]` (bare) or
/// `@file[label](path[:line[:col]])` (labeled) — collecting deduped FileRefs and
/// replacing each occurrence with a placeholder whose styled chip is pushed to
/// `chips`. Every occurrence is styled; only new targets are collected.
fn replace_file_refs(
    text: &str,
    file_refs: &mut Vec<FileRef>,
    seen_file_refs: &mut HashSet<String>,
    file_ref_occurrences: &mut Vec<FileRef>,
    chips: &mut Vec<String>,
) -> String {
    const MARKER: &str = "@file";
    let mut result = String::new();
    let mut remaining = text;
    while let Some(pos) = remaining.find(MARKER) {
        result.push_str(&remaining[..pos]);
        let after = &remaining[pos + MARKER.len()..];
        match parse_file_ref_at(after) {
            Some((consumed, file_ref)) => {
                let key = format!("{}|{:?}|{:?}", file_ref.path, file_ref.line, file_ref.col);
                let index = file_ref_occurrences.len();
                let chip =
                    wrap_occurrence_marker(OCCURRENCE_FILE_REF, index, file_ref_chip(&file_ref));
                if seen_file_refs.insert(key) {
                    file_refs.push(file_ref.clone());
                }
                file_ref_occurrences.push(file_ref);
                let idx = chips.len();
                chips.push(chip);
                result.push_str(&format!("\x00FR{idx}\x00"));
                remaining = &after[consumed..];
            }
            None => {
                result.push_str(MARKER);
                remaining = after;
            }
        }
    }
    result.push_str(remaining);
    result
}

/// Parse a file ref immediately after the `@file` marker. `:path[:line[:col]]`
/// is the bare form; `[label](path[:line[:col]])` is the labeled form. Returns
/// the bytes consumed from `after` and the parsed FileRef, or None.
fn parse_file_ref_at(after: &str) -> Option<(usize, FileRef)> {
    match after.chars().next()? {
        ':' => {
            let rest = &after[1..];
            let end = rest.find(char::is_whitespace).unwrap_or(rest.len());
            let (path, line, col) = parse_file_target(&rest[..end])?;
            Some((
                1 + end,
                FileRef {
                    path,
                    line,
                    col,
                    label: None,
                },
            ))
        }
        '[' => {
            let rest = &after[1..];
            let label_end = rest.find(']')?;
            let label = rest[..label_end].to_string();
            let after_label = &rest[label_end + 1..];
            if !after_label.starts_with('(') {
                return None;
            }
            let in_paren = &after_label[1..];
            let paren_end = in_paren.find(')')?;
            let (path, line, col) = parse_file_target(in_paren[..paren_end].trim())?;
            let consumed = 1 + label_end + 1 + 1 + paren_end + 1;
            Some((
                consumed,
                FileRef {
                    path,
                    line,
                    col,
                    label: Some(label),
                },
            ))
        }
        _ => None,
    }
}

/// Split a target into path and trailing numeric `:line[:col]`. None if empty.
fn parse_file_target(target: &str) -> Option<(String, Option<usize>, Option<usize>)> {
    if target.is_empty() {
        return None;
    }
    let parts: Vec<&str> = target.split(':').collect();
    let is_num = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    let n = parts.len();
    if n >= 3 && is_num(parts[n - 1]) && is_num(parts[n - 2]) {
        let path = parts[..n - 2].join(":");
        if !path.is_empty() {
            return Some((path, parts[n - 2].parse().ok(), parts[n - 1].parse().ok()));
        }
    }
    if n >= 2 && is_num(parts[n - 1]) {
        let path = parts[..n - 1].join(":");
        if !path.is_empty() {
            return Some((path, parts[n - 1].parse().ok(), None));
        }
    }
    Some((target.to_string(), None, None))
}

/// Styled inline chip for a file ref: a green, bold, underlined `↪label`.
fn file_ref_chip(file_ref: &FileRef) -> String {
    ansi_bold_underline_256(76, &format!("↪{}", file_ref.display()))
}

/// A simple regex replacement function using a manual approach to avoid
/// pulling in the `regex` crate for inline markdown parsing.
fn regex_replace_all<F>(text: &str, pattern: &str, mut replacer: F) -> String
where
    F: FnMut(&[&str]) -> String,
{
    // We use a hand-rolled approach for the specific patterns we need.
    match pattern {
        r"`([^`]+)`" => replace_code_spans(text, &mut replacer),
        r"\*{3}(.+?)\*{3}" => replace_delimited(text, "***", "***", &mut replacer),
        r"_{3}(.+?)_{3}" => replace_delimited(text, "___", "___", &mut replacer),
        r"\*{2}(.+?)\*{2}" => replace_delimited(text, "**", "**", &mut replacer),
        r"_{2}(.+?)_{2}" => replace_delimited(text, "__", "__", &mut replacer),
        r"~~(.+?)~~" => replace_delimited(text, "~~", "~~", &mut replacer),
        r"(?<!\w)\*(.+?)\*(?!\*)" => replace_italic_star(text, &mut replacer),
        r"(?<!\w)_(.+?)_(?!\w)" => replace_italic_underscore(text, &mut replacer),
        _ => text.to_string(),
    }
}

fn replace_code_spans<F>(text: &str, replacer: &mut F) -> String
where
    F: FnMut(&[&str]) -> String,
{
    let mut result = String::new();
    let mut chars = text.char_indices();

    while let Some((i, ch)) = chars.next() {
        if ch == '`' {
            // Find closing backtick.
            if let Some(close) = text[i + ch.len_utf8()..].find('`') {
                let inner_start = i + ch.len_utf8();
                let inner = &text[inner_start..inner_start + close];
                if !inner.is_empty() {
                    let full_match = &text[i..inner_start + close + 1];
                    let caps: Vec<&str> = vec![full_match, inner];
                    result.push_str(&replacer(&caps));
                    // Advance chars iterator past the code span
                    let skip_to = inner_start + close + 1;
                    while let Some((j, _)) = chars.next() {
                        if j + 1 >= skip_to {
                            break;
                        }
                    }
                    continue;
                }
            }
            result.push(ch);
        } else {
            result.push(ch);
        }
    }
    result
}

fn replace_delimited<F>(text: &str, open: &str, close: &str, replacer: &mut F) -> String
where
    F: FnMut(&[&str]) -> String,
{
    let mut result = String::new();
    let mut remaining = text;

    while let Some(start) = remaining.find(open) {
        result.push_str(&remaining[..start]);
        let after_open = &remaining[start + open.len()..];
        if let Some(end) = after_open.find(close) {
            let inner = &after_open[..end];
            if !inner.is_empty() {
                let full = &remaining[start..start + open.len() + end + close.len()];
                let caps: Vec<&str> = vec![full, inner];
                result.push_str(&replacer(&caps));
                remaining = &after_open[end + close.len()..];
            } else {
                result.push_str(open);
                remaining = after_open;
            }
        } else {
            result.push_str(&remaining[start..]);
            return result;
        }
    }
    result.push_str(remaining);
    result
}

fn replace_italic_star<F>(text: &str, replacer: &mut F) -> String
where
    F: FnMut(&[&str]) -> String,
{
    let mut result = String::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        // Check for ANSI escape (don't match inside escape sequences).
        if chars[i] == '\x1b' {
            // Copy the ANSI sequence as-is.
            while i < chars.len() {
                result.push(chars[i]);
                if chars[i] == 'm' {
                    i += 1;
                    break;
                }
                i += 1;
            }
            continue;
        }
        if chars[i] == '*' && (i == 0 || !chars[i - 1].is_alphanumeric()) {
            // Ensure not ** (bold).
            if i + 1 < chars.len() && chars[i + 1] == '*' {
                result.push(chars[i]);
                i += 1;
                continue;
            }
            // Find closing *.
            if let Some(close) = find_closing_star(&chars, i + 1) {
                let inner: String = chars[i + 1..close].iter().collect();
                if !inner.is_empty() {
                    let full: String = chars[i..=close].iter().collect();
                    let caps: Vec<&str> = vec![&full, &inner];
                    result.push_str(&replacer(&caps));
                    i = close + 1;
                    continue;
                }
            }
        }
        result.push(chars[i]);
        i += 1;
    }
    result
}

fn find_closing_star(chars: &[char], start: usize) -> Option<usize> {
    let mut i = start;
    while i < chars.len() {
        if chars[i] == '*' && (i + 1 >= chars.len() || chars[i + 1] != '*') {
            return Some(i);
        }
        i += 1;
    }
    None
}

fn replace_italic_underscore<F>(text: &str, replacer: &mut F) -> String
where
    F: FnMut(&[&str]) -> String,
{
    let mut result = String::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        // Check for ANSI escape.
        if chars[i] == '\x1b' {
            while i < chars.len() {
                result.push(chars[i]);
                if chars[i] == 'm' {
                    i += 1;
                    break;
                }
                i += 1;
            }
            continue;
        }
        if chars[i] == '_' && (i == 0 || !chars[i - 1].is_alphanumeric()) {
            // Ensure not __ (bold).
            if i + 1 < chars.len() && chars[i + 1] == '_' {
                result.push(chars[i]);
                i += 1;
                continue;
            }
            // Find closing _ that is not followed by a word char.
            if let Some(close) = find_closing_underscore(&chars, i + 1) {
                let inner: String = chars[i + 1..close].iter().collect();
                if !inner.is_empty() {
                    let full: String = chars[i..=close].iter().collect();
                    let caps: Vec<&str> = vec![&full, &inner];
                    result.push_str(&replacer(&caps));
                    i = close + 1;
                    continue;
                }
            }
        }
        result.push(chars[i]);
        i += 1;
    }
    result
}

fn find_closing_underscore(chars: &[char], start: usize) -> Option<usize> {
    let mut i = start;
    while i < chars.len() {
        if chars[i] == '_' && (i + 1 >= chars.len() || !chars[i + 1].is_alphanumeric()) {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// Handle link replacement [text](url) and collect links.
fn regex_replace_all_with_links(
    text: &str,
    links: &mut Vec<Link>,
    seen_links: &mut HashSet<String>,
    link_occurrences: &mut Vec<Link>,
) -> String {
    let mut result = String::new();
    let mut remaining = text;

    while let Some(bracket_start) = remaining.find('[') {
        result.push_str(&remaining[..bracket_start]);
        let after_bracket = &remaining[bracket_start + 1..];

        if let Some(bracket_end) = after_bracket.find(']') {
            let label = &after_bracket[..bracket_end];
            let after_close = &after_bracket[bracket_end + 1..];

            if after_close.starts_with('(') {
                if let Some(paren_end) = after_close.find(')') {
                    let href = after_close[1..paren_end].trim();
                    let trim_label = label.trim();
                    let display = if trim_label.is_empty() || is_bare_url(trim_label) {
                        compact_url_label(href)
                    } else {
                        trim_label.to_string()
                    };

                    let key = format!("{display}|{href}");
                    if !href.is_empty() {
                        let link = Link {
                            text: display.clone(),
                            href: href.to_string(),
                        };
                        let index = link_occurrences.len();
                        link_occurrences.push(link.clone());

                        if seen_links.insert(key) {
                            links.push(link);
                        }
                        result.push_str(&wrap_occurrence_marker(
                            OCCURRENCE_LINK,
                            index,
                            ansi_underline(&display),
                        ));
                    } else {
                        result.push_str(&ansi_underline(&display));
                    }

                    remaining = &after_close[paren_end + 1..];
                    continue;
                }
            }

            // Not a valid link, keep the bracket.
            result.push('[');
            remaining = after_bracket;
        } else {
            result.push('[');
            remaining = after_bracket;
        }
    }
    result.push_str(remaining);
    result
}

fn replace_bare_urls(
    text: &str,
    links: &mut Vec<Link>,
    seen_links: &mut HashSet<String>,
    link_occurrences: &mut Vec<Link>,
) -> String {
    let mut result = String::new();
    let mut remaining = text;

    while let Some(start) = find_next_url_start(remaining) {
        result.push_str(&remaining[..start]);
        let candidate = &remaining[start..];
        let raw_end = candidate
            .char_indices()
            .find(|(_, ch)| is_url_boundary(*ch))
            .map(|(idx, _)| idx)
            .unwrap_or(candidate.len());
        let (href, trailing) = split_trailing_url_punctuation(&candidate[..raw_end]);

        if href.is_empty() {
            result.push_str(&candidate[..raw_end]);
            remaining = &candidate[raw_end..];
            continue;
        }

        let display = compact_url_label(href);
        let key = format!("{display}|{href}");
        let link = Link {
            text: display.clone(),
            href: href.to_string(),
        };
        let index = link_occurrences.len();
        link_occurrences.push(link.clone());
        if seen_links.insert(key) {
            links.push(link);
        }
        result.push_str(&wrap_occurrence_marker(
            OCCURRENCE_LINK,
            index,
            ansi_underline(&display),
        ));
        result.push_str(trailing);
        remaining = &candidate[raw_end..];
    }

    result.push_str(remaining);
    result
}

fn find_next_url_start(text: &str) -> Option<usize> {
    match (text.find("https://"), text.find("http://")) {
        (Some(a), Some(b)) => Some(a.min(b)),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }
}

fn is_bare_url(text: &str) -> bool {
    text.starts_with("https://") || text.starts_with("http://")
}

fn is_url_boundary(ch: char) -> bool {
    ch.is_whitespace() || matches!(ch, '<' | '>' | '"' | '\'')
}

fn split_trailing_url_punctuation(url: &str) -> (&str, &str) {
    let mut end = url.len();
    while end > 0 {
        let Some(ch) = url[..end].chars().next_back() else {
            break;
        };
        if matches!(ch, '.' | ',' | ';' | ':' | '!' | '?' | ')' | ']' | '}') {
            end -= ch.len_utf8();
        } else {
            break;
        }
    }
    url.split_at(end)
}

fn compact_url_label(url: &str) -> String {
    let without_scheme = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url)
        .trim_end_matches('/');
    let (host, rest) = without_scheme
        .split_once('/')
        .unwrap_or((without_scheme, ""));
    let host = host.strip_prefix("www.").unwrap_or(host);

    let mut label = if host == "github.com" && !rest.is_empty() {
        compact_github_url_label(rest)
    } else if rest.is_empty() {
        format!("↗{host}")
    } else {
        let tail = rest
            .rsplit('/')
            .find(|part| !part.is_empty())
            .unwrap_or(rest);
        let tail = tail
            .split('#')
            .next_back()
            .filter(|part| !part.is_empty())
            .unwrap_or(tail);
        format!("↗{host}/…/{}", compact_url_tail(tail))
    };

    if visible_width(&label) > 18 {
        label = format!("{}…", truncate_to_width(&label, 17));
    }
    label
}

fn compact_github_url_label(rest: &str) -> String {
    let fragment = rest.split('#').nth(1);
    if let Some(fragment) = fragment {
        if let Some(id) = fragment.strip_prefix("discussion_") {
            return format!("↗GH#{}", compact_url_tail(id));
        }
        return format!("↗GH#{}", compact_url_tail(fragment));
    }

    let mut parts = rest.split('/');
    let _owner = parts.next();
    let _repo = parts.next();
    match (parts.next(), parts.next()) {
        (Some("pull"), Some(pr)) => format!("↗GH#PR{}", compact_url_tail(pr)),
        (Some("issues"), Some(issue)) => format!("↗GH#{}", compact_url_tail(issue)),
        _ => "↗GH".to_string(),
    }
}

fn compact_url_tail(tail: &str) -> String {
    let compact = tail
        .strip_prefix("discussion_")
        .unwrap_or(tail)
        .replace('_', "-");
    if visible_width(&compact) > 8 {
        let mut start = compact.chars().take(3).collect::<String>();
        start.push('…');
        let end = compact
            .chars()
            .rev()
            .take(4)
            .collect::<String>()
            .chars()
            .rev()
            .collect::<String>();
        start.push_str(&end);
        start
    } else {
        compact
    }
}

/// Strip inline markdown syntax for plain text extraction.
fn strip_inline(text: &str) -> String {
    let mut t = text.to_string();
    // code spans
    t = strip_delimited_content(&t, "`", "`");
    // links
    t = strip_links(&t);
    // bold+italic
    t = strip_delimited_markers(&t, "***", "***");
    t = strip_delimited_markers(&t, "___", "___");
    // bold
    t = strip_delimited_markers(&t, "**", "**");
    t = strip_delimited_markers(&t, "__", "__");
    // italic (simple approach)
    t = strip_single_markers(&t, '*');
    t = strip_single_markers(&t, '_');
    // strikethrough
    t = strip_delimited_markers(&t, "~~", "~~");
    t
}

fn strip_delimited_content(text: &str, open: &str, close: &str) -> String {
    let mut result = String::new();
    let mut remaining = text;
    while let Some(start) = remaining.find(open) {
        result.push_str(&remaining[..start]);
        let after = &remaining[start + open.len()..];
        if let Some(end) = after.find(close) {
            result.push_str(&after[..end]);
            remaining = &after[end + close.len()..];
        } else {
            result.push_str(&remaining[start..]);
            return result;
        }
    }
    result.push_str(remaining);
    result
}

fn strip_delimited_markers(text: &str, open: &str, close: &str) -> String {
    let mut result = String::new();
    let mut remaining = text;
    while let Some(start) = remaining.find(open) {
        result.push_str(&remaining[..start]);
        let after = &remaining[start + open.len()..];
        if let Some(end) = after.find(close) {
            let inner = &after[..end];
            if !inner.is_empty() {
                result.push_str(inner);
                remaining = &after[end + close.len()..];
            } else {
                result.push_str(open);
                remaining = after;
            }
        } else {
            result.push_str(&remaining[start..]);
            return result;
        }
    }
    result.push_str(remaining);
    result
}

fn strip_single_markers(text: &str, marker: char) -> String {
    let mut result = String::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == marker
            && (i == 0 || !chars[i - 1].is_alphanumeric())
            && i + 1 < chars.len()
            && chars[i + 1] != marker
        {
            // Find closing marker.
            let mut j = i + 1;
            let mut found = false;
            while j < chars.len() {
                if chars[j] == marker && (j + 1 >= chars.len() || chars[j + 1] != marker) {
                    // Found closing marker.
                    let inner: String = chars[i + 1..j].iter().collect();
                    result.push_str(&inner);
                    i = j + 1;
                    found = true;
                    break;
                }
                j += 1;
            }
            if !found {
                result.push(chars[i]);
                i += 1;
            }
        } else {
            result.push(chars[i]);
            i += 1;
        }
    }
    result
}

fn strip_links(text: &str) -> String {
    let mut result = String::new();
    let mut remaining = text;
    while let Some(start) = remaining.find('[') {
        result.push_str(&remaining[..start]);
        let after = &remaining[start + 1..];
        if let Some(end) = after.find(']') {
            let label = &after[..end];
            let after_close = &after[end + 1..];
            if after_close.starts_with('(') {
                if let Some(paren_end) = after_close.find(')') {
                    result.push_str(label);
                    remaining = &after_close[paren_end + 1..];
                    continue;
                }
            }
            result.push('[');
            remaining = after;
        } else {
            result.push('[');
            remaining = after;
        }
    }
    result.push_str(remaining);
    result
}

// ---------------------------------------------------------------------------
// Word wrapping (ANSI-aware)
// ---------------------------------------------------------------------------

fn wrap_line(text: &str, width: usize, hanging_indent: &str) -> Vec<String> {
    if width == 0 {
        return vec![text.to_string()];
    }
    if visible_width(text) <= width {
        return vec![text.to_string()];
    }

    let plain = strip_ansi(text);
    if plain.len() <= width {
        return vec![text.to_string()];
    }

    let mut result: Vec<String> = Vec::new();
    let mut current_line = String::new();
    let mut current_vis = 0;
    let hang_width = visible_width(hanging_indent);

    let words = split_words(text);
    let mut is_first_line = true;

    for word in &words {
        let w_vis = visible_width(word);

        if current_vis == 0 {
            if is_first_line {
                current_line = word.clone();
                current_vis = w_vis;
            } else {
                current_line = format!("{hanging_indent}{word}");
                current_vis = hang_width + w_vis;
            }
        } else {
            let limit = if is_first_line {
                width
            } else {
                width + hang_width
            };
            if current_vis + 1 + w_vis > limit {
                result.push(current_line);
                is_first_line = false;
                current_line = format!("{hanging_indent}{word}");
                current_vis = hang_width + w_vis;
            } else {
                current_line.push(' ');
                current_line.push_str(word);
                current_vis += 1 + w_vis;
            }
        }
    }
    if !current_line.is_empty() {
        result.push(current_line);
    }

    if result.is_empty() {
        vec![text.to_string()]
    } else {
        result
    }
}

/// Split text into words, preserving ANSI codes attached to adjacent text.
fn split_words(text: &str) -> Vec<String> {
    let mut words: Vec<String> = Vec::new();
    let mut current = String::new();
    let mut chars = text.char_indices().peekable();

    while let Some((_i, ch)) = chars.next() {
        if ch == OCCURRENCE_MARKER_START {
            current.push(ch);
            while let Some((_, c)) = chars.next() {
                current.push(c);
                if c == OCCURRENCE_MARKER_END {
                    break;
                }
            }
            continue;
        }
        // Check for ANSI escape.
        if ch == '\x1b' {
            if let Some(&(_, next_ch)) = chars.peek() {
                if next_ch == '[' {
                    current.push(ch);
                    let (_, bracket) = chars.next().unwrap();
                    current.push(bracket);
                    // Copy until 'm'
                    while let Some((_, c)) = chars.next() {
                        current.push(c);
                        if c == 'm' {
                            break;
                        }
                    }
                    continue;
                }
            }
        }
        if ch == ' ' {
            if !strip_ansi(&current).is_empty() {
                words.push(current);
                current = String::new();
            }
            // If current is ANSI-only prefix, keep it for the next word.
        } else {
            current.push(ch);
        }
    }
    if !current.is_empty() && !strip_ansi(&current).is_empty() {
        words.push(current);
    }

    words
}

// ---------------------------------------------------------------------------
// Block parser
// ---------------------------------------------------------------------------

#[derive(Debug)]
enum Block {
    Heading {
        level: usize,
        text: String,
    },
    Paragraph {
        text: String,
    },
    List {
        ordered: bool,
        items: Vec<String>,
    },
    Blockquote {
        lines: Vec<String>,
    },
    Code {
        lang: String,
        code: String,
    },
    Table {
        headers: Vec<String>,
        rows: Vec<Vec<String>>,
    },
    Hr,
    Blank,
}

fn parse_blocks(content: &str) -> Vec<Block> {
    let lines: Vec<&str> = content.split('\n').collect();
    let mut blocks: Vec<Block> = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i];

        // Blank line.
        if line.trim().is_empty() {
            blocks.push(Block::Blank);
            i += 1;
            continue;
        }

        // Fenced code block.
        if let Some(fence_info) = try_parse_fence(line) {
            let code_lines = collect_fenced_code(&lines, &mut i, &fence_info.0);
            blocks.push(Block::Code {
                lang: fence_info.1.to_lowercase(),
                code: code_lines.join("\n"),
            });
            continue;
        }

        // Heading (ATX-style).
        if let Some((level, text)) = try_parse_heading(line) {
            blocks.push(Block::Heading { level, text });
            i += 1;
            continue;
        }

        // Horizontal rule.
        if is_horizontal_rule(line) {
            blocks.push(Block::Hr);
            i += 1;
            continue;
        }

        // Blockquote.
        if line.starts_with("> ") || line == ">" {
            let bq_lines = collect_blockquote(&lines, &mut i);
            blocks.push(Block::Blockquote { lines: bq_lines });
            continue;
        }

        // Unordered list.
        if is_unordered_list_item(line) {
            let items = collect_unordered_list(&lines, &mut i);
            blocks.push(Block::List {
                ordered: false,
                items,
            });
            continue;
        }

        // Ordered list.
        if is_ordered_list_item(line) {
            let items = collect_ordered_list(&lines, &mut i);
            blocks.push(Block::List {
                ordered: true,
                items,
            });
            continue;
        }

        // Table — detect pipe-delimited rows with a separator line.
        if let Some(table) = try_parse_table(&lines, &mut i) {
            blocks.push(table);
            continue;
        }

        // Paragraph — collect contiguous non-blank, non-special lines.
        let para_lines = collect_paragraph(&lines, &mut i);
        if !para_lines.is_empty() {
            blocks.push(Block::Paragraph {
                text: para_lines.join(" "),
            });
        }
    }

    blocks
}

/// Try to parse a fence opener line. Returns (fence_string, lang).
fn try_parse_fence(line: &str) -> Option<(String, String)> {
    let trimmed = line;
    let mut fence_char = None;
    let mut fence_len = 0;

    for ch in trimmed.chars() {
        if ch == '`' || ch == '~' {
            match fence_char {
                None => {
                    fence_char = Some(ch);
                    fence_len = 1;
                }
                Some(fc) if fc == ch => fence_len += 1,
                _ => return None,
            }
        } else {
            break;
        }
    }

    if fence_len >= 3 {
        let fence = trimmed[..fence_len].to_string();
        let lang = trimmed[fence_len..].trim().to_string();
        Some((fence, lang))
    } else {
        None
    }
}

fn collect_fenced_code(lines: &[&str], i: &mut usize, fence: &str) -> Vec<String> {
    *i += 1; // skip the opening fence
    let mut code_lines = Vec::new();
    while *i < lines.len() {
        if lines[*i].starts_with(fence) && lines[*i].trim() == fence {
            *i += 1;
            break;
        }
        code_lines.push(lines[*i].to_string());
        *i += 1;
    }
    code_lines
}

/// Try to parse a GFM table starting at the current line.
/// A table requires: header row with `|`, separator row with `|---|`, and at least one data row.
fn try_parse_table(lines: &[&str], i: &mut usize) -> Option<Block> {
    let start = *i;
    // Need at least 3 lines: header, separator, one data row
    if start + 2 >= lines.len() {
        return None;
    }

    let header_line = lines[start].trim();
    let sep_line = lines[start + 1].trim();

    // Header must contain pipes
    if !header_line.contains('|') {
        return None;
    }
    // Separator must be pipes and dashes (with optional colons for alignment)
    if !is_table_separator(sep_line) {
        return None;
    }

    let headers = parse_table_row(header_line);
    if headers.is_empty() {
        return None;
    }

    let mut rows: Vec<Vec<String>> = Vec::new();
    let mut j = start + 2;
    while j < lines.len() {
        let row_line = lines[j].trim();
        if row_line.is_empty() || !row_line.contains('|') {
            break;
        }
        let cells = parse_table_row(row_line);
        rows.push(cells);
        j += 1;
    }

    if rows.is_empty() {
        return None;
    }

    *i = j;
    Some(Block::Table { headers, rows })
}

fn is_table_separator(line: &str) -> bool {
    if !line.contains('|') || !line.contains('-') {
        return false;
    }
    line.chars()
        .all(|c| c == '|' || c == '-' || c == ':' || c == ' ')
}

fn parse_table_row(line: &str) -> Vec<String> {
    let trimmed = line.trim().trim_matches('|');
    trimmed
        .split('|')
        .map(|cell| cell.trim().to_string())
        .collect()
}

fn try_parse_heading(line: &str) -> Option<(usize, String)> {
    let bytes = line.as_bytes();
    let mut level = 0;
    while level < bytes.len() && bytes[level] == b'#' && level < 6 {
        level += 1;
    }
    if level == 0 || level >= bytes.len() || bytes[level] != b' ' {
        return None;
    }
    let text = line[level + 1..].to_string();
    // Strip trailing # markers.
    let text = strip_trailing_hashes(&text);
    Some((level, text))
}

fn strip_trailing_hashes(text: &str) -> String {
    // Remove trailing ` #+` from the heading text.
    let trimmed = text.trim_end();
    if let Some(idx) = trimmed.rfind(|c: char| c != '#' && c != ' ') {
        let candidate = &trimmed[idx + 1..];
        if candidate.contains('#') {
            // Only strip if the # sequence is preceded by a space.
            let pre = &trimmed[..=idx];
            if pre.ends_with(' ') {
                return pre.trim_end().to_string();
            }
        }
    }
    text.to_string()
}

fn is_horizontal_rule(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.len() < 3 {
        return false;
    }
    let first = trimmed.chars().next().unwrap();
    if first != '*' && first != '-' && first != '_' {
        return false;
    }
    trimmed.chars().all(|c| c == first || c == ' ')
}

fn collect_blockquote(lines: &[&str], i: &mut usize) -> Vec<String> {
    let mut bq_lines = Vec::new();
    while *i < lines.len() {
        let line = lines[*i];
        if line.starts_with("> ") {
            bq_lines.push(line[2..].to_string());
            *i += 1;
        } else if line == ">" {
            bq_lines.push(String::new());
            *i += 1;
        } else {
            break;
        }
    }
    bq_lines
}

fn is_unordered_list_item(line: &str) -> bool {
    let bytes = line.as_bytes();
    bytes.len() >= 2
        && (bytes[0] == b'-' || bytes[0] == b'*' || bytes[0] == b'+')
        && bytes[1] == b' '
}

fn collect_unordered_list(lines: &[&str], i: &mut usize) -> Vec<String> {
    let mut items = Vec::new();
    while *i < lines.len() {
        let line = lines[*i];
        if is_unordered_list_item(line) {
            items.push(line[2..].to_string());
            *i += 1;
            // Continuation lines (indented).
            while *i < lines.len()
                && !lines[*i].trim().is_empty()
                && lines[*i].starts_with("  ")
                && !is_unordered_list_item(lines[*i])
            {
                let last = items.last_mut().unwrap();
                last.push(' ');
                last.push_str(lines[*i].trim());
                *i += 1;
            }
        } else if lines[*i].trim().is_empty() {
            // Blank line inside list — peek ahead.
            if *i + 1 < lines.len() && is_unordered_list_item(lines[*i + 1]) {
                *i += 1;
                continue;
            }
            break;
        } else {
            break;
        }
    }
    items
}

fn is_ordered_list_item(line: &str) -> bool {
    let mut idx = 0;
    let bytes = line.as_bytes();
    while idx < bytes.len() && bytes[idx].is_ascii_digit() {
        idx += 1;
    }
    if idx == 0 || idx >= bytes.len() {
        return false;
    }
    if bytes[idx] != b'.' && bytes[idx] != b')' {
        return false;
    }
    idx + 1 < bytes.len() && bytes[idx + 1] == b' '
}

fn ordered_list_item_text(line: &str) -> &str {
    let bytes = line.as_bytes();
    let mut idx = 0;
    while idx < bytes.len() && bytes[idx].is_ascii_digit() {
        idx += 1;
    }
    // Skip the '.' or ')' and the space.
    if idx < bytes.len() && (bytes[idx] == b'.' || bytes[idx] == b')') {
        idx += 1;
    }
    if idx < bytes.len() && bytes[idx] == b' ' {
        idx += 1;
    }
    &line[idx..]
}

fn collect_ordered_list(lines: &[&str], i: &mut usize) -> Vec<String> {
    let mut items = Vec::new();
    while *i < lines.len() {
        let line = lines[*i];
        if is_ordered_list_item(line) {
            items.push(ordered_list_item_text(line).to_string());
            *i += 1;
            // Continuation lines.
            while *i < lines.len()
                && !lines[*i].trim().is_empty()
                && lines[*i].starts_with("  ")
                && !is_ordered_list_item(lines[*i])
            {
                let last = items.last_mut().unwrap();
                last.push(' ');
                last.push_str(lines[*i].trim());
                *i += 1;
            }
        } else if lines[*i].trim().is_empty() {
            if *i + 1 < lines.len() && is_ordered_list_item(lines[*i + 1]) {
                *i += 1;
                continue;
            }
            break;
        } else {
            break;
        }
    }
    items
}

fn is_special_line(line: &str) -> bool {
    if line.trim().is_empty() {
        return true;
    }
    // Heading
    if try_parse_heading(line).is_some() {
        return true;
    }
    // List items
    if is_unordered_list_item(line) || is_ordered_list_item(line) {
        return true;
    }
    // Blockquote
    if line.starts_with("> ") || line == ">" {
        return true;
    }
    // Fences
    if try_parse_fence(line).is_some() {
        return true;
    }
    // HR
    if is_horizontal_rule(line) {
        return true;
    }
    false
}

fn collect_paragraph(lines: &[&str], i: &mut usize) -> Vec<String> {
    let mut para_lines = Vec::new();
    while *i < lines.len() && !lines[*i].trim().is_empty() && !is_special_line(lines[*i]) {
        para_lines.push(lines[*i].to_string());
        *i += 1;
    }
    para_lines
}

// ---------------------------------------------------------------------------
// Block rendering
// ---------------------------------------------------------------------------

struct RenderContext {
    width: usize,
    viewport_width: usize,
    links: Vec<Link>,
    seen_links: HashSet<String>,
    link_occurrences: Vec<Link>,
    file_refs: Vec<FileRef>,
    seen_file_refs: HashSet<String>,
    file_ref_occurrences: Vec<FileRef>,
    headings: Vec<Heading>,
}

fn render_blocks(blocks: &[Block], ctx: &mut RenderContext) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();

    for block in blocks {
        match block {
            Block::Blank => {
                // Handled by spacing between blocks.
            }

            Block::Heading { level, text } => {
                if !out.is_empty() {
                    out.push(String::new());
                }
                let plain_text = strip_inline(text);
                let line_index = out.len();
                ctx.headings.push(Heading {
                    level: *level,
                    text: plain_text.clone(),
                    line: line_index,
                });
                out.push(render_heading(*level, &plain_text));
                out.push(String::new());
            }

            Block::Paragraph { text } => {
                if !out.is_empty() && !is_blank_line(out.last().unwrap()) {
                    out.push(String::new());
                }
                let styled = render_inline(
                    text,
                    &mut ctx.links,
                    &mut ctx.seen_links,
                    &mut ctx.link_occurrences,
                    &mut ctx.file_refs,
                    &mut ctx.seen_file_refs,
                    &mut ctx.file_ref_occurrences,
                );
                let wrapped = wrap_line(&styled, ctx.width, "");
                out.extend(wrapped);
            }

            Block::List { ordered, items } => {
                if !out.is_empty() && !is_blank_line(out.last().unwrap()) {
                    out.push(String::new());
                }
                for (idx, item) in items.iter().enumerate() {
                    let (bullet, bullet_plain_width) = if *ordered {
                        let num = format!("{}.", idx + 1);
                        let plain_width = num.len() + 1; // "N. "
                        (format!("{} ", ansi_dim(&num)), plain_width)
                    } else {
                        (format!("{} ", ansi_dim("\u{2022}")), 2) // "• "
                    };
                    let hang_indent = " ".repeat(bullet_plain_width);
                    let styled = render_inline(
                        item,
                        &mut ctx.links,
                        &mut ctx.seen_links,
                        &mut ctx.link_occurrences,
                        &mut ctx.file_refs,
                        &mut ctx.seen_file_refs,
                        &mut ctx.file_ref_occurrences,
                    );
                    let item_width = ctx.width.saturating_sub(bullet_plain_width);
                    let effective_width = if item_width > 10 {
                        item_width
                    } else {
                        ctx.width
                    };
                    let wrapped = wrap_line(&styled, effective_width, &hang_indent);
                    if !wrapped.is_empty() {
                        out.push(format!("{bullet}{}", wrapped[0]));
                        for w in &wrapped[1..] {
                            out.push(w.clone());
                        }
                    }
                }
            }

            Block::Blockquote { lines } => {
                if !out.is_empty() && !is_blank_line(out.last().unwrap()) {
                    out.push(String::new());
                }
                let bar = ansi_dim("\u{2502} ");
                let bq_width = ctx.width.saturating_sub(2);
                for bq_line in lines {
                    if bq_line.trim().is_empty() {
                        out.push(bar.clone());
                    } else {
                        let styled = render_inline(
                            bq_line,
                            &mut ctx.links,
                            &mut ctx.seen_links,
                            &mut ctx.link_occurrences,
                            &mut ctx.file_refs,
                            &mut ctx.seen_file_refs,
                            &mut ctx.file_ref_occurrences,
                        );
                        let effective_width = if bq_width > 10 { bq_width } else { ctx.width };
                        let wrapped = wrap_line(&styled, effective_width, "");
                        for w in &wrapped {
                            out.push(format!("{bar}{}", ansi_italic(w)));
                        }
                    }
                }
            }

            Block::Code { lang, code } => {
                if !out.is_empty() && !is_blank_line(out.last().unwrap()) {
                    out.push(String::new());
                }
                if lang == "sketch" {
                    match crate::diagram::render_json_in(code, ctx.viewport_width) {
                        Ok(rendered) if rendered.graph_width <= ctx.viewport_width => {
                            out.extend(rendered.lines);
                            out.push(String::new());
                            continue;
                        }
                        Ok(rendered) => {
                            // The graph itself is too wide for the viewport: an
                            // honest placeholder beats a truncated diagram.
                            // Footnotes wrap, so only the node/edge extent
                            // (graph_width) can trip this — never a long note.
                            let title = rendered.title.as_deref().unwrap_or("untitled");
                            out.push(ansi_dim(&format!(
                                "◆ sketch `{title}` — {} nodes, {} edges — needs {} cols (viewport {})",
                                rendered.node_count,
                                rendered.edge_count,
                                rendered.graph_width,
                                ctx.viewport_width,
                            )));
                            out.push(String::new());
                            continue;
                        }
                        Err(e) => {
                            out.push(ansi_256(196, &format!("✗ sketch: {e}")));
                            // Show the source so the author can fix it.
                        }
                    }
                }
                out.extend(render_code_lines(lang, code));
                out.push(String::new());
            }

            Block::Table { headers, rows } => {
                if !out.is_empty() && !is_blank_line(out.last().unwrap()) {
                    out.push(String::new());
                }
                let table_lines = render_table(headers, rows, ctx);
                out.extend(table_lines);
                out.push(String::new());
            }

            Block::Hr => {
                if !out.is_empty() && !is_blank_line(out.last().unwrap()) {
                    out.push(String::new());
                }
                let rule_width = ctx.width.min(40);
                out.push(ansi_dim(&"\u{2500}".repeat(rule_width)));
                out.push(String::new());
            }
        }
    }

    out
}

fn render_table(headers: &[String], rows: &[Vec<String>], ctx: &mut RenderContext) -> Vec<String> {
    let num_cols = headers.len();

    // Pre-render all cells through render_inline so we can measure VISIBLE width
    let rendered_headers: Vec<String> = headers
        .iter()
        .map(|h| {
            render_inline(
                h,
                &mut ctx.links,
                &mut ctx.seen_links,
                &mut ctx.link_occurrences,
                &mut ctx.file_refs,
                &mut ctx.seen_file_refs,
                &mut ctx.file_ref_occurrences,
            )
        })
        .collect();
    let rendered_rows: Vec<Vec<String>> = rows
        .iter()
        .map(|row| {
            (0..num_cols)
                .map(|c| {
                    let text = row.get(c).map(|s| s.as_str()).unwrap_or("");
                    render_inline(
                        text,
                        &mut ctx.links,
                        &mut ctx.seen_links,
                        &mut ctx.link_occurrences,
                        &mut ctx.file_refs,
                        &mut ctx.seen_file_refs,
                        &mut ctx.file_ref_occurrences,
                    )
                })
                .collect()
        })
        .collect();

    // Compute column widths from VISIBLE width of rendered content
    let mut col_widths: Vec<usize> = rendered_headers.iter().map(|h| visible_width(h)).collect();
    let mut min_col_widths: Vec<usize> = rendered_headers
        .iter()
        .map(|h| longest_unbreakable_width(h).max(3))
        .collect();
    for row in &rendered_rows {
        for (c, cell) in row.iter().enumerate() {
            if c < col_widths.len() {
                col_widths[c] = col_widths[c].max(visible_width(cell));
                min_col_widths[c] = min_col_widths[c].max(longest_unbreakable_width(cell).max(3));
            }
        }
    }

    // Cap total width to viewport, shrinking proportionally if needed
    let border_overhead = num_cols + 1;
    let padding_overhead = num_cols * 2;
    let total = col_widths.iter().sum::<usize>() + border_overhead + padding_overhead;
    if total > ctx.viewport_width && ctx.viewport_width > border_overhead + padding_overhead {
        let avail = ctx.viewport_width - border_overhead - padding_overhead;
        shrink_col_widths_to_fit(&mut col_widths, avail, &min_col_widths);
    }

    let mut out = Vec::new();

    // Top border: ┌───┬───┐
    out.push(format!(
        "{}{}{}",
        ansi_dim("┌"),
        col_widths
            .iter()
            .map(|w| ansi_dim(&"─".repeat(w + 2)))
            .collect::<Vec<_>>()
            .join(&ansi_dim("┬")),
        ansi_dim("┐")
    ));

    // Header row: │ H1 │ H2 │
    out.extend(render_table_row(&rendered_headers, &col_widths));

    // Separator: ├───┼───┤
    out.push(format!(
        "{}{}{}",
        ansi_dim("├"),
        col_widths
            .iter()
            .map(|w| ansi_dim(&"─".repeat(w + 2)))
            .collect::<Vec<_>>()
            .join(&ansi_dim("┼")),
        ansi_dim("┤")
    ));

    // Data rows
    for (idx, row) in rendered_rows.iter().enumerate() {
        out.extend(render_table_row(row, &col_widths));
        if idx + 1 < rendered_rows.len() {
            out.push(format!(
                "{}{}{}",
                ansi_dim("├"),
                col_widths
                    .iter()
                    .map(|w| ansi_dim(&"─".repeat(w + 2)))
                    .collect::<Vec<_>>()
                    .join(&ansi_dim("┼")),
                ansi_dim("┤")
            ));
        }
    }

    // Bottom border: └───┴───┘
    out.push(format!(
        "{}{}{}",
        ansi_dim("└"),
        col_widths
            .iter()
            .map(|w| ansi_dim(&"─".repeat(w + 2)))
            .collect::<Vec<_>>()
            .join(&ansi_dim("┴")),
        ansi_dim("┘")
    ));

    out
}

fn longest_unbreakable_width(text: &str) -> usize {
    split_words(text)
        .iter()
        .map(|word| visible_width(word))
        .max()
        .unwrap_or(0)
}

fn shrink_col_widths_to_fit(col_widths: &mut [usize], avail: usize, min_widths: &[usize]) {
    let content_total: usize = col_widths.iter().sum();
    if content_total == 0 {
        return;
    }

    let fallback_mins;
    let min_widths = if min_widths.iter().sum::<usize>() <= avail {
        min_widths
    } else {
        fallback_mins = vec![1; col_widths.len()];
        &fallback_mins
    };

    for w in col_widths.iter_mut() {
        *w = *w * avail / content_total;
    }
    for (idx, w) in col_widths.iter_mut().enumerate() {
        *w = (*w).max(min_widths.get(idx).copied().unwrap_or(3));
    }

    while col_widths.iter().sum::<usize>() > avail {
        if let Some((idx, _)) = col_widths
            .iter()
            .enumerate()
            .filter(|(idx, w)| **w > min_widths.get(*idx).copied().unwrap_or(3))
            .max_by_key(|(_, w)| **w)
        {
            col_widths[idx] -= 1;
        } else {
            break;
        }
    }
}

fn render_table_row(row: &[String], col_widths: &[usize]) -> Vec<String> {
    let wrapped_cells: Vec<Vec<String>> = row
        .iter()
        .enumerate()
        .map(|(c, styled)| wrap_table_cell(styled, col_widths.get(c).copied().unwrap_or(0)))
        .collect();
    let row_height = wrapped_cells
        .iter()
        .map(|cell| cell.len())
        .max()
        .unwrap_or(1);

    let mut lines = Vec::with_capacity(row_height);
    for line_idx in 0..row_height {
        let cells: Vec<String> = wrapped_cells
            .iter()
            .enumerate()
            .map(|(c, cell_lines)| {
                let content = cell_lines.get(line_idx).map(|s| s.as_str()).unwrap_or("");
                let col_w = col_widths.get(c).copied().unwrap_or(0);
                let pad = col_w.saturating_sub(visible_width(content));
                format!(" {}{} ", content, " ".repeat(pad))
            })
            .collect();
        lines.push(format!(
            "{}{}{}",
            ansi_dim("│"),
            cells.join(&ansi_dim("│")),
            ansi_dim("│")
        ));
    }

    lines
}

fn wrap_table_cell(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![String::new()];
    }

    let mut lines = Vec::new();
    for wrapped in wrap_line(text, width, "") {
        if visible_width(&wrapped) <= width {
            lines.push(wrapped);
        } else {
            lines.extend(split_overlong_ansi_line(&wrapped, width));
        }
    }

    let lines = make_lines_self_contained(&lines);
    if lines.is_empty() {
        vec![String::new()]
    } else {
        lines
    }
}

fn split_overlong_ansi_line(text: &str, width: usize) -> Vec<String> {
    if width == 0 {
        return vec![String::new()];
    }

    let mut parts = Vec::new();
    let mut remaining = text.to_string();

    while !remaining.is_empty() {
        if visible_width(&remaining) <= width {
            parts.push(remaining);
            break;
        }

        let (head, tail) = split_ansi_prefix_at_width(&remaining, width);
        if head.is_empty() {
            break;
        }
        parts.push(head);
        remaining = tail;
    }

    if parts.is_empty() {
        vec![text.to_string()]
    } else {
        parts
    }
}

fn split_ansi_prefix_at_width(text: &str, width: usize) -> (String, String) {
    let bytes = text.as_bytes();
    let mut i = 0;
    let mut visible = 0;

    while i < bytes.len() {
        if text[i..].starts_with(OCCURRENCE_MARKER_START) {
            if let Some(marker_end) = text[i..].find(OCCURRENCE_MARKER_END) {
                i += marker_end + OCCURRENCE_MARKER_END.len_utf8();
                continue;
            }
        }

        if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b'[' {
            i += 2;
            while i < bytes.len() && bytes[i] != b'm' {
                i += 1;
            }
            if i < bytes.len() {
                i += 1;
            }
            continue;
        }

        if visible == width {
            break;
        }

        if let Some(ch) = text[i..].chars().next() {
            i += ch.len_utf8();
            visible += 1;
        } else {
            break;
        }
    }

    (text[..i].to_string(), text[i..].to_string())
}

fn render_heading(level: usize, text: &str) -> String {
    match level {
        1 => ansi_bold_underline_256(205, &format!("\u{2588} {text}")),
        2 => ansi_bold_256(81, &format!("\u{258c} {text}")),
        3 => ansi_bold_256(114, &format!("\u{258e} {text}")),
        4 => ansi_bold_256(186, text),
        _ => ansi_256(244, text),
    }
}

// ---------------------------------------------------------------------------
// Metadata extraction (headings and links from raw markdown)
// ---------------------------------------------------------------------------

fn extract_metadata(content: &str) -> (Vec<Heading>, Vec<Link>) {
    let mut headings = Vec::new();
    let mut links = Vec::new();
    let mut seen_links = HashSet::new();

    for line in content.split('\n') {
        // ATX headings.
        if let Some((level, text)) = try_parse_heading(line) {
            let plain = strip_inline(&text).trim().to_string();
            if !plain.is_empty() {
                headings.push(Heading {
                    level,
                    text: plain,
                    line: 0,
                });
            }
        }

        // Links anywhere in line.
        extract_links_from_line(line, &mut links, &mut seen_links);
    }

    (headings, links)
}

fn extract_links_from_line(line: &str, links: &mut Vec<Link>, seen_links: &mut HashSet<String>) {
    let mut remaining = line;
    while let Some(start) = remaining.find('[') {
        if remaining[..start].ends_with("@file") {
            remaining = &remaining[start + 1..];
            continue;
        }
        let after = &remaining[start + 1..];
        if let Some(end) = after.find(']') {
            let label = after[..end].trim();
            let after_close = &after[end + 1..];
            if after_close.starts_with('(') {
                if let Some(paren_end) = after_close.find(')') {
                    let href = after_close[1..paren_end].trim();
                    if !href.is_empty() {
                        let text = if label.is_empty() { href } else { label };
                        let key = format!("{text}|{href}");
                        if !seen_links.contains(&key) {
                            seen_links.insert(key);
                            links.push(Link {
                                text: text.to_string(),
                                href: href.to_string(),
                            });
                        }
                    }
                    remaining = &after_close[paren_end + 1..];
                    continue;
                }
            }
            remaining = after;
        } else {
            break;
        }
    }
}

// ---------------------------------------------------------------------------
// Line normalization (matches Go/TS implementation)
// ---------------------------------------------------------------------------

fn normalize_rendered_lines(lines: &[String]) -> Vec<String> {
    if lines.is_empty() {
        return vec![String::new()];
    }

    let mut lines = trim_outer_blank_lines(lines);
    if lines.is_empty() {
        return vec![String::new()];
    }

    let indent = common_leading_indent(&lines);
    if indent > 0 {
        lines = lines
            .iter()
            .map(|l| trim_leading_indent(l, indent))
            .collect();
    }

    let lines = collapse_blank_runs(&lines, 1);
    if lines.is_empty() {
        vec![String::new()]
    } else {
        lines
    }
}

fn trim_outer_blank_lines(lines: &[String]) -> Vec<String> {
    let mut start = 0;
    while start < lines.len() && is_blank_line(&lines[start]) {
        start += 1;
    }
    let mut end = lines.len();
    while end > start && is_blank_line(&lines[end - 1]) {
        end -= 1;
    }
    lines[start..end].to_vec()
}

fn common_leading_indent(lines: &[String]) -> usize {
    let mut common: Option<usize> = None;
    for line in lines {
        if is_blank_line(line) {
            continue;
        }
        let indent = leading_indent_width(line);
        common = Some(match common {
            None => indent,
            Some(c) => c.min(indent),
        });
    }
    common.unwrap_or(0)
}

fn leading_indent_width(line: &str) -> usize {
    let mut width = 0;
    let bytes = line.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        // Skip ANSI escape sequences.
        if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b'[' {
            i += 2;
            while i < bytes.len() && bytes[i] != b'm' {
                i += 1;
            }
            if i < bytes.len() {
                i += 1;
            }
            continue;
        }
        if bytes[i] == b' ' || bytes[i] == b'\t' {
            width += 1;
            i += 1;
        } else {
            break;
        }
    }
    width
}

fn trim_leading_indent(line: &str, width: usize) -> String {
    if width == 0 || line.is_empty() {
        return line.to_string();
    }
    let mut result = String::new();
    let mut trimmed = 0;
    let bytes = line.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        // Skip ANSI escape sequences.
        if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b'[' {
            let start = i;
            i += 2;
            while i < bytes.len() && bytes[i] != b'm' {
                i += 1;
            }
            if i < bytes.len() {
                i += 1;
            }
            result.push_str(&line[start..i]);
            continue;
        }
        if trimmed >= width {
            result.push_str(&line[i..]);
            return result;
        }
        if bytes[i] != b' ' && bytes[i] != b'\t' {
            result.push_str(&line[i..]);
            return result;
        }
        trimmed += 1;
        i += 1;
    }
    result
}

fn collapse_blank_runs(lines: &[String], keep: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut blank_run = 0;
    for line in lines {
        if is_blank_line(line) {
            blank_run += 1;
            if blank_run <= keep {
                out.push(String::new());
            }
        } else {
            blank_run = 0;
            out.push(line.clone());
        }
    }
    out
}

// ---------------------------------------------------------------------------
// ANSI state propagation — make each line independently renderable
// ---------------------------------------------------------------------------

/// Ensure each line carries its own ANSI open/close codes so that displaying
/// any contiguous slice of lines produces correct styling.
fn make_lines_self_contained(lines: &[String]) -> Vec<String> {
    let mut result = Vec::new();
    let mut active: Vec<(String, String)> = Vec::new(); // (category, ansi_code)

    for line in lines {
        // Prepend any styles that were active at the end of the previous line.
        let prefix: String = if !active.is_empty() {
            active
                .iter()
                .map(|(_, code)| code.as_str())
                .collect::<Vec<_>>()
                .join("")
        } else {
            String::new()
        };

        // Walk this line's ANSI codes to update the active state.
        let bytes = line.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == 0x1b && i + 1 < bytes.len() && bytes[i + 1] == b'[' {
                let start = i;
                i += 2;
                let params_start = i;
                while i < bytes.len() && bytes[i] != b'm' {
                    i += 1;
                }
                let params = &line[params_start..i];
                if i < bytes.len() {
                    i += 1;
                }
                let _full_code = &line[start..i];
                apply_ansi_code(&mut active, params);
            } else {
                i += 1;
            }
        }

        // Append a reset if any styles remain open.
        if !prefix.is_empty() || !active.is_empty() {
            result.push(format!("{prefix}{line}\x1b[0m"));
        } else {
            result.push(line.clone());
        }
    }

    result
}

/// Update the active-style list based on a single SGR parameter string.
fn apply_ansi_code(active: &mut Vec<(String, String)>, params: &str) {
    if params.is_empty() || params == "0" {
        active.clear();
        return;
    }

    let parts: Vec<u32> = params.split(';').filter_map(|p| p.parse().ok()).collect();

    if parts.is_empty() {
        return;
    }

    match parts[0] {
        1 => set_active(active, "bold", &format!("\x1b[{params}m")),
        2 => set_active(active, "dim", &format!("\x1b[{params}m")),
        3 => set_active(active, "italic", &format!("\x1b[{params}m")),
        4 => set_active(active, "underline", &format!("\x1b[{params}m")),
        9 => set_active(active, "strikethrough", &format!("\x1b[{params}m")),
        22 => {
            remove_active(active, "bold");
            remove_active(active, "dim");
        }
        23 => remove_active(active, "italic"),
        24 => remove_active(active, "underline"),
        29 => remove_active(active, "strikethrough"),
        38 => set_active(active, "fg", &format!("\x1b[{params}m")),
        39 => remove_active(active, "fg"),
        _ => {}
    }
}

fn set_active(active: &mut Vec<(String, String)>, category: &str, code: &str) {
    if let Some(entry) = active.iter_mut().find(|(cat, _)| cat == category) {
        entry.1 = code.to_string();
    } else {
        active.push((category.to_string(), code.to_string()));
    }
}

fn remove_active(active: &mut Vec<(String, String)>, category: &str) {
    active.retain(|(cat, _)| cat != category);
}

// ---------------------------------------------------------------------------
// Heading line remapping (after normalization shifts line positions)
// ---------------------------------------------------------------------------

fn remap_heading_lines(headings: &mut [Heading], plain: &[String]) {
    let mut search_from = 0;
    for h in headings.iter_mut() {
        for i in search_from..plain.len() {
            if plain[i].contains(&h.text) {
                h.line = i;
                search_from = i + 1;
                break;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Main entry point
// ---------------------------------------------------------------------------

/// Render Markdown for terminal display and extract metadata.
///
/// `width` is clamped to a minimum of 20 columns.
pub fn render(content: &str, width: usize) -> RenderResult {
    render_with_viewport(content, width, width)
}

/// Render markdown with a separate viewport width for diagram blocks.
/// `width` controls text wrapping; `viewport_width` is the full terminal width
/// used for sketch diagrams that benefit from extra horizontal space.
pub fn render_with_viewport(content: &str, width: usize, viewport_width: usize) -> RenderResult {
    let width = width.max(20);
    let viewport_width = viewport_width.max(width);

    // Extract metadata from raw source.
    let (meta_headings, meta_links) = extract_metadata(content);
    let _ = meta_headings; // headings from render context are used instead

    // Parse and render blocks.
    let mut ctx = RenderContext {
        width,
        viewport_width,
        links: Vec::new(),
        seen_links: HashSet::new(),
        link_occurrences: Vec::new(),
        file_refs: Vec::new(),
        seen_file_refs: HashSet::new(),
        file_ref_occurrences: Vec::new(),
        headings: Vec::new(),
    };

    let blocks = parse_blocks(content);
    let raw_lines = render_blocks(&blocks, &mut ctx);

    // Normalize output lines.
    let mut lines = normalize_rendered_lines(&raw_lines);

    // Truncate lines exceeding viewport width.
    // Use viewport_width (not prose width) so diagrams aren't clipped.
    for line in &mut lines {
        if visible_width(line) > viewport_width {
            *line = truncate_to_width(line, viewport_width);
        }
    }

    // Make each line self-contained so viewport slicing preserves styling.
    lines = make_lines_self_contained(&lines);

    let marker_positions = extract_occurrence_positions(&lines);
    lines = lines
        .into_iter()
        .map(|line| strip_occurrence_markers(&line))
        .collect();

    // Build plain-text mirror.
    let plain: Vec<String> = lines
        .iter()
        .map(|l| strip_ansi(l).trim_end().to_string())
        .collect();

    // Remap heading line indices.
    remap_heading_lines(&mut ctx.headings, &plain);
    let link_occurrences = map_marked_link_occurrences(
        &ctx.link_occurrences,
        &marker_positions.link_positions,
        &plain,
    );
    let file_ref_occurrences = map_marked_file_ref_occurrences(
        &ctx.file_ref_occurrences,
        &marker_positions.file_ref_positions,
        &plain,
    );

    // Merge links from metadata extraction with those found during rendering.
    let mut all_links = ctx.links.clone();
    for ml in &meta_links {
        let key = format!("{}|{}", ml.text, ml.href);
        if !ctx.seen_links.contains(&key) {
            ctx.seen_links.insert(key);
            all_links.push(ml.clone());
        }
    }

    let rendered = lines.join("\n");

    RenderResult {
        rendered,
        lines,
        plain,
        headings: ctx.headings,
        links: all_links,
        link_occurrences,
        file_refs: ctx.file_refs,
        file_ref_occurrences,
    }
}

#[derive(Debug, Clone, Copy)]
struct OccurrencePosition {
    line: usize,
    start_col: usize,
    end_col: usize,
}

#[derive(Debug, Default)]
struct OccurrencePositions {
    link_positions: Vec<Option<OccurrencePosition>>,
    file_ref_positions: Vec<Option<OccurrencePosition>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct OccurrenceKey {
    kind: char,
    index: usize,
}

fn strip_occurrence_markers(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.char_indices().peekable();
    while let Some((_, ch)) = chars.next() {
        if ch == OCCURRENCE_MARKER_START {
            while let Some((_, c)) = chars.next() {
                if c == OCCURRENCE_MARKER_END {
                    break;
                }
            }
        } else {
            out.push(ch);
        }
    }
    out
}

fn extract_occurrence_positions(lines: &[String]) -> OccurrencePositions {
    let mut clean_line_widths = Vec::with_capacity(lines.len());
    let mut starts: HashMap<OccurrenceKey, (usize, usize)> = HashMap::new();
    let mut ends: HashMap<OccurrenceKey, (usize, usize)> = HashMap::new();

    for (line_index, line) in lines.iter().enumerate() {
        let plain = strip_ansi_preserving_occurrence_markers(line);
        let mut col = 0usize;
        let mut chars = plain.char_indices().peekable();
        while let Some((_, ch)) = chars.next() {
            if ch == OCCURRENCE_MARKER_START {
                let mut marker = String::new();
                while let Some((_, c)) = chars.next() {
                    if c == OCCURRENCE_MARKER_END {
                        break;
                    }
                    marker.push(c);
                }
                if let Some((key, edge)) = parse_occurrence_marker(&marker) {
                    match edge {
                        OCCURRENCE_START => {
                            starts.entry(key).or_insert((line_index, col));
                        }
                        OCCURRENCE_END => {
                            ends.entry(key).or_insert((line_index, col));
                        }
                        _ => {}
                    }
                }
            } else {
                col += UnicodeWidthStr::width(ch.to_string().as_str());
            }
        }
        clean_line_widths.push(col);
    }

    let mut positions = OccurrencePositions::default();
    for (key, (start_line, start_col)) in starts {
        let end = ends.get(&key).copied();
        let end_col = match end {
            Some((end_line, end_col)) if end_line == start_line => end_col,
            Some(_) | None => clean_line_widths
                .get(start_line)
                .copied()
                .unwrap_or(start_col)
                .max(start_col),
        };
        let position = OccurrencePosition {
            line: start_line,
            start_col,
            end_col: end_col.max(start_col),
        };
        if position.end_col <= position.start_col {
            continue;
        }
        match key.kind {
            OCCURRENCE_LINK => {
                if positions.link_positions.len() <= key.index {
                    positions.link_positions.resize(key.index + 1, None);
                }
                positions.link_positions[key.index] = Some(position);
            }
            OCCURRENCE_FILE_REF => {
                if positions.file_ref_positions.len() <= key.index {
                    positions.file_ref_positions.resize(key.index + 1, None);
                }
                positions.file_ref_positions[key.index] = Some(position);
            }
            _ => {}
        }
    }

    positions
}

fn parse_occurrence_marker(marker: &str) -> Option<(OccurrenceKey, char)> {
    let mut chars = marker.chars();
    let kind = chars.next()?;
    let edge = chars.next()?;
    if !matches!(kind, OCCURRENCE_LINK | OCCURRENCE_FILE_REF)
        || !matches!(edge, OCCURRENCE_START | OCCURRENCE_END)
    {
        return None;
    }
    let index = chars.as_str().parse::<usize>().ok()?;
    Some((OccurrenceKey { kind, index }, edge))
}

fn map_marked_link_occurrences(
    occurrences: &[Link],
    positions: &[Option<OccurrencePosition>],
    plain: &[String],
) -> Vec<LinkOccurrence> {
    let fallback = OnceLock::new();
    occurrences
        .iter()
        .enumerate()
        .filter_map(|(index, link)| {
            if let Some(Some(position)) = positions.get(index) {
                return Some(LinkOccurrence {
                    link: link.clone(),
                    line: position.line,
                    start_col: position.start_col,
                    end_col: position.end_col,
                });
            }
            fallback
                .get_or_init(|| map_link_occurrences(occurrences, plain))
                .get(index)
                .cloned()
        })
        .collect()
}

fn map_marked_file_ref_occurrences(
    occurrences: &[FileRef],
    positions: &[Option<OccurrencePosition>],
    plain: &[String],
) -> Vec<FileRefOccurrence> {
    let fallback = OnceLock::new();
    occurrences
        .iter()
        .enumerate()
        .filter_map(|(index, file_ref)| {
            if let Some(Some(position)) = positions.get(index) {
                return Some(FileRefOccurrence {
                    file_ref: file_ref.clone(),
                    line: position.line,
                    start_col: position.start_col,
                    end_col: position.end_col,
                });
            }
            fallback
                .get_or_init(|| map_file_ref_occurrences(occurrences, plain))
                .get(index)
                .cloned()
        })
        .collect()
}

fn map_link_occurrences(occurrences: &[Link], plain: &[String]) -> Vec<LinkOccurrence> {
    let mut mapped = Vec::with_capacity(occurrences.len());
    let mut consumed_by_needle: HashMap<String, Vec<usize>> = HashMap::new();
    let mut search_start = 0usize;

    for link in occurrences {
        let needle = link.text.trim();
        if needle.is_empty() {
            continue;
        }
        let candidates = link_match_candidates(needle);

        let mut found = None;

        for (line_index, line) in plain.iter().enumerate().skip(search_start) {
            if let Some((candidate, start_col, end_col)) =
                find_link_match_columns(line, &candidates, &mut consumed_by_needle, line_index)
            {
                found = Some((line_index, candidate, start_col, end_col));
                search_start = line_index;
                break;
            }
        }

        if found.is_none() {
            for (line_index, line) in plain.iter().enumerate() {
                if let Some((candidate, start_col, end_col)) =
                    find_link_match_columns(line, &candidates, &mut consumed_by_needle, line_index)
                {
                    found = Some((line_index, candidate, start_col, end_col));
                    break;
                }
            }
        }

        if let Some((line, _, start_col, end_col)) = found {
            mapped.push(LinkOccurrence {
                link: link.clone(),
                line,
                start_col,
                end_col,
            });
        }
    }

    mapped
}

fn find_link_match_columns(
    line: &str,
    candidates: &[String],
    consumed_by_needle: &mut HashMap<String, Vec<usize>>,
    line_index: usize,
) -> Option<(String, usize, usize)> {
    for candidate in candidates {
        let consumed_by_line = consumed_by_needle
            .entry(candidate.clone())
            .or_insert_with(|| vec![0usize; line_index + 1]);
        if consumed_by_line.len() <= line_index {
            consumed_by_line.resize(line_index + 1, 0);
        }

        if let Some((start_col, end_col)) =
            nth_match_columns(line, candidate, consumed_by_line[line_index])
        {
            consumed_by_line[line_index] += 1;
            return Some((candidate.clone(), start_col, end_col));
        }
    }

    None
}

fn link_match_candidates(label: &str) -> Vec<String> {
    let mut candidates = vec![label.to_string()];
    let words: Vec<&str> = label.split_whitespace().collect();
    for len in (1..words.len()).rev() {
        let candidate = words[..len].join(" ");
        if UnicodeWidthStr::width(candidate.as_str()) >= 4 {
            candidates.push(candidate);
        }
    }
    if label.starts_with('↪') {
        if let Some(first_word) = words.first() {
            let candidate = (*first_word).to_string();
            if !candidates.contains(&candidate) {
                candidates.push(candidate);
            }
        }
    }
    candidates
}

fn nth_match_columns(line: &str, needle: &str, occurrence: usize) -> Option<(usize, usize)> {
    let (start, _) = line.match_indices(needle).nth(occurrence)?;
    let end = start + needle.len();
    Some((
        UnicodeWidthStr::width(&line[..start]),
        UnicodeWidthStr::width(&line[..end]),
    ))
}

fn map_file_ref_occurrences(occurrences: &[FileRef], plain: &[String]) -> Vec<FileRefOccurrence> {
    let mut mapped = Vec::with_capacity(occurrences.len());
    let mut consumed_by_needle: HashMap<String, Vec<usize>> = HashMap::new();
    let mut search_start = 0usize;

    for file_ref in occurrences {
        let needle = format!("↪{}", file_ref.display());
        let candidates = link_match_candidates(&needle);
        let mut found = None;

        for (line_index, line) in plain.iter().enumerate().skip(search_start) {
            if let Some((_, start_col, end_col)) =
                find_link_match_columns(line, &candidates, &mut consumed_by_needle, line_index)
            {
                found = Some((line_index, start_col, end_col));
                search_start = line_index;
                break;
            }
        }

        if found.is_none() {
            for (line_index, line) in plain.iter().enumerate() {
                if let Some((_, start_col, end_col)) =
                    find_link_match_columns(line, &candidates, &mut consumed_by_needle, line_index)
                {
                    found = Some((line_index, start_col, end_col));
                    break;
                }
            }
        }

        if let Some((line, start_col, end_col)) = found {
            mapped.push(FileRefOccurrence {
                file_ref: file_ref.clone(),
                line,
                start_col,
                end_col,
            });
        }
    }

    mapped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sketch_blocks_render_as_diagrams() {
        let content = "```sketch\n{\"nodes\": [{\"id\": \"a\", \"label\": \"API\"}, {\"id\": \"b\", \"label\": \"DB\", \"kind\": \"store\"}], \"edges\": [{\"from\": \"a\", \"to\": \"b\", \"label\": \"query\"}]}\n```";
        let result = render_with_viewport(content, 72, 72);
        let plain = result.plain.join("\n");
        assert!(plain.contains("API"), "diagram nodes render");
        assert!(plain.contains("query"), "edge label renders");
        assert!(plain.contains("▼"), "edge arrow renders");
        assert!(!plain.contains("nodes"), "raw JSON is not shown");
    }

    #[test]
    fn sketch_blocks_too_wide_show_placeholder() {
        let content = "```sketch\n{\"title\": \"big one\", \"nodes\": [{\"id\": \"a\", \"label\": \"A very long service name in a very wide box\"}, {\"id\": \"b\", \"label\": \"Another very long service name beside it\"}]}\n```";
        let result = render_with_viewport(content, 30, 30);
        let plain = result.plain.join("\n");
        assert!(
            plain.contains("◆ sketch `big one`"),
            "placeholder names the diagram instead of truncating it"
        );
    }

    #[test]
    fn long_footnote_wraps_instead_of_suppressing_diagram() {
        // A tiny graph whose footnote, on one line, is far wider than the
        // viewport. The graph fits, so it must render with the note wrapped —
        // not collapse to a placeholder because of the note's width.
        let content = "```sketch\n{\"title\": \"tiny graph\", \"nodes\": [{\"id\": \"a\", \"label\": \"A\"}, {\"id\": \"b\", \"label\": \"B\"}], \"edges\": [{\"from\": \"a\", \"to\": \"b\"}], \"notes\": [{\"on\": \"a\", \"text\": \"this is a deliberately long footnote that on a single line would be far wider than the viewport and used to drag the whole diagram past the fit check even though the graph itself is tiny\"}]}\n```";
        let result = render_with_viewport(content, 120, 120);
        let plain = result.plain.join("\n");
        assert!(
            !plain.contains("◆ sketch"),
            "a tiny graph must render even with a long footnote"
        );
        assert!(plain.contains("[1] A —"), "footnote anchor renders");
        assert!(plain.contains("deliberately"), "footnote text is present");
        // The footnote wraps to the caption width (the readable floor for a tiny
        // diagram), not the full viewport — so it never sprawls edge-to-edge.
        let max = result
            .plain
            .iter()
            .map(|l| l.chars().count())
            .max()
            .unwrap_or(0);
        assert!(
            max <= 60,
            "footnote wraps to caption width (~56), not the 120-col viewport (got {max})"
        );
    }

    #[test]
    fn sketch_blocks_with_errors_fail_loudly() {
        let content = "```sketch\n{\"nodes\": [{\"id\": \"a\", \"label\": \"A\"}], \"edges\": [{\"from\": \"a\", \"to\": \"ghost\"}]}\n```";
        let result = render_with_viewport(content, 72, 72);
        let plain = result.plain.join("\n");
        assert!(
            plain.contains("✗ sketch:") && plain.contains("ghost"),
            "error names the offending reference"
        );
    }

    #[test]
    fn rust_code_blocks_are_syntax_highlighted() {
        let content = "```rust\nfn main() {\n    println!(\"hello\");\n}\n```";
        let result = render_with_viewport(content, 72, 72);

        assert!(
            result.rendered.contains("\x1b[38;2;"),
            "expected rust code block to include truecolor ANSI styling"
        );
        assert!(
            result.lines.iter().any(|line| line.contains("\x1b[0m")),
            "expected highlighted code lines to close their ANSI styling"
        );
        assert!(
            result.plain.iter().any(|line| line.contains("fn main()")),
            "expected highlighted code to preserve plain-text output"
        );
    }

    #[test]
    fn local_night_owl_theme_is_preferred_when_available() {
        let Some(home) = std::env::var_os("HOME").map(PathBuf::from) else {
            return;
        };
        let theme_path = home.join("src/config/yazi/night-owl.tmTheme");
        if !theme_path.exists() {
            return;
        }

        let theme = highlight_theme().expect("expected a syntax highlight theme");

        assert_eq!(theme.name.as_deref(), Some("Night Owl Custom"));
    }

    #[test]
    fn text_code_blocks_use_plain_rendering() {
        let content = "```text\nplain text\n```";
        let result = render_with_viewport(content, 72, 72);

        assert!(
            result.plain.iter().any(|line| line == "plain text"),
            "expected text code block to preserve plain-text output"
        );
        assert!(
            !result.rendered.contains("\x1b[2mplain text\x1b[22m"),
            "expected text code block to render without muted styling"
        );
    }

    #[test]
    fn math_code_blocks_style_expression_relations_and_answers() {
        let content = "```math\n(-8) + 5 = -3\nAnswer: -3\n```";
        let result = render_with_viewport(content, 72, 72);
        let plain = result.plain.join("\n");

        assert!(
            plain.contains("(-8) + 5 = -3"),
            "math block keeps searchable plain text: {plain}"
        );
        assert!(
            plain.contains("Answer: -3"),
            "answer label remains in plain text: {plain}"
        );
        assert!(
            result.rendered.contains("\x1b[1m8\x1b[22m"),
            "ordinary expression numbers receive calm emphasis"
        );
        assert!(
            result.rendered.contains("\x1b[1m\x1b[38;5;114m="),
            "relations receive strong math styling"
        );
        assert!(
            result.rendered.contains("\x1b[38;5;114m+\x1b[39m"),
            "primary expression operators remain readable as math signs"
        );
        assert!(
            result.rendered.contains("\x1b[1m\x1b[38;5;220m3"),
            "answer values receive the strongest emphasis"
        );
    }

    #[test]
    fn math_code_blocks_dim_annotations_and_pop_final_result() {
        let content = "```math\n(-6) + (-3): sizes 6 and 3 -> 6 + 3 = 9, both negative -> -9\n```";
        let result = render_with_viewport(content, 100, 100);

        assert!(
            !result.rendered.contains("│"),
            "math blocks should not insert decorative bars"
        );
        assert!(
            result.rendered.contains("\x1b[2msizes\x1b[22m"),
            "annotation prose should be dim"
        );
        assert!(
            result.rendered.contains("\x1b[38;5;81m6\x1b[39m"),
            "annotation numbers should stay readable as reasoning math"
        );
        assert!(
            result.rendered.contains("\x1b[38;5;81m+\x1b[39m"),
            "annotation operators should be visible but secondary"
        );
        assert!(
            result.rendered.contains("\x1b[38;5;81m=\x1b[39m"),
            "annotation relations should stay readable inside worked steps"
        );
        assert!(
            result.rendered.contains("\x1b[1m\x1b[38;5;114m->"),
            "final transformation marker should anchor the final result"
        );
        assert!(
            result
                .rendered
                .contains("\x1b[1m\x1b[38;5;220m-\x1b[39m\x1b[22m\x1b[1m\x1b[38;5;220m9"),
            "final result should pop as the answer"
        );
    }

    #[test]
    fn math_code_blocks_render_step_labels_as_controls() {
        let content = "```math\nEvaluate:  -3 + 4 * (-2) - (-5)\nStep 1  multiply first:   4 * (-2) = -8\n```";
        let result = render_with_viewport(content, 100, 100);

        assert!(
            result.rendered.contains("\x1b[1m\x1b[38;5;81mEvaluate"),
            "evaluate labels should render as compact math-block controls"
        );
        assert!(
            result.rendered.contains("\x1b[1m\x1b[38;5;81mStep 1"),
            "step labels should render as compact math-block controls"
        );
        assert!(
            result
                .rendered
                .contains("\x1b[1m\x1b[38;5;220m-\x1b[39m\x1b[22m\x1b[1m\x1b[38;5;220m8"),
            "step results should pop when a line performs a worked transformation"
        );
    }

    #[test]
    fn math_code_blocks_keep_reference_formulas_calm() {
        let content = "```math\na/d + b/d = (a + b)/d\n```";
        let result = render_with_viewport(content, 72, 72);

        assert!(
            result.rendered.contains("\x1b[38;5;250m(\x1b[39m"),
            "grouping marks inside expressions should stay visible"
        );
        assert!(
            !result.rendered.contains("\x1b[1m\x1b[38;5;220m("),
            "single-reference formulas should not make the right side look like a final answer"
        );
    }

    #[test]
    fn math_code_blocks_keep_prose_steps_supporting() {
        let content = "```math\nSigns differ (one negative, one positive) -> subtract the sizes.\nSizes: 9 and 4 -> 9 - 4 = 5\n```";
        let result = render_with_viewport(content, 100, 100);

        assert!(
            result.rendered.contains("\x1b[2mSigns\x1b[22m"),
            "prose-heavy lines inside math blocks should remain supporting text"
        );
        assert!(
            !result.rendered.contains("\x1b[1m\x1b[38;5;220msubtract"),
            "prose after an arrow should not be styled as a final numeric answer"
        );
        assert!(
            !result.rendered.contains("\x1b[1m\x1b[38;5;220m5"),
            "prose-heavy worked notes should not pop mini-calculations as answers"
        );
    }

    #[test]
    fn math_code_blocks_keep_trailing_reasons_secondary() {
        let content = "```math\n5 - 8        = 5 + (-8)   = -3      (different signs: 8 - 5 = 3, keep negative)\n5 - (-3)     = 5 + 3      = 8       (subtracting a negative adds)\n-4 - 6       = -4 + (-6)  = -10     (same signs: 4 + 6 = 10, keep negative)\n-4 - (-9)    = -4 + 9     = 5       (different signs: 9 - 4 = 5, keep positive)\n```";
        let result = render_with_viewport(content, 100, 100);
        let reason_starts: Vec<usize> = result
            .plain
            .iter()
            .filter_map(|line| {
                ["(different signs", "(subtracting", "(same signs"]
                    .into_iter()
                    .find_map(|marker| line.find(marker))
            })
            .collect();

        assert!(
            result
                .rendered
                .contains("\x1b[1m\x1b[38;5;220m-\x1b[39m\x1b[22m\x1b[1m\x1b[38;5;220m3"),
            "main equation result should pop"
        );
        assert!(
            !result
                .rendered
                .contains("\x1b[1m\x1b[38;5;81mdifferent signs"),
            "trailing reason labels should not render as controls"
        );
        assert!(
            !result
                .rendered
                .contains("\x1b[1m\x1b[38;5;220m3\x1b[39m\x1b[22m, keep negative"),
            "mini-calculation inside the reason should not compete as an answer"
        );
        assert_eq!(reason_starts.len(), 4, "expected four trailing reasons");
        assert!(
            reason_starts.windows(2).all(|pair| pair[0] == pair[1]),
            "trailing reason notes should align as one column: {reason_starts:?}"
        );
    }

    #[test]
    fn math_code_blocks_support_equation_alias() {
        let content = "```equation\nx <= 5\n```";
        let result = render_with_viewport(content, 72, 72);

        assert!(
            result.plain.iter().any(|line| line.contains("x <= 5")),
            "equation alias should preserve expression text"
        );
        assert!(
            result.rendered.contains("\x1b[1m\x1b[38;5;114m<="),
            "compound relations receive relation styling"
        );
    }

    #[test]
    fn math_code_blocks_mute_ascii_guides() {
        let content = "```math\n... -3, -2, -1, 0, 1, 2, 3 ...\n<----|---->\n```";
        let result = render_with_viewport(content, 72, 72);

        assert!(
            result.plain.iter().any(|line| line.contains("<----|---->")),
            "guide line remains searchable"
        );
        assert!(
            result.rendered.contains("\x1b[38;5;244m----"),
            "dash guide runs are muted"
        );
        assert!(
            result.rendered.contains("\x1b[38;5;244m..."),
            "ellipsis guide runs are muted"
        );
        assert!(
            result.rendered.contains("\x1b[38;5;114m-\x1b[39m\x1b[1m3"),
            "negative signs still render as operators before emphasized numbers"
        );
    }

    #[test]
    fn file_refs_are_extracted_and_styled() {
        let result = render("See @file:src/store.rs:1290 for the bug.", 80);
        assert_eq!(result.file_refs.len(), 1);
        assert_eq!(result.file_ref_occurrences.len(), 1);
        assert_eq!(result.file_refs[0].path, "src/store.rs");
        assert_eq!(result.file_refs[0].line, Some(1290));
        assert_eq!(result.file_refs[0].col, None);
        assert_eq!(result.file_refs[0].label, None);
        assert_eq!(result.file_ref_occurrences[0].file_ref.path, "src/store.rs");
        assert_eq!(result.file_ref_occurrences[0].line, 0);
        assert_eq!(result.file_ref_occurrences[0].start_col, "See ".len());
        assert_eq!(
            result.file_ref_occurrences[0].end_col,
            "See ↪store.rs:1290".chars().count()
        );
        let plain = result.plain.join("\n");
        // Bare chip shows the basename, not the full path.
        assert!(plain.contains("↪store.rs:1290"), "chip rendered: {plain}");
        assert!(!plain.contains("@file"), "raw token consumed: {plain}");
    }

    #[test]
    fn file_refs_support_labeled_form() {
        let result = render("see @file[the store](/abs/store.rs:64) here", 80);
        assert_eq!(result.file_refs.len(), 1);
        assert_eq!(result.file_ref_occurrences.len(), 1);
        assert_eq!(result.file_refs[0].path, "/abs/store.rs");
        assert_eq!(result.file_refs[0].line, Some(64));
        assert_eq!(result.file_refs[0].label.as_deref(), Some("the store"));
        assert!(
            result.links.is_empty(),
            "@file[label](path) should not also be collected as a normal link"
        );
        let plain = result.plain.join("\n");
        assert!(plain.contains("↪the store"), "chip shows label: {plain}");
        assert!(!plain.contains("@file"), "token consumed: {plain}");
    }

    #[test]
    fn bare_urls_render_as_compact_links() {
        let url =
            "https://github.com/xepelinapp/xepelin-client-global/pull/4839#discussion_r3469081382";
        let result = render(&format!("Comment {url}"), 48);

        assert_eq!(result.links.len(), 1);
        assert_eq!(result.links[0].href, url);
        assert_eq!(result.links[0].text, "↗GH#r34…1382");
        assert_eq!(result.link_occurrences.len(), 1);
        assert_eq!(result.link_occurrences[0].link.href, url);
        assert_eq!(result.link_occurrences[0].line, 0);
        assert_eq!(result.link_occurrences[0].start_col, "Comment ".len());
        assert_eq!(
            result.link_occurrences[0].end_col,
            "Comment ↗GH#r34…1382".chars().count()
        );

        let plain = result.plain.join("\n");
        assert!(
            !plain.contains("https://github.com"),
            "raw wrapped URL should not be rendered: {plain}"
        );
        assert!(
            plain.contains("↗GH#r34…1382"),
            "compact link label should render: {plain}"
        );
    }

    #[test]
    fn repeated_links_preserve_rendered_occurrences() {
        let url = "https://github.com/owner/repo/pull/12#discussion_r123456";
        let result = render(&format!("{url}\n\nagain {url}"), 80);

        assert_eq!(result.links.len(), 1);
        assert_eq!(result.link_occurrences.len(), 2);
        assert_eq!(result.link_occurrences[0].link.href, url);
        assert_eq!(result.link_occurrences[1].link.href, url);
        assert_eq!(
            result
                .link_occurrences
                .iter()
                .map(|occurrence| occurrence.line)
                .collect::<Vec<_>>(),
            vec![0, 2]
        );
        assert_eq!(result.link_occurrences[0].start_col, 0);
        assert_eq!(result.link_occurrences[1].start_col, "again ".len());
    }

    #[test]
    fn wrapped_table_link_labels_preserve_occurrences() {
        let content = r#"
| Evidence |
| --- |
| [$first evidence label with many words](https://example.com/first), [$second evidence label with many words](https://example.com/second) |
"#;

        let result = render_with_viewport(content, 44, 44);
        let plain = result.plain.join("\n");

        assert_eq!(result.links.len(), 2);
        assert_eq!(
            result.link_occurrences.len(),
            2,
            "wrapped link labels should still be reachable by hint mode: {plain}"
        );
        assert_eq!(
            result
                .link_occurrences
                .iter()
                .map(|occurrence| occurrence.link.href.as_str())
                .collect::<Vec<_>>(),
            vec!["https://example.com/first", "https://example.com/second"]
        );
        assert!(result
            .link_occurrences
            .iter()
            .all(|occurrence| occurrence.end_col > occurrence.start_col));
    }

    #[test]
    fn table_link_occurrence_uses_rendered_target_not_earlier_matching_text() {
        let label = "RegisterFromBusinessInvitationUseCase";
        let content = format!(
            r#"
| Original concern | Evidence |
|---|---|
| `{label}` created the user before validation. | [{label}](https://example.com/register) validation evidence |
"#
        );

        let result = render_with_viewport(&content, 140, 140);
        let occurrence = result
            .link_occurrences
            .first()
            .expect("link occurrence should be mapped");
        let line = &result.plain[occurrence.line];
        let first = line.find(label).expect("earlier text should render");
        let second = line.rfind(label).expect("link label should render");

        assert_ne!(
            first, second,
            "test must contain matching text before the link"
        );
        assert_eq!(
            occurrence.start_col,
            UnicodeWidthStr::width(&line[..second]),
            "link hint should anchor to the rendered link target, not matching text in an earlier cell: {line}"
        );
        assert!(!result.rendered.contains(OCCURRENCE_MARKER_START));
        assert!(!result.plain.join("\n").contains(OCCURRENCE_MARKER_START));
    }

    #[test]
    fn table_file_ref_occurrence_uses_rendered_target_not_earlier_matching_text() {
        let label = "resolved name guard";
        let chip = format!("↪{label}");
        let content = format!(
            r#"
| Original concern | Evidence |
|---|---|
| `{chip}` appears in prose before the real file ref. | @file[{label}](/tmp/RegisterFromBusinessInvitationUseCase.ts:78) |
"#
        );

        let result = render_with_viewport(&content, 140, 140);
        let occurrence = result
            .file_ref_occurrences
            .first()
            .expect("file-ref occurrence should be mapped");
        let line = &result.plain[occurrence.line];
        let first = line.find(&chip).expect("earlier text should render");
        let second = line.rfind(&chip).expect("file-ref chip should render");

        assert_ne!(
            first, second,
            "test must contain matching text before the file ref"
        );
        assert_eq!(
            occurrence.start_col,
            UnicodeWidthStr::width(&line[..second]),
            "file hint should anchor to the rendered file ref, not matching text in an earlier cell: {line}"
        );
        assert!(!result.rendered.contains(OCCURRENCE_MARKER_START));
        assert!(!result.plain.join("\n").contains(OCCURRENCE_MARKER_START));
    }

    #[test]
    fn auth142_wrapped_table_file_ref_labels_preserve_occurrences() {
        let content = r#"
| Comment | Original concern | Decision and local status | Evidence |
|---|---|---|---|
| https://github.com/xepelinapp/xepelin-client-application/pull/1184#discussion_r3469082289 | `firstName ?? invitation.firstName` and `lastName ?? invitation.lastName` allowed empty strings to override invitation names. | Valid. Implemented by requiring non-empty values when optional fields are present and by trimming at the domain boundary before choosing the form name over the invitation fallback. | @file[DTO non-empty optional names](/Users/raulsaavedra/Projects/xepelin/xepelin-client-application/.worktrees/AUTH-142/apps/bff/src/modules/auth/dto/accept-invitation-request-dto.ts:28), @file[BFF name trimming](/Users/raulsaavedra/Projects/xepelin/xepelin-client-application/.worktrees/AUTH-142/apps/bff/src/modules/auth/auth.domain-service.ts:770), @file[whitespace-name test](/Users/raulsaavedra/Projects/xepelin/xepelin-client-application/.worktrees/AUTH-142/apps/bff/src/modules/auth/auth.domain-service.accept-invitation.spec.ts:102) |
"#;

        let result = render_with_viewport(content, 110, 110);
        let plain = result.plain.join("\n");

        assert_eq!(result.file_refs.len(), 3);
        assert_eq!(
            result.file_ref_occurrences.len(),
            3,
            "AUTH-142 wrapped evidence refs should all remain reachable by f hints: {plain}"
        );
        assert_eq!(
            result
                .file_ref_occurrences
                .iter()
                .map(|occurrence| occurrence.file_ref.label.as_deref())
                .collect::<Vec<_>>(),
            vec![
                Some("DTO non-empty optional names"),
                Some("BFF name trimming"),
                Some("whitespace-name test")
            ]
        );
        assert!(result
            .file_ref_occurrences
            .iter()
            .all(|occurrence| occurrence.end_col > occurrence.start_col));
    }

    #[test]
    fn auth142_viewport_edge_file_ref_label_preserves_occurrence() {
        let content = r#"
| Comment | Original concern | Decision and local status | Evidence |
|---|---|---|---|
| https://github.com/xepelinapp/xepelin-server-global/pull/5996#discussion_r3469083101 | `RegisterFromBusinessInvitationUseCase` accepts a form-provided name without matching the invited name, and the legacy path can still arrive with no form name while the invitation row has an empty `receiverName`. | Partially implemented. The empty-name create path now fails before `createUser`, which prevents a worse half-created user. It does not make the legacy flag-off acceptance path succeed when v3 invitation creation stored an empty name. The name-matching concern is rejected for the new flow because v3 invitation creation no longer asks the inviter for the invited user's name; the acceptance form is the source for the invited user's real name. | @file[empty-name guard](/Users/raulsaavedra/Projects/xepelin/xepelin-server-global/.worktrees/AUTH-142/src/features/business/users/useCases/RegisterFromBusinessInvitationUseCase.ts:78), @file[empty-name test](/Users/raulsaavedra/Projects/xepelin/xepelin-server-global/.worktrees/AUTH-142/test/unit/features/business/users/useCases/RegisterFromBusinessInvitationUseCase.spec.ts:148), @file[v3 invitation sends empty name](/Users/raulsaavedra/Projects/xepelin/xepelin-client-global/.worktrees/AUTH-142/src/modules/user-profile/users-management-v3/services/create-invitation.ts:9) |
"#;

        let result = render_with_viewport(content, 170, 170);
        let plain = result.plain.join("\n");

        assert_eq!(result.file_refs.len(), 3);
        assert_eq!(
            result.file_ref_occurrences.len(),
            3,
            "AUTH-142 viewport-edge file ref should remain reachable by f hints: {plain}"
        );
        assert_eq!(
            result
                .file_ref_occurrences
                .iter()
                .map(|occurrence| occurrence.file_ref.label.as_deref())
                .collect::<Vec<_>>(),
            vec![
                Some("empty-name guard"),
                Some("empty-name test"),
                Some("v3 invitation sends empty name")
            ]
        );
    }

    #[test]
    fn file_refs_dedupe_and_support_absolute_path_and_col() {
        let content = "@file:/abs/a.rs:12:5 then again @file:/abs/a.rs:12:5 and @file:rel/b.rs";
        let result = render(content, 120);
        assert_eq!(result.file_refs.len(), 2);
        assert_eq!(result.file_refs[0].path, "/abs/a.rs");
        assert_eq!(result.file_refs[0].line, Some(12));
        assert_eq!(result.file_refs[0].col, Some(5));
        assert_eq!(result.file_refs[1].path, "rel/b.rs");
        assert_eq!(result.file_refs[1].line, None);
        assert_eq!(
            result.file_ref_occurrences.len(),
            3,
            "occurrences preserve repeated jumps even when targets are deduped"
        );
    }

    #[test]
    fn repeated_file_ref_occurrences_map_to_their_rendered_lines() {
        let result = render("@file:src/a.rs\n\n@file:src/a.rs", 80);
        let lines: Vec<usize> = result
            .file_ref_occurrences
            .iter()
            .map(|occurrence| occurrence.line)
            .collect();

        assert_eq!(lines, vec![0, 2]);
    }

    #[test]
    fn file_refs_inside_code_spans_stay_literal() {
        let result = render("inline `@file:src/x.rs:1` code", 80);
        assert!(result.file_refs.is_empty());
        assert!(result
            .plain
            .iter()
            .any(|line| line.contains("@file:src/x.rs:1")));
    }

    #[test]
    fn tables_wrap_long_cells_instead_of_clipping_them() {
        let content = r#"
### Flujos

| Flujo | Hoy | Propuesta inicial |
| --- | --- | --- |
| **Crear invitación** | client-global llama a SG. SG valida rol en msUsers, valida reglas de invitación, crea `BusinessInvitation` y envía el email. | client-global llama a BFF. BFF valida permiso y rol en msUsers, llama a SG para crear `BusinessInvitation`, genera el link/token y envía el email. |
"#;

        let result = render_with_viewport(content, 72, 72);

        assert!(
            result
                .plain
                .iter()
                .any(|line| line.contains("BusinessInvitation")),
            "expected wrapped table output to preserve code-span content"
        );
        let rendered_plain = result.plain.join("\n");
        for expected in ["link/token", "envía", "email."] {
            assert!(
                rendered_plain.contains(expected),
                "expected long proposal cell token {expected:?} to be preserved"
            );
        }
        assert!(
            result
                .plain
                .iter()
                .filter(|line| line.starts_with('│'))
                .count()
                > 2,
            "expected the long table row to render as multiple terminal lines"
        );
        assert!(
            result.lines.iter().all(|line| visible_width(line) <= 72),
            "expected rendered table lines to fit the viewport without truncation"
        );
        assert!(
            result.plain.iter().any(|line| line.starts_with("├")),
            "expected wrapped tables to include row separators for readability"
        );
    }

    #[test]
    fn tables_render_bare_urls_without_splitting_the_href_text() {
        let url =
            "https://github.com/xepelinapp/xepelin-client-global/pull/4839#discussion_r3469081382";
        let content = format!(
            r#"
| Comment | Decision |
| --- | --- |
| {url} | Valid. Preserves backend message and submitted credentials. |
"#
        );

        let result = render_with_viewport(&content, 72, 72);
        let plain = result.plain.join("\n");

        assert_eq!(result.links.len(), 1);
        assert_eq!(result.links[0].href, url);
        assert_eq!(result.link_occurrences.len(), 1);
        assert_eq!(result.link_occurrences[0].link.href, url);
        assert!(
            !plain.contains("https://github.com/xepelinapp/xepelin-client-global"),
            "table should not render a partial raw URL: {plain}"
        );
        assert!(
            plain.contains("↗GH#r34…1382"),
            "table should render the compact link label: {plain}"
        );
        assert!(
            result.lines.iter().all(|line| visible_width(line) <= 72),
            "expected rendered table lines to fit the viewport"
        );
    }
}
