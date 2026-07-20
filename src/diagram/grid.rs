//! Styled cell grid: the single render artifact.
//!
//! Two write paths with different composition rules:
//!  - strokes carry a direction mask (N/E/S/W) and *compose* — two strokes
//!    crossing a cell merge into the correct junction glyph (`┼`, `├`, ...).
//!  - text writes own their cells outright. A text write landing on an
//!    occupied cell is a layout bug; debug builds panic, release overwrites.
//!
//! Paint has no other way to touch cells, so every collision rule lives here.

pub const N: u8 = 1;
pub const E: u8 = 2;
pub const S: u8 = 4;
pub const W: u8 = 8;

/// Visual style of a cell, mapped to ANSI at blit time.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Style {
    #[default]
    Empty,
    Border,
    BorderStore,
    BorderQueue,
    BorderExternal,
    Label,
    LabelDecision,
    LabelExternal,
    LabelNote,
    LabelUncertain,
    NoteMarker,
    EdgeLine,
    EdgeLineEvent,
    EdgeLabel,
    Arrow,
    Title,
}

#[derive(Clone, Copy, Default)]
struct Cell {
    ch: char,
    style: Style,
    /// Direction mask when the cell holds a stroke; 0 for text/empty.
    mask: u8,
    /// True when the stroke is dashed (async/event edges, tethers).
    dashed: bool,
}

pub struct Grid {
    width: usize,
    height: usize,
    cells: Vec<Cell>,
}

impl Grid {
    pub fn new(width: usize, height: usize) -> Self {
        Self {
            width,
            height,
            cells: vec![
                Cell {
                    ch: ' ',
                    ..Cell::default()
                };
                width * height
            ],
        }
    }

    fn idx(&self, x: usize, y: usize) -> usize {
        debug_assert!(
            x < self.width && y < self.height,
            "cell ({x},{y}) out of bounds"
        );
        y * self.width + x
    }

    /// Compose a stroke into a cell. Crossings merge via the direction mask.
    pub fn stroke(&mut self, x: usize, y: usize, mask: u8, style: Style, dashed: bool) {
        let i = self.idx(x, y);
        let cell = &mut self.cells[i];
        if cell.mask == 0 && cell.ch != ' ' {
            // Stroke meeting text/border: layout must prevent this.
            debug_assert!(false, "stroke ({x},{y}) collides with text {:?}", cell.ch);
            return;
        }
        cell.mask |= mask;
        // Solid wins over dashed at junctions; a pure dashed line stays dashed.
        cell.dashed = if cell.mask == mask {
            dashed
        } else {
            cell.dashed && dashed
        };
        cell.ch = mask_to_char(cell.mask, cell.dashed);
        // Event styling wins so event edges stay visible through junctions.
        if cell.style == Style::Empty || style == Style::EdgeLineEvent {
            cell.style = style;
        }
    }

    /// Write a single owned glyph (arrowheads, border corners painted as text).
    pub fn put(&mut self, x: usize, y: usize, ch: char, style: Style) {
        let i = self.idx(x, y);
        let cell = &mut self.cells[i];
        debug_assert!(
            cell.ch == ' ' && cell.mask == 0,
            "glyph {ch:?} at ({x},{y}) collides with {:?}",
            cell.ch
        );
        *cell = Cell {
            ch,
            style,
            mask: 0,
            dashed: false,
        };
    }

    /// Write text left-to-right starting at (x, y).
    pub fn text(&mut self, x: usize, y: usize, s: &str, style: Style) {
        for (i, ch) in s.chars().enumerate() {
            self.put(x + i, y, ch, style);
        }
    }

    /// An arrowhead terminating a stroke: allowed to land on a stroke cell.
    pub fn arrow(&mut self, x: usize, y: usize, ch: char) {
        let i = self.idx(x, y);
        let cell = &mut self.cells[i];
        debug_assert!(
            cell.ch == ' ' || cell.mask != 0,
            "arrow at ({x},{y}) collides with text {:?}",
            cell.ch
        );
        *cell = Cell {
            ch,
            style: Style::Arrow,
            mask: 0,
            dashed: false,
        };
    }

    /// Blit the grid to ANSI strings — the degenerate, full-extent viewport.
    pub fn to_ansi_lines(&self) -> Vec<String> {
        let mut lines = Vec::with_capacity(self.height);
        for y in 0..self.height {
            let mut out = String::new();
            let mut current = Style::Empty;
            for x in 0..self.width {
                let cell = self.cells[y * self.width + x];
                if cell.style != current {
                    if current != Style::Empty {
                        out.push_str("\x1b[0m");
                    }
                    out.push_str(style_code(cell.style));
                    current = cell.style;
                }
                out.push(cell.ch);
            }
            if current != Style::Empty {
                out.push_str("\x1b[0m");
            }
            let trimmed = out.trim_end().to_string();
            lines.push(trimmed);
        }
        while lines.last().is_some_and(|l| l.is_empty()) {
            lines.pop();
        }
        lines
    }
}

fn style_code(style: Style) -> &'static str {
    match style {
        Style::Empty => "",
        Style::Border => "\x1b[2m",
        Style::BorderStore => "\x1b[35m",
        Style::BorderQueue => "\x1b[33m",
        Style::BorderExternal => "\x1b[2m",
        Style::Label => "\x1b[1m",
        Style::LabelDecision => "\x1b[1;33m",
        Style::LabelExternal => "\x1b[2;3m",
        Style::LabelNote => "\x1b[2;3m",
        Style::LabelUncertain => "\x1b[33;3m",
        Style::NoteMarker => "\x1b[1;36m",
        Style::EdgeLine => "\x1b[2m",
        Style::EdgeLineEvent => "\x1b[33m",
        Style::EdgeLabel => "\x1b[3m",
        Style::Arrow => "\x1b[36m",
        Style::Title => "\x1b[1;4m",
    }
}

fn mask_to_char(mask: u8, dashed: bool) -> char {
    match mask {
        m if m == (N | S) => {
            if dashed {
                '┆'
            } else {
                '│'
            }
        }
        m if m == (E | W) => {
            if dashed {
                '┄'
            } else {
                '─'
            }
        }
        m if m == (N | E) => '└',
        m if m == (N | W) => '┘',
        m if m == (S | E) => '┌',
        m if m == (S | W) => '┐',
        m if m == (N | E | S) => '├',
        m if m == (N | S | W) => '┤',
        m if m == (E | S | W) => '┬',
        m if m == (N | E | W) => '┴',
        m if m == (N | E | S | W) => '┼',
        m if m & (N | S) != 0 => '│',
        m if m & (E | W) != 0 => '─',
        _ => ' ',
    }
}
