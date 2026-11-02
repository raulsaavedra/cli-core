use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Mutex, OnceLock};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use ratatui::style::{Modifier, Style};
use serde::Deserialize;

use super::code::{Highlighter, Tokens};
use super::layout::{fit_row, row_width, Mark, Piece, Row};
use super::refs::{self, FileRef};
use super::theme::Theme;

pub(super) const KINDS: [&str; 4] = ["flow", "map", "states", "code"];
const COMMAND: &str = "explain-render";
const TIMEOUT: Duration = Duration::from_secs(5);
const CACHE_LIMIT: usize = 256;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub(super) struct Drawn {
    pub text: String,
    pub tone: String,
    #[serde(default)]
    pub language: Option<String>,
    #[serde(default)]
    pub room: Option<usize>,
    #[serde(default)]
    pub href: Option<String>,
}

pub(super) type Drawing = Vec<Vec<Drawn>>;
type Key = (String, String, usize);

struct Entry {
    drawing: Option<Drawing>,
    stamps: Vec<(PathBuf, Option<SystemTime>)>,
}

impl Entry {
    fn fresh(&self) -> bool {
        self.stamps.iter().all(|(path, stamp)| modified(path) == *stamp)
    }
}

#[derive(Default)]
pub(super) struct Drawings {
    entries: Mutex<HashMap<Key, Entry>>,
}

impl Drawings {
    pub fn draw(&self, kind: &str, source: &str, width: usize, run: impl Fn(&str, &str, usize) -> Option<Drawing>) -> Option<Drawing> {
        let key = (kind.to_string(), source.to_string(), width);
        if let Some(found) = self.fresh(&key) {
            return found;
        }
        let stamps = read_files(kind, source)
            .into_iter()
            .map(|path| {
                let stamp = modified(&path);
                (path, stamp)
            })
            .collect();
        let drawing = run(kind, source, width);
        if let Ok(mut entries) = self.entries.lock() {
            if entries.len() >= CACHE_LIMIT {
                entries.clear();
            }
            entries.insert(
                key,
                Entry {
                    drawing: drawing.clone(),
                    stamps,
                },
            );
        }
        drawing
    }

    fn fresh(&self, key: &Key) -> Option<Option<Drawing>> {
        let entries = self.entries.lock().ok()?;
        let entry = entries.get(key)?;
        entry.fresh().then(|| entry.drawing.clone())
    }

    pub fn missing(&self, requests: Vec<Key>) -> Vec<Key> {
        requests
            .into_iter()
            .filter(|key| self.fresh(key).is_none())
            .collect()
    }
}

fn drawings() -> &'static Drawings {
    static DRAWINGS: OnceLock<Drawings> = OnceLock::new();
    DRAWINGS.get_or_init(Drawings::default)
}

fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|metadata| metadata.modified()).ok()
}

fn read_files(kind: &str, source: &str) -> Vec<PathBuf> {
    if kind != "code" {
        return Vec::new();
    }
    let mut paths: Vec<PathBuf> = Vec::new();
    for line in source.lines().map(str::trim).filter(|line| !line.is_empty()) {
        let note = line
            .split_once(':')
            .is_some_and(|(number, _)| !number.is_empty() && number.trim().chars().all(|character| character.is_ascii_digit()));
        if note || line.starts_with('"') {
            continue;
        }
        let target = match line.rsplit_once(" @") {
            Some((target, reference)) if !reference.contains(char::is_whitespace) => target.trim_end(),
            _ => line,
        };
        let end = target.find(['#', ':']).unwrap_or(target.len());
        let path = FileRef {
            path: target[..end].to_string(),
            line: None,
            symbol: None,
            label: None,
        }
        .resolved_path();
        if !paths.contains(&path) {
            paths.push(path);
        }
    }
    paths
}

pub(super) fn draw(kind: &str, source: &str, width: usize) -> Option<Drawing> {
    drawings().draw(kind, source, width, run)
}

pub(super) fn prefetch(requests: Vec<Key>) {
    let missing = drawings().missing(requests);
    if missing.len() < 2 {
        return;
    }
    let handles: Vec<_> = missing
        .into_iter()
        .map(|(kind, source, width)| thread::spawn(move || draw(&kind, &source, width)))
        .collect();
    for handle in handles {
        let _ = handle.join();
    }
}

fn run(kind: &str, source: &str, width: usize) -> Option<Drawing> {
    let mut child = Command::new(COMMAND)
        .arg(kind)
        .arg(width.to_string())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdin = child.stdin.take()?;
    let input = source.to_string();
    let writer = thread::spawn(move || stdin.write_all(input.as_bytes()));
    let mut stdout = child.stdout.take()?;
    let reader = thread::spawn(move || {
        let mut output = String::new();
        stdout.read_to_string(&mut output).map(|_| output)
    });
    let deadline = Instant::now() + TIMEOUT;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(5)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    };
    let _ = writer.join();
    let output = reader.join().ok()?.ok()?;
    if !status.success() {
        return None;
    }
    serde_json::from_str(&output).ok()
}

pub(super) fn rows(drawing: Drawing, theme: &Theme, files: &mut Vec<FileRef>) -> Vec<Row> {
    let mut highlighter: Option<(String, Highlighter)> = None;
    drawing
        .into_iter()
        .map(|line| {
            let mut row = Row::new();
            for piece in line {
                if piece.tone == "code" {
                    row.extend(code(piece, theme, &mut highlighter));
                    continue;
                }
                let style = style_of(&piece.tone, theme);
                let mark = piece
                    .href
                    .as_deref()
                    .and_then(|href| refs::file_link(href, ""))
                    .map(|file_ref| {
                        files.push(file_ref);
                        Mark::File(files.len() - 1)
                    });
                row.push(Piece::marked(piece.text, style, mark));
            }
            row
        })
        .collect()
}

fn code(piece: Drawn, theme: &Theme, highlighter: &mut Option<(String, Highlighter)>) -> Row {
    let text = piece.text.replace('\t', "    ");
    let tokens: Row = match piece
        .language
        .as_deref()
        .and_then(|language| highlighted(language, &text, highlighter))
    {
        Some(tokens) => tokens
            .into_iter()
            .map(|(text, style)| Piece::new(text, style))
            .collect(),
        None => vec![Piece::new(text, Style::default().fg(theme.code))],
    };
    let Some(room) = piece.room else {
        return tokens;
    };
    let mut fitted = fit_row(tokens, room);
    let used = row_width(&fitted);
    if used < room {
        fitted.push(Piece::new(" ".repeat(room - used), Style::default()));
    }
    fitted
}

fn highlighted(language: &str, text: &str, highlighter: &mut Option<(String, Highlighter)>) -> Option<Tokens> {
    if highlighter
        .as_ref()
        .is_none_or(|(current, _)| current != language)
    {
        *highlighter = Some((language.to_string(), Highlighter::new(language)?));
    }
    highlighter.as_mut()?.1.line(text)
}

fn style_of(tone: &str, theme: &Theme) -> Style {
    let style = Style::default();
    match tone {
        "lane" => style.fg(theme.faint),
        "name" | "hot" => style.fg(theme.heading).add_modifier(Modifier::BOLD),
        "note" => style.fg(theme.quote).add_modifier(Modifier::ITALIC),
        "dim" => style.fg(theme.quote),
        "border" => style.fg(theme.border),
        "link" => style.fg(theme.link),
        "warning" => style.fg(theme.subheading),
        _ => style.fg(theme.prose),
    }
}

#[cfg(test)]
mod tests {
    use super::{draw, rows, Drawn};
    use crate::markdown::layout::row_width;
    use crate::markdown::Theme;
    use ratatui::style::Modifier;

    fn piece(text: &str, tone: &str) -> Drawn {
        Drawn {
            text: text.to_string(),
            tone: tone.to_string(),
            language: None,
            room: None,
            href: None,
        }
    }

    #[test]
    fn tones_take_the_theme_colors() {
        let theme = Theme::default();
        let drawn = rows(
            vec![vec![piece("│", "lane"), piece("Orders", "name"), piece("retries", "note")]],
            &theme,
            &mut Vec::new(),
        );
        let styles: Vec<_> = drawn[0].iter().map(|piece| piece.style).collect();
        assert_eq!(styles[0].fg, Some(theme.faint));
        assert_eq!(styles[1].fg, Some(theme.heading));
        assert!(styles[1].add_modifier.contains(Modifier::BOLD));
        assert_eq!(styles[2].fg, Some(theme.quote));
        assert!(styles[2].add_modifier.contains(Modifier::ITALIC));
    }

    #[test]
    fn code_fills_exactly_its_room_and_is_highlighted() {
        let theme = Theme::default();
        let long = Drawn {
            language: Some("typescript".into()),
            room: Some(12),
            ..piece("const key = input.idempotencyKey", "code")
        };
        let short = Drawn {
            language: Some("typescript".into()),
            room: Some(12),
            ..piece("return 1", "code")
        };
        let drawn = rows(vec![vec![long], vec![short]], &theme, &mut Vec::new());
        assert_eq!(row_width(&drawn[0]), 12);
        assert_eq!(row_width(&drawn[1]), 12);
        let text: String = drawn[0].iter().map(|piece| piece.text.as_str()).collect();
        assert_eq!(text, "const key =…");
        assert!(drawn[0].iter().any(|piece| piece.style.fg != Some(theme.code)));
    }

    #[test]
    fn a_header_link_becomes_a_file_reference() {
        let mut files = Vec::new();
        let drawn = rows(
            vec![vec![Drawn {
                href: Some("file:///repo/orders/create.ts#L4".into()),
                ..piece("orders/create.ts:4", "link")
            }]],
            &Theme::default(),
            &mut files,
        );
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "/repo/orders/create.ts");
        assert_eq!(files[0].line, Some(4));
        assert!(drawn[0][0].mark.is_some());
    }

    #[test]
    fn an_unknown_kind_draws_nothing() {
        assert!(draw("no-such-kind", "a -> b", 40).is_none());
    }
}
