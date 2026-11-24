use ratatui::style::Style;
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum Mark {
    Link(usize),
    File(usize),
}

#[derive(Debug, Clone, PartialEq)]
pub(super) struct Piece {
    pub text: String,
    pub style: Style,
    pub mark: Option<Mark>,
}

impl Piece {
    pub fn new(text: impl Into<String>, style: Style) -> Piece {
        Piece {
            text: text.into(),
            style,
            mark: None,
        }
    }

    pub fn marked(text: impl Into<String>, style: Style, mark: Option<Mark>) -> Piece {
        Piece {
            text: text.into(),
            style,
            mark,
        }
    }
}

pub(super) type Row = Vec<Piece>;

pub(super) fn width(text: &str) -> usize {
    UnicodeWidthStr::width(text)
}

pub(super) fn row_width(row: &[Piece]) -> usize {
    row.iter().map(|piece| width(&piece.text)).sum()
}

fn cut(text: &str, room: usize) -> String {
    if room == 0 {
        return String::new();
    }
    let mut out = String::new();
    let mut used = 0;
    for character in text.chars() {
        let size = character.width().unwrap_or(0);
        if used + size + 1 > room {
            break;
        }
        out.push(character);
        used += size;
    }
    out.push('…');
    out
}

pub(super) fn fit_row(row: Row, room: usize) -> Row {
    if row_width(&row) <= room {
        return row;
    }
    let mut kept = Vec::new();
    let mut used = 0;
    for piece in row {
        let size = width(&piece.text);
        if used + size < room {
            used += size;
            kept.push(piece);
            continue;
        }
        kept.push(Piece {
            text: cut(&piece.text, room - used),
            ..piece
        });
        break;
    }
    kept
}

pub(super) fn wrap_row(row: Row, room: usize) -> Vec<Row> {
    if room == 0 || row_width(&row) <= room {
        return vec![row];
    }
    let cells: Vec<(char, usize, Style, Option<Mark>)> = row
        .into_iter()
        .flat_map(|piece| {
            let style = piece.style;
            let mark = piece.mark;
            piece
                .text
                .chars()
                .map(move |ch| (ch, ch.width().unwrap_or(0), style, mark))
                .collect::<Vec<_>>()
        })
        .collect();
    let mut out = Vec::new();
    let mut pos = 0;
    while pos < cells.len() {
        let mut used = 0;
        let mut end = pos;
        let mut seen_non_space = false;
        let mut last_space: Option<usize> = None;
        while end < cells.len() {
            let (ch, size, _, _) = cells[end];
            if used + size > room {
                break;
            }
            if ch.is_whitespace() {
                if seen_non_space {
                    last_space = Some(end);
                }
            } else {
                seen_non_space = true;
            }
            used += size;
            end += 1;
        }
        if end == pos {
            end += 1;
        }
        if end == cells.len() {
            out.push(pack_cells(&cells[pos..end]));
            break;
        }
        let (emit_end, next_pos) = if cells[end].0.is_whitespace() && seen_non_space {
            (end, end + 1)
        } else if let Some(space) = last_space {
            (space, space + 1)
        } else {
            (end, end)
        };
        out.push(pack_cells(&cells[pos..emit_end]));
        pos = next_pos;
    }
    out
}

fn pack_cells(cells: &[(char, usize, Style, Option<Mark>)]) -> Row {
    let mut row: Row = Vec::new();
    for &(ch, _, style, mark) in cells {
        if let Some(last) = row.last_mut().filter(|piece| piece.style == style && piece.mark == mark) {
            last.text.push(ch);
        } else {
            row.push(Piece::marked(ch.to_string(), style, mark));
        }
    }
    row
}

fn split_at_width(text: &str, max_width: usize) -> usize {
    let mut used = 0;
    for (offset, character) in text.char_indices() {
        let size = character.width().unwrap_or(0);
        if used + size > max_width {
            if offset > 0 {
                return offset;
            }
            return character.len_utf8();
        }
        used += size;
    }
    text.len()
}

pub(super) fn wrap(pieces: &[Piece], room: usize, first: Row, rest: Row) -> Vec<Row> {
    let first_width = row_width(&first);
    let rest_width = row_width(&rest);
    let mut rows = Vec::new();
    let mut current = first;
    let mut used = first_width;
    let mut limit = room.max(first_width + 4);
    let mut line_has_text = false;
    let mut pending_space: Option<(Style, Option<Mark>)> = None;
    for piece in pieces {
        for word in split_words(&piece.text) {
            if word.chars().all(char::is_whitespace) {
                if line_has_text {
                    pending_space = Some((piece.style, piece.mark));
                }
                continue;
            }
            let word_width = width(&word);
            let space = usize::from(pending_space.is_some());
            if line_has_text && used + space + word_width > limit {
                rows.push(std::mem::take(&mut current));
                current = rest.clone();
                used = rest_width;
                limit = room.max(rest_width + 4);
                line_has_text = false;
                pending_space = None;
            }
            if let Some((style, mark)) = pending_space.take() {
                let mark = mark.filter(|mark| Some(*mark) == piece.mark);
                current.push(Piece::marked(" ", style, mark));
                used += 1;
            }
            let mut tail = word.as_str();
            while !tail.is_empty() {
                let avail = limit.saturating_sub(used).max(1);
                if width(tail) <= avail {
                    used += width(tail);
                    current.push(Piece::marked(tail, piece.style, piece.mark));
                    line_has_text = true;
                    break;
                }
                let split = split_at_width(tail, avail);
                let (head, next) = tail.split_at(split);
                current.push(Piece::marked(head, piece.style, piece.mark));
                rows.push(std::mem::take(&mut current));
                current = rest.clone();
                used = rest_width;
                limit = room.max(rest_width + 4);
                line_has_text = false;
                tail = next;
            }
        }
    }
    if line_has_text || rows.is_empty() {
        rows.push(current);
    }
    rows
}

fn split_words(text: &str) -> Vec<String> {
    let mut words = Vec::new();
    let mut current = String::new();
    let mut in_space = false;
    for character in text.chars() {
        let space = character.is_whitespace();
        if space != in_space && !current.is_empty() {
            words.push(std::mem::take(&mut current));
        }
        in_space = space;
        current.push(character);
    }
    if !current.is_empty() {
        words.push(current);
    }
    words
}

#[cfg(test)]
mod tests {
    use super::{fit_row, wrap, wrap_row, Piece};
    use ratatui::style::{Color, Style};

    fn texts(rows: &[Vec<Piece>]) -> Vec<String> {
        rows.iter()
            .map(|row| row.iter().map(|piece| piece.text.as_str()).collect())
            .collect()
    }

    #[test]
    fn fit_row_keeps_styles_and_ends_in_an_ellipsis() {
        let row = vec![Piece::new("let ", Style::default()), Piece::new("value = 1;", Style::default())];
        assert_eq!(texts(&[fit_row(row.clone(), 8)]), vec!["let val…"]);
        assert_eq!(texts(&[fit_row(row, 4)]), vec!["let…"]);
    }

    #[test]
    fn wrap_row_breaks_styled_code_on_spaces_and_splits_oversized_tokens() {
        let key = Style::default().fg(Color::Cyan);
        let ident = Style::default().fg(Color::Yellow);
        let row = vec![
            Piece::new("  let ", key),
            Piece::new("very_long_identifier_name", ident),
        ];
        let wrapped = wrap_row(row, 10);
        assert_eq!(texts(&wrapped), vec!["  let", "very_long_", "identifier", "_name"]);
        assert_eq!(wrapped[0][0].style, key);
        assert_eq!(wrapped[1][0].style, ident);
        assert_eq!(wrapped[2][0].style, ident);
    }

    #[test]
    fn wrap_breaks_between_words_and_splits_oversized_tokens() {
        let pieces = vec![Piece::new("one two three four", Style::default())];
        let rows = wrap(&pieces, 9, vec![Piece::new("- ", Style::default())], vec![Piece::new("  ", Style::default())]);
        assert_eq!(texts(&rows), vec!["- one two", "  three", "  four"]);

        let long = vec![Piece::new("abcdefghij", Style::default())];
        let split = wrap(&long, 6, vec![Piece::new("- ", Style::default())], vec![Piece::new("  ", Style::default())]);
        assert_eq!(texts(&split), vec!["- abcd", "  efgh", "  ij"]);
    }
}
