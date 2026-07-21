//! Pure painter: walks fully determined scene ops into the grid.
//!
//! Contains no collision logic and no fallbacks — layout already allocated
//! every cell. A collision here is a layout bug and panics in debug builds
//! (enforced inside [`Grid`]).

use super::grid::{Grid, E, N, S, W};
use super::layout::{BorderKind, Op, Scene};

pub fn paint(scene: &Scene) -> Grid {
    let mut grid = Grid::new(scene.width, scene.height);

    for op in &scene.ops {
        match op {
            Op::Box {
                x,
                y,
                w,
                h,
                content,
                content_style,
                ..
            } => {
                draw_box(&mut grid, op);
                let inner = w - 2;
                let len = content.chars().count();
                let pad = inner.saturating_sub(len) / 2;
                grid.text(x + 1 + pad, y + h / 2, content, *content_style);
            }
            Op::Stroke {
                cells,
                dashed,
                style,
            } => {
                for &(x, y, mask) in cells {
                    grid.stroke(x, y, mask, *style, *dashed);
                }
            }
            Op::Crossover { x, y } => {
                grid.crossover(*x, *y);
            }
            Op::Text { x, y, text, style } => {
                grid.text(*x, *y, text, *style);
            }
        }
    }

    grid
}

fn draw_box(grid: &mut Grid, op: &Op) {
    let Op::Box {
        x,
        y,
        w,
        h,
        border,
        border_style,
        ingresses,
        ..
    } = op
    else {
        unreachable!("draw_box receives a box operation");
    };
    let (tl, tr, bl, br, hor, ver) = match border {
        BorderKind::Solid => ('┌', '┐', '└', '┘', '─', '│'),
        BorderKind::Double => ('╔', '╗', '╚', '╝', '═', '║'),
    };
    let _ = (N, E, S, W); // box borders are owned glyphs, never composed strokes

    grid.put(*x, *y, tl, *border_style);
    grid.put(x + w - 1, *y, tr, *border_style);
    grid.put(*x, y + h - 1, bl, *border_style);
    grid.put(x + w - 1, y + h - 1, br, *border_style);
    for dx in 1..w - 1 {
        if let Some(ingress) = ingresses.iter().find(|ingress| ingress.x == x + dx) {
            grid.put(x + dx, *y, '▼', ingress.style);
        } else {
            grid.put(x + dx, *y, hor, *border_style);
        }
        grid.put(x + dx, y + h - 1, hor, *border_style);
    }
    for dy in 1..h - 1 {
        grid.put(*x, y + dy, ver, *border_style);
        grid.put(x + w - 1, y + dy, ver, *border_style);
    }
}
