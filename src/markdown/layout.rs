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

pub(super) fn fit(text: &str, room: usize) -> String {
    if width(text) <= room {
        return text.to_string();
    }
    cut(text, room)
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
            if word_width > limit.saturating_sub(used) && !line_has_text {
                let cut = fit(&word, limit.saturating_sub(used));
                used += width(&cut);
                current.push(Piece::marked(cut, piece.style, piece.mark));
            } else {
                used += word_width;
                current.push(Piece::marked(word, piece.style, piece.mark));
            }
            line_has_text = true;
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
    use super::{fit, fit_row, wrap, Piece};
    use ratatui::style::Style;

    fn texts(rows: &[Vec<Piece>]) -> Vec<String> {
        rows.iter()
            .map(|row| row.iter().map(|piece| piece.text.as_str()).collect())
            .collect()
    }

    #[test]
    fn fit_cuts_with_an_ellipsis_inside_the_room() {
        assert_eq!(fit("abcdef", 4), "abc…");
        assert_eq!(fit("abc", 4), "abc");
        assert_eq!(fit("abc", 0), "");
    }

    #[test]
    fn fit_row_keeps_styles_and_ends_in_an_ellipsis() {
        let row = vec![Piece::new("let ", Style::default()), Piece::new("value = 1;", Style::default())];
        assert_eq!(texts(&[fit_row(row.clone(), 8)]), vec!["let val…"]);
        assert_eq!(texts(&[fit_row(row, 4)]), vec!["let…"]);
    }

    #[test]
    fn wrap_breaks_between_words_and_hangs_the_rest() {
        let pieces = vec![Piece::new("one two three four", Style::default())];
        let rows = wrap(&pieces, 9, vec![Piece::new("- ", Style::default())], vec![Piece::new("  ", Style::default())]);
        assert_eq!(texts(&rows), vec!["- one two", "  three", "  four"]);
    }
}
