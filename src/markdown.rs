use std::collections::HashSet;
use unicode_width::UnicodeWidthStr;

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
}

// ---------------------------------------------------------------------------
// ANSI helpers
// ---------------------------------------------------------------------------

/// Strip all ANSI escape sequences from a string.
fn strip_ansi(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.char_indices().peekable();
    while let Some((_i, ch)) = chars.next() {
        if ch == '\x1b' {
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
// Inline rendering — processes bold, italic, code, links, strikethrough
// ---------------------------------------------------------------------------

/// Render inline markdown to ANSI-styled text. Also collects links.
fn render_inline(text: &str, links: &mut Vec<Link>, seen_links: &mut HashSet<String>) -> String {
    let mut text = text.to_string();

    // 1. Protect code spans — replace with placeholders, render after.
    let mut code_spans: Vec<String> = Vec::new();
    text = regex_replace_all(&text, r"`([^`]+)`", |caps: &[&str]| {
        let idx = code_spans.len();
        code_spans.push(ansi_cyan(caps[1]));
        format!("\x00CS{idx}\x00")
    });

    // 2. Links: [text](url)
    text = regex_replace_all_with_links(&text, links, seen_links);

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
    text = regex_replace_all(&text, r"_{2}(.+?)_{2}", |caps: &[&str]| {
        ansi_bold(caps[1])
    });

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

    text
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
                    let trim_label = if label.trim().is_empty() { href } else { label.trim() };

                    let key = format!("{trim_label}|{href}");
                    if !href.is_empty() && !seen_links.contains(&key) {
                        seen_links.insert(key);
                        links.push(Link {
                            text: trim_label.to_string(),
                            href: href.to_string(),
                        });
                    }

                    result.push_str(&ansi_underline(trim_label));
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
            let limit = if is_first_line { width } else { width + hang_width };
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
    Heading { level: usize, text: String },
    Paragraph { text: String },
    List { ordered: bool, items: Vec<String> },
    Blockquote { lines: Vec<String> },
    Code { lang: String, code: String },
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
                let styled = render_inline(text, &mut ctx.links, &mut ctx.seen_links);
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
                    let styled = render_inline(item, &mut ctx.links, &mut ctx.seen_links);
                    let item_width = ctx.width.saturating_sub(bullet_plain_width);
                    let effective_width = if item_width > 10 { item_width } else { ctx.width };
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
                        let styled = render_inline(bq_line, &mut ctx.links, &mut ctx.seen_links);
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
                if lang == "mermaid" {
                    if let Some(lines) = crate::mermaid::render_flowchart(code, ctx.viewport_width) {
                        out.extend(lines);
                        out.push(String::new());
                        continue;
                    }
                }
                for code_line in code.split('\n') {
                    out.push(format!("  {}", ansi_dim(code_line)));
                }
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
        lines = lines.iter().map(|l| trim_leading_indent(l, indent)).collect();
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
            active.iter().map(|(_, code)| code.as_str()).collect::<Vec<_>>().join("")
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

    let parts: Vec<u32> = params
        .split(';')
        .filter_map(|p| p.parse().ok())
        .collect();

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
/// used for mermaid diagrams that benefit from extra horizontal space.
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

    // Build plain-text mirror.
    let plain: Vec<String> = lines.iter().map(|l| strip_ansi(l).trim_end().to_string()).collect();

    // Remap heading line indices.
    remap_heading_lines(&mut ctx.headings, &plain);

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
    }
}
