mod code;
mod layout;
mod refs;
mod theme;

use std::collections::HashMap;

use pulldown_cmark::{CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use layout::{fit_row, row_width, width, wrap, Mark, Piece, Row};

pub use refs::{definition_line, FileRef};
pub use theme::Theme;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Heading {
    pub level: usize,
    pub text: String,
    pub line: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Link {
    pub text: String,
    pub href: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LinkOccurrence {
    pub link: Link,
    pub line: usize,
    pub start_col: usize,
    pub end_col: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRefOccurrence {
    pub file_ref: FileRef,
    pub line: usize,
    pub start_col: usize,
    pub end_col: usize,
}

#[derive(Debug, Clone, Default)]
pub struct Rendered {
    pub lines: Vec<Line<'static>>,
    pub headings: Vec<Heading>,
    pub links: Vec<Link>,
    pub link_occurrences: Vec<LinkOccurrence>,
    pub file_refs: Vec<FileRef>,
    pub file_ref_occurrences: Vec<FileRefOccurrence>,
}

impl Rendered {
    pub fn plain(&self) -> Vec<String> {
        self.lines
            .iter()
            .map(|line| {
                line.spans
                    .iter()
                    .map(|span| span.content.as_ref())
                    .collect::<String>()
                    .trim_end()
                    .to_string()
            })
            .collect()
    }
}

pub fn render(source: &str, width: usize, theme: &Theme) -> Rendered {
    let mut writer = Writer::new(width.max(12), theme);
    let options = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    for event in Parser::new_ext(source, options) {
        writer.event(event);
    }
    writer.finish()
}

struct Table {
    rows: Vec<Vec<Vec<Piece>>>,
    head_rows: usize,
}

#[derive(Default)]
struct Fence {
    language: String,
    source: String,
}

struct OpenLink {
    mark: Mark,
    text: String,
}

struct Writer<'t> {
    theme: &'t Theme,
    room: usize,
    rows: Vec<Row>,
    spans: Vec<Piece>,
    styles: Vec<Style>,
    lists: Vec<Option<u64>>,
    item_prefix: Option<String>,
    quote: usize,
    heading: Option<HeadingLevel>,
    fence: Option<Fence>,
    table: Option<Table>,
    tight_item: bool,
    link: Option<OpenLink>,
    links: Vec<Link>,
    files: Vec<FileRef>,
    headings: Vec<Heading>,
}

impl<'t> Writer<'t> {
    fn new(room: usize, theme: &'t Theme) -> Writer<'t> {
        Writer {
            theme,
            room,
            rows: Vec::new(),
            spans: Vec::new(),
            styles: vec![Style::default().fg(theme.prose)],
            lists: Vec::new(),
            item_prefix: None,
            quote: 0,
            heading: None,
            fence: None,
            table: None,
            tight_item: false,
            link: None,
            links: Vec::new(),
            files: Vec::new(),
            headings: Vec::new(),
        }
    }

    fn style(&self) -> Style {
        self.styles.last().copied().unwrap_or_default()
    }

    fn push_style(&mut self, change: impl Fn(Style) -> Style) {
        let next = change(self.style());
        self.styles.push(next);
    }

    fn pop_style(&mut self) {
        if self.styles.len() > 1 {
            self.styles.pop();
        }
    }

    fn code_style(&self) -> Style {
        Style::default().fg(self.theme.code)
    }

    fn border_style(&self) -> Style {
        Style::default().fg(self.theme.border)
    }

    fn gap(&mut self) {
        if self.rows.last().is_some_and(|row| row_width(row) > 0) {
            self.rows.push(Row::new());
        }
    }

    fn indent(&self) -> usize {
        self.lists.len().saturating_sub(1) * 2 + if self.lists.is_empty() { 0 } else { 2 }
    }

    fn prefixes(&mut self) -> (Row, Row) {
        let mut first = Row::new();
        let mut rest = Row::new();
        let bar = Style::default().fg(self.theme.faint);
        for _ in 0..self.quote {
            first.push(Piece::new("▎ ", bar));
            rest.push(Piece::new("▎ ", bar));
        }
        let indent = self.indent();
        match self.item_prefix.take() {
            Some(marker) => {
                let lead = indent.saturating_sub(2);
                first.push(Piece::new(" ".repeat(lead), Style::default()));
                rest.push(Piece::new(" ".repeat(lead + width(&marker) + 1), Style::default()));
                first.push(Piece::new(format!("{marker} "), Style::default().fg(self.theme.heading)));
            }
            None => {
                first.push(Piece::new(" ".repeat(indent), Style::default()));
                rest.push(Piece::new(" ".repeat(indent), Style::default()));
            }
        }
        (first, rest)
    }

    fn flush(&mut self) {
        if self.spans.is_empty() && self.item_prefix.is_none() {
            return;
        }
        let spans = std::mem::take(&mut self.spans);
        if let Some(level) = self.heading {
            let text: String = spans.iter().map(|piece| piece.text.as_str()).collect();
            self.headings.push(Heading {
                level: level as usize,
                text: text.trim().to_string(),
                line: self.rows.len(),
            });
        }
        let (first, rest) = self.prefixes();
        let mut wrapped = wrap(&spans, self.room, first, rest);
        if self.heading.is_some_and(|level| level <= HeadingLevel::H2) {
            if let Some(row) = wrapped.last_mut() {
                let used = row_width(row);
                if used + 2 < self.room {
                    row.push(Piece::new(
                        format!(" {}", "─".repeat(self.room - used - 1)),
                        self.border_style(),
                    ));
                }
            }
        }
        self.rows.extend(wrapped);
    }

    fn push_piece(&mut self, piece: Piece) {
        if let Some(table) = self.table.as_mut() {
            if let Some(cell) = table.rows.last_mut().and_then(|row| row.last_mut()) {
                cell.push(piece);
            }
            return;
        }
        self.spans.push(piece);
    }

    fn text(&mut self, text: &str) {
        if let Some(fence) = self.fence.as_mut() {
            fence.source.push_str(text);
            return;
        }
        let style = self.style();
        if let Some(link) = self.link.as_mut() {
            link.text.push_str(text);
            let mark = Some(link.mark);
            self.push_piece(Piece::marked(text, style, mark));
            return;
        }
        for (part, url) in bare_urls(text) {
            let mark = url.then(|| {
                self.links.push(Link {
                    text: part.to_string(),
                    href: part.to_string(),
                });
                Mark::Link(self.links.len() - 1)
            });
            self.push_piece(Piece::marked(part, style, mark));
        }
    }

    fn inline_code(&mut self, code: &str) {
        let style = self.code_style();
        if let Some(link) = self.link.as_mut() {
            link.text.push_str(code);
            let mark = Some(link.mark);
            self.push_piece(Piece::marked(code, style, mark));
            return;
        }
        let mark = refs::mention(code).map(|file_ref| {
            self.files.push(file_ref);
            Mark::File(self.files.len() - 1)
        });
        self.push_piece(Piece::marked(code, style, mark));
    }

    fn event(&mut self, event: Event) {
        match event {
            Event::Start(tag) => self.start(tag),
            Event::End(tag) => self.end(tag),
            Event::Text(text) => self.text(&text),
            Event::Code(code) => self.inline_code(&code),
            Event::SoftBreak => self.text(" "),
            Event::HardBreak => self.flush(),
            Event::Rule => {
                self.flush();
                self.gap();
                let rule = Piece::new("─".repeat(self.room), self.border_style());
                self.rows.push(vec![rule]);
            }
            Event::TaskListMarker(done) => {
                self.item_prefix = Some(if done { "☑".into() } else { "☐".into() });
            }
            Event::Html(html) | Event::InlineHtml(html) => self.text(&html),
            _ => {}
        }
    }

    fn start(&mut self, tag: Tag) {
        match tag {
            Tag::Paragraph if !self.tight_item => self.gap_unless_item(),
            Tag::Heading { level, .. } => {
                self.flush();
                self.gap();
                self.heading = Some(level);
                let color = match level {
                    HeadingLevel::H1 | HeadingLevel::H2 => self.theme.heading,
                    HeadingLevel::H3 => self.theme.subheading,
                    _ => self.theme.minor,
                };
                self.styles.push(Style::default().fg(color).add_modifier(Modifier::BOLD));
            }
            Tag::BlockQuote(_) => {
                self.flush();
                self.gap();
                self.quote += 1;
                let color = self.theme.quote;
                self.push_style(|style| style.fg(color).add_modifier(Modifier::ITALIC));
            }
            Tag::CodeBlock(kind) => {
                self.flush();
                self.gap_unless_item();
                let language = match kind {
                    CodeBlockKind::Fenced(info) => info
                        .split(|character: char| character.is_whitespace() || character == ',')
                        .next()
                        .unwrap_or_default()
                        .to_string(),
                    CodeBlockKind::Indented => String::new(),
                };
                self.fence = Some(Fence {
                    language,
                    source: String::new(),
                });
            }
            Tag::List(start) => {
                self.flush();
                if self.lists.is_empty() {
                    self.gap();
                }
                self.lists.push(start);
            }
            Tag::Item => {
                self.flush();
                let marker = match self.lists.last_mut() {
                    Some(Some(number)) => {
                        let marker = format!("{number}.");
                        *number += 1;
                        marker
                    }
                    _ => "•".to_string(),
                };
                self.item_prefix = Some(marker);
                self.tight_item = true;
            }
            Tag::Emphasis => self.push_style(|style| style.add_modifier(Modifier::ITALIC)),
            Tag::Strong => {
                let (prose, strong) = (self.theme.prose, self.theme.strong);
                self.push_style(|style| {
                    let style = style.add_modifier(Modifier::BOLD);
                    if style.fg == Some(prose) {
                        style.fg(strong)
                    } else {
                        style
                    }
                });
            }
            Tag::Strikethrough => self.push_style(|style| style.add_modifier(Modifier::CROSSED_OUT)),
            Tag::Link { dest_url, .. } => {
                let mark = match refs::file_link(&dest_url, "") {
                    Some(file_ref) => {
                        self.files.push(file_ref);
                        Mark::File(self.files.len() - 1)
                    }
                    None => {
                        self.links.push(Link {
                            text: String::new(),
                            href: dest_url.to_string(),
                        });
                        Mark::Link(self.links.len() - 1)
                    }
                };
                self.link = Some(OpenLink {
                    mark,
                    text: String::new(),
                });
                let color = self.theme.link;
                self.push_style(|style| style.fg(color).add_modifier(Modifier::UNDERLINED));
            }
            Tag::Table(_) => {
                self.flush();
                self.gap();
                self.table = Some(Table {
                    rows: Vec::new(),
                    head_rows: 0,
                });
            }
            Tag::TableHead => {
                if let Some(table) = self.table.as_mut() {
                    table.rows.push(Vec::new());
                }
                let color = self.theme.subheading;
                self.push_style(|style| style.fg(color).add_modifier(Modifier::BOLD));
            }
            Tag::TableRow => {
                if let Some(table) = self.table.as_mut() {
                    table.rows.push(Vec::new());
                }
            }
            Tag::TableCell => {
                if let Some(row) = self.table.as_mut().and_then(|table| table.rows.last_mut()) {
                    row.push(Vec::new());
                }
            }
            _ => {}
        }
    }

    fn gap_unless_item(&mut self) {
        if self.item_prefix.is_none() && self.lists.is_empty() {
            self.flush();
            self.gap();
        } else {
            self.flush();
        }
    }

    fn end(&mut self, tag: TagEnd) {
        match tag {
            TagEnd::Paragraph => self.flush(),
            TagEnd::Heading(_) => {
                self.flush();
                self.heading = None;
                self.pop_style();
            }
            TagEnd::BlockQuote(_) => {
                self.flush();
                self.quote = self.quote.saturating_sub(1);
                self.pop_style();
            }
            TagEnd::CodeBlock => {
                let fence = self.fence.take().unwrap_or_default();
                self.fence(&fence);
            }
            TagEnd::List(_) => {
                self.flush();
                self.lists.pop();
                if self.lists.is_empty() {
                    self.tight_item = false;
                }
            }
            TagEnd::Item => self.flush(),
            TagEnd::Emphasis | TagEnd::Strong | TagEnd::Strikethrough => self.pop_style(),
            TagEnd::Link => {
                self.pop_style();
                if let Some(link) = self.link.take() {
                    let text = link.text.trim().to_string();
                    match link.mark {
                        Mark::Link(index) => {
                            let entry = &mut self.links[index];
                            entry.text = if text.is_empty() { entry.href.clone() } else { text };
                        }
                        Mark::File(index) => {
                            self.files[index].label = (!text.is_empty()).then_some(text);
                        }
                    }
                }
            }
            TagEnd::TableHead => {
                if let Some(table) = self.table.as_mut() {
                    table.head_rows = table.rows.len();
                }
                self.pop_style();
            }
            TagEnd::Table => {
                if let Some(table) = self.table.take() {
                    self.table(table);
                }
            }
            _ => {}
        }
    }

    fn fence(&mut self, fence: &Fence) {
        let indent = self.indent();
        let room = self.room.saturating_sub(indent + 2);
        let source = fence.source.trim_end_matches('\n');
        let lines: Vec<Row> = match code::highlight(&fence.language, source) {
            Some(tokens) => tokens
                .into_iter()
                .map(|line| line.into_iter().map(|(text, style)| Piece::new(text, style)).collect())
                .collect(),
            None => source
                .lines()
                .map(|line| vec![Piece::new(line.replace('\t', "    "), self.code_style())])
                .collect(),
        };
        for line in lines {
            let mut row = vec![Piece::new(" ".repeat(indent + 2), Style::default())];
            row.extend(fit_row(line, room));
            self.rows.push(row);
        }
    }

    fn table(&mut self, table: Table) {
        let columns = table.rows.iter().map(Vec::len).max().unwrap_or(0);
        if columns == 0 {
            return;
        }
        let mut widths = vec![1usize; columns];
        for row in &table.rows {
            for (index, cell) in row.iter().enumerate() {
                widths[index] = widths[index].max(row_width(cell));
            }
        }
        let frame = columns * 3 + 1;
        while widths.iter().sum::<usize>() + frame > self.room {
            let Some((widest, _)) = widths.iter().enumerate().max_by_key(|(_, width)| **width) else {
                break;
            };
            if widths[widest] <= 6 {
                break;
            }
            widths[widest] -= 1;
        }
        let border = self.border_style();
        let rule = |left: &str, middle: &str, right: &str| {
            let parts: Vec<String> = widths.iter().map(|room| "─".repeat(room + 2)).collect();
            vec![Piece::new(format!("{left}{}{right}", parts.join(middle)), border)]
        };
        self.rows.push(rule("┌", "┬", "┐"));
        for (row_index, row) in table.rows.iter().enumerate() {
            let head = row_index < table.head_rows;
            let cells: Vec<Vec<Row>> = widths
                .iter()
                .enumerate()
                .map(|(index, room)| {
                    let cell = row.get(index).cloned().unwrap_or_default();
                    wrap(&cell, *room, Row::new(), Row::new())
                })
                .collect();
            let height = cells.iter().map(Vec::len).max().unwrap_or(1).max(1);
            for line_index in 0..height {
                let mut pieces = vec![Piece::new("│", border)];
                for (index, room) in widths.iter().enumerate() {
                    let line = cells[index].get(line_index).cloned().unwrap_or_default();
                    let free = room.saturating_sub(row_width(&line));
                    let (before, after) = if head { (free / 2, free - free / 2) } else { (0, free) };
                    pieces.push(Piece::new(" ".repeat(before + 1), Style::default()));
                    pieces.extend(line);
                    pieces.push(Piece::new(" ".repeat(after + 1), Style::default()));
                    pieces.push(Piece::new("│", border));
                }
                self.rows.push(pieces);
            }
            if row_index + 1 < table.rows.len() {
                self.rows.push(rule("├", "┼", "┤"));
            }
        }
        self.rows.push(rule("└", "┴", "┘"));
    }

    fn finish(mut self) -> Rendered {
        self.flush();
        while self.rows.last().is_some_and(|row| row_width(row) == 0) {
            self.rows.pop();
        }
        let places = locate(&self.rows);
        let mut rendered = Rendered {
            headings: self.headings,
            ..Rendered::default()
        };
        for (index, link) in self.links.into_iter().enumerate() {
            let Some(&(line, start_col, end_col)) = places.get(&Mark::Link(index)) else {
                continue;
            };
            if !rendered.links.contains(&link) {
                rendered.links.push(link.clone());
            }
            rendered.link_occurrences.push(LinkOccurrence {
                link,
                line,
                start_col,
                end_col,
            });
        }
        for (index, file_ref) in self.files.into_iter().enumerate() {
            let Some(&(line, start_col, end_col)) = places.get(&Mark::File(index)) else {
                continue;
            };
            if !rendered.file_refs.contains(&file_ref) {
                rendered.file_refs.push(file_ref.clone());
            }
            rendered.file_ref_occurrences.push(FileRefOccurrence {
                file_ref,
                line,
                start_col,
                end_col,
            });
        }
        rendered.lines = self
            .rows
            .into_iter()
            .map(|row| Line::from(row.into_iter().map(|piece| Span::styled(piece.text, piece.style)).collect::<Vec<_>>()))
            .collect();
        rendered
    }
}

fn locate(rows: &[Row]) -> HashMap<Mark, (usize, usize, usize)> {
    let mut places: HashMap<Mark, (usize, usize, usize)> = HashMap::new();
    for (line, row) in rows.iter().enumerate() {
        let mut col = 0;
        for piece in row {
            let size = width(&piece.text);
            if let Some(mark) = piece.mark {
                match places.get_mut(&mark) {
                    None => {
                        places.insert(mark, (line, col, col + size));
                    }
                    Some(place) if place.0 == line => place.2 = col + size,
                    Some(_) => {}
                }
            }
            col += size;
        }
    }
    places
}

fn bare_urls(text: &str) -> Vec<(&str, bool)> {
    let mut parts = Vec::new();
    let mut rest = text;
    while let Some(start) = [rest.find("https://"), rest.find("http://")].into_iter().flatten().min() {
        let tail = &rest[start..];
        let end = tail
            .find(|character: char| character.is_whitespace() || matches!(character, '<' | '>' | '"' | '\''))
            .unwrap_or(tail.len());
        let url = tail[..end].trim_end_matches(['.', ',', ';', ':', '!', '?', ')', ']', '}']);
        let scheme = if url.starts_with("https://") { 8 } else { 7 };
        if url.len() <= scheme {
            parts.push((&rest[..start + end], false));
            rest = &rest[start + end..];
            continue;
        }
        if start > 0 {
            parts.push((&rest[..start], false));
        }
        parts.push((url, true));
        rest = &rest[start + url.len()..];
    }
    if !rest.is_empty() {
        parts.push((rest, false));
    }
    parts
}

#[cfg(test)]
mod tests;
