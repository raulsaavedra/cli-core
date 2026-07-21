//! Styled cell grid: the single render artifact.
//!
//! Two write paths with different composition rules:
//!  - strokes carry a direction mask (N/E/S/W) and *compose* into junction
//!    glyphs (`┼`, `├`, ...). Layout can then mark an independent crossing as
//!    an overpass without changing either route.
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
    EdgeBranch,
    EdgeLineEvent,
    EdgeLabel,
    EdgeLabelEvent,
    Ingress,
    IngressEvent,
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
        // The target-owning branch stays legible where it leaves a dim route,
        // while event relationships retain their stronger semantic color.
        if style_priority(style) > style_priority(cell.style) {
            cell.style = style;
        }
    }

    /// Write a single owned glyph such as a border corner or target ingress.
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

    /// Mark two unrelated routes crossing without connecting. Horizontal
    /// double and vertical single strokes read as an overpass instead of a
    /// four-way junction.
    pub fn crossover(&mut self, x: usize, y: usize) {
        let i = self.idx(x, y);
        let cell = &mut self.cells[i];
        debug_assert!(
            cell.mask & (N | S) != 0 && cell.mask & (E | W) != 0,
            "crossover at ({x},{y}) does not contain both axes"
        );
        cell.ch = '╪';
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
        Style::EdgeBranch => "\x1b[36m",
        Style::EdgeLineEvent => "\x1b[33m",
        Style::EdgeLabel => "\x1b[3;36m",
        Style::EdgeLabelEvent => "\x1b[3;33m",
        Style::Ingress => "\x1b[1;36m",
        Style::IngressEvent => "\x1b[1;33m",
        Style::Title => "\x1b[1;4m",
    }
}

fn style_priority(style: Style) -> usize {
    match style {
        Style::EdgeLineEvent => 3,
        Style::EdgeBranch => 2,
        Style::EdgeLine => 1,
        _ => 0,
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
        m if m == (N | E) => '╰',
        m if m == (N | W) => '╯',
        m if m == (S | E) => '╭',
        m if m == (S | W) => '╮',
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
