use super::graph::EdgeStyle;
use super::layout::{BorderStyle, ContentStyle, LayoutBox, LayoutEdge, LayoutResult};

// ---------------------------------------------------------------------------
// Cell styles for ANSI output
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq)]
enum CellStyle {
    Empty,
    NodeBorder,
    NodeLabel,
    DiamondLabel,
    CircleLabel,
    EdgeLine,
    EdgeLabel,
    ArrowTip,
    SubgraphBorder,
    SubgraphLabel,
}

// ---------------------------------------------------------------------------
// Public entry point
// ---------------------------------------------------------------------------

pub fn render_to_lines(_graph: &super::graph::FlowGraph, layout: &LayoutResult) -> Vec<String> {
    let w = layout.root.width;
    let h = layout.root.height;

    if w == 0 || h == 0 {
        return Vec::new();
    }

    let mut grid = vec![vec![' '; w]; h];
    let mut styles = vec![vec![CellStyle::Empty; w]; h];

    // 1. Walk the layout tree and draw boxes (depth-first, parents first)
    draw_box(&mut grid, &mut styles, &layout.root, 0, 0);

    // 2. Draw edge lines/arrows (but don't overwrite node content)
    for edge in &layout.edges {
        draw_edge(&mut grid, &mut styles, edge);
    }

    // 3. Convert to ANSI-styled strings with 2-space indent
    let mut lines = Vec::with_capacity(h);
    for y in 0..h {
        let line = render_styled_line(&grid[y], &styles[y]);
        lines.push(format!("  {}", line));
    }

    // Trim trailing blank lines
    while lines.last().map_or(false, |l| l.trim().is_empty()) {
        lines.pop();
    }

    lines
}

// ---------------------------------------------------------------------------
// Box drawing (recursive tree walk)
// ---------------------------------------------------------------------------

fn draw_box(
    grid: &mut [Vec<char>],
    styles: &mut [Vec<CellStyle>],
    layout_box: &LayoutBox,
    abs_x: usize,
    abs_y: usize,
) {
    let max_y = grid.len();
    let max_x = if max_y > 0 { grid[0].len() } else { 0 };
    let w = layout_box.width;
    let h = layout_box.height;

    // Draw border if present
    if let Some(border_style) = layout_box.border {
        let is_subgraph = matches!(border_style, BorderStyle::Dashed);
        let border_cell_style = if is_subgraph {
            CellStyle::SubgraphBorder
        } else {
            CellStyle::NodeBorder
        };

        let (tl, tr, bl, br, horiz, vert) = match border_style {
            BorderStyle::Rounded => ('╭', '╮', '╰', '╯', '─', '│'),
            BorderStyle::Dashed => ('┌', '┐', '└', '┘', '┄', '┆'),
            BorderStyle::Solid => ('┌', '┐', '└', '┘', '─', '│'),
        };

        // Top border
        if abs_y < max_y {
            set(
                grid,
                styles,
                abs_x,
                abs_y,
                tl,
                border_cell_style,
                max_x,
                max_y,
            );
            for dx in 1..w.saturating_sub(1) {
                set(
                    grid,
                    styles,
                    abs_x + dx,
                    abs_y,
                    horiz,
                    border_cell_style,
                    max_x,
                    max_y,
                );
            }
            if w > 1 {
                set(
                    grid,
                    styles,
                    abs_x + w - 1,
                    abs_y,
                    tr,
                    border_cell_style,
                    max_x,
                    max_y,
                );
            }
        }

        // Side borders
        for dy in 1..h.saturating_sub(1) {
            set(
                grid,
                styles,
                abs_x,
                abs_y + dy,
                vert,
                border_cell_style,
                max_x,
                max_y,
            );
            if w > 1 {
                set(
                    grid,
                    styles,
                    abs_x + w - 1,
                    abs_y + dy,
                    vert,
                    border_cell_style,
                    max_x,
                    max_y,
                );
            }
        }

        // Bottom border
        if h > 1 {
            let by = abs_y + h - 1;
            set(grid, styles, abs_x, by, bl, border_cell_style, max_x, max_y);
            for dx in 1..w.saturating_sub(1) {
                set(
                    grid,
                    styles,
                    abs_x + dx,
                    by,
                    horiz,
                    border_cell_style,
                    max_x,
                    max_y,
                );
            }
            if w > 1 {
                set(
                    grid,
                    styles,
                    abs_x + w - 1,
                    by,
                    br,
                    border_cell_style,
                    max_x,
                    max_y,
                );
            }
        }

        // Draw label on the top border (for subgraphs)
        if let Some(ref label) = layout_box.label {
            let label_x = abs_x + 2;
            let label_style = if is_subgraph {
                CellStyle::SubgraphLabel
            } else {
                CellStyle::NodeLabel
            };
            for (i, ch) in label.chars().enumerate() {
                let cx = label_x + i;
                if cx < abs_x + w - 1 {
                    set(grid, styles, cx, abs_y, ch, label_style, max_x, max_y);
                }
            }
        }
    }

    // Draw content (centered in the middle row)
    if let Some(ref content) = layout_box.content {
        let content_style = match content.style {
            ContentStyle::NodeLabel => CellStyle::NodeLabel,
            ContentStyle::DiamondLabel => CellStyle::DiamondLabel,
            ContentStyle::CircleLabel => CellStyle::CircleLabel,
            // Edge labels don't come through as BoxContent; handled separately
        };

        let mid_y = abs_y + h / 2;
        let inner_w = w.saturating_sub(2); // inside borders
        let text_len = content.text.chars().count();
        let pad_left = inner_w.saturating_sub(text_len) / 2;

        for (i, ch) in content.text.chars().enumerate() {
            let cx = abs_x + 1 + pad_left + i;
            if cx < abs_x + w.saturating_sub(1) {
                set(grid, styles, cx, mid_y, ch, content_style, max_x, max_y);
            }
        }
    }

    // Draw children (recursive)
    let border_offset = if layout_box.border.is_some() { 1 } else { 0 };
    let inner_x = abs_x + border_offset + layout_box.padding.left;
    let inner_y = abs_y + border_offset + layout_box.padding.top;

    for child in &layout_box.children {
        draw_box(
            grid,
            styles,
            &child.layout_box,
            inner_x + child.x,
            inner_y + child.y,
        );
    }
}

// ---------------------------------------------------------------------------
// Edge drawing
// ---------------------------------------------------------------------------

fn draw_edge(grid: &mut [Vec<char>], styles: &mut [Vec<CellStyle>], edge: &LayoutEdge) {
    let max_y = grid.len();
    let max_x = if max_y > 0 { grid[0].len() } else { 0 };

    let fx = edge.from_x;
    let fy = edge.from_y;
    let tx = edge.to_x;
    let ty = edge.to_y;
    let is_arrow = matches!(edge.style, EdgeStyle::Arrow);

    let is_vertical = (fx == tx) || (fx as isize - tx as isize).unsigned_abs() <= 1;
    let is_td = ty > fy; // top-to-bottom direction

    if is_td {
        let has_label = edge.label.is_some();
        // Arrow 2 rows above target if labeled, 1 row if not
        let arrow_gap = if has_label { 2 } else { 1 };

        if is_vertical {
            // Straight vertical
            let x = tx;
            let arrow_y = if is_arrow {
                ty.saturating_sub(arrow_gap)
            } else {
                ty
            };
            for y in fy..arrow_y {
                set_edge(grid, styles, x, y, '│', CellStyle::EdgeLine, max_x, max_y);
            }
            if is_arrow {
                set_edge(
                    grid,
                    styles,
                    x,
                    arrow_y,
                    '▼',
                    CellStyle::ArrowTip,
                    max_x,
                    max_y,
                );
            }
        } else {
            // L-path: vertical down, corner, horizontal, arrow
            let turn_y = ty.saturating_sub(arrow_gap);

            // Vertical segment
            for y in fy..turn_y {
                set_edge(grid, styles, fx, y, '│', CellStyle::EdgeLine, max_x, max_y);
            }

            // Corner
            let corner_ch = if tx > fx { '└' } else { '┘' };
            set_edge(
                grid,
                styles,
                fx,
                turn_y,
                corner_ch,
                CellStyle::EdgeLine,
                max_x,
                max_y,
            );

            // Horizontal dashes
            let (inner_start, inner_end) = if tx > fx { (fx + 1, tx) } else { (tx + 1, fx) };
            for x in inner_start..inner_end {
                set_edge(
                    grid,
                    styles,
                    x,
                    turn_y,
                    '─',
                    CellStyle::EdgeLine,
                    max_x,
                    max_y,
                );
            }

            // Arrow or endpoint
            if is_arrow {
                set_edge(
                    grid,
                    styles,
                    tx,
                    turn_y,
                    '▼',
                    CellStyle::ArrowTip,
                    max_x,
                    max_y,
                );
            } else {
                set_edge(
                    grid,
                    styles,
                    tx,
                    turn_y,
                    '─',
                    CellStyle::EdgeLine,
                    max_x,
                    max_y,
                );
            }
        }

        // Label: centered above the target node, on the row between arrow and target
        if let Some(ref label) = edge.label {
            let label_y = ty.saturating_sub(1);
            // Center label on the target's x position
            let label_len = label.chars().count();
            let label_x = if label_len < 2 {
                tx
            } else {
                tx.saturating_sub(label_len / 2)
            };
            draw_label_at(grid, styles, label_x, label_y, label, max_x, max_y);
        }
    } else if tx > fx {
        // Left-to-right (LR layout)
        let is_horizontal = (fy == ty) || (fy as isize - ty as isize).unsigned_abs() <= 1;

        if is_horizontal {
            let y = ty;
            let arrow_x = if is_arrow { tx.saturating_sub(1) } else { tx };

            if let Some(ref label) = edge.label {
                // Embed label in horizontal segment
                let span = arrow_x.saturating_sub(fx);
                draw_label_on_horiz(grid, styles, fx, arrow_x, y, label, span, max_x, max_y);
            } else {
                for x in fx..arrow_x {
                    set_edge(grid, styles, x, y, '─', CellStyle::EdgeLine, max_x, max_y);
                }
            }
            if is_arrow {
                set_edge(
                    grid,
                    styles,
                    arrow_x,
                    y,
                    '▶',
                    CellStyle::ArrowTip,
                    max_x,
                    max_y,
                );
            }
        } else {
            // L-path for LR: horizontal, corner, vertical, arrow
            let turn_x = tx.saturating_sub(1);

            // Horizontal with inline label
            if let Some(ref label) = edge.label {
                let span = turn_x.saturating_sub(fx);
                draw_label_on_horiz(grid, styles, fx, turn_x, fy, label, span, max_x, max_y);
            } else {
                for x in fx..turn_x {
                    set_edge(grid, styles, x, fy, '─', CellStyle::EdgeLine, max_x, max_y);
                }
            }

            if ty > fy {
                set_edge(
                    grid,
                    styles,
                    turn_x,
                    fy,
                    '┐',
                    CellStyle::EdgeLine,
                    max_x,
                    max_y,
                );
                for y in (fy + 1)..ty {
                    set_edge(
                        grid,
                        styles,
                        turn_x,
                        y,
                        '│',
                        CellStyle::EdgeLine,
                        max_x,
                        max_y,
                    );
                }
            } else {
                set_edge(
                    grid,
                    styles,
                    turn_x,
                    fy,
                    '┘',
                    CellStyle::EdgeLine,
                    max_x,
                    max_y,
                );
                for y in (ty + 1)..fy {
                    set_edge(
                        grid,
                        styles,
                        turn_x,
                        y,
                        '│',
                        CellStyle::EdgeLine,
                        max_x,
                        max_y,
                    );
                }
            }

            if is_arrow {
                set_edge(
                    grid,
                    styles,
                    turn_x,
                    ty,
                    '▶',
                    CellStyle::ArrowTip,
                    max_x,
                    max_y,
                );
            }
        }
    }
}

/// Draw a label at a specific position, truncating if needed.
/// Labels overwrite edge lines but not node/subgraph content.
fn draw_label_at(
    grid: &mut [Vec<char>],
    styles: &mut [Vec<CellStyle>],
    x: usize,
    y: usize,
    label: &str,
    max_x: usize,
    max_y: usize,
) {
    let avail = max_x.saturating_sub(x);
    let chars: Vec<char> = label.chars().collect();
    if avail == 0 || y >= max_y {
        return;
    }
    let display: Vec<char> = if chars.len() <= avail {
        chars
    } else {
        let mut t: Vec<char> = chars.into_iter().take(avail.saturating_sub(1)).collect();
        t.push('…');
        t
    };
    for (i, &ch) in display.iter().enumerate() {
        let cx = x + i;
        if cx < max_x {
            // Overwrite empty and edge-line cells, but not node/subgraph content
            match styles[y][cx] {
                CellStyle::Empty | CellStyle::EdgeLine | CellStyle::ArrowTip => {
                    grid[y][cx] = ch;
                    styles[y][cx] = CellStyle::EdgeLabel;
                }
                _ => {}
            }
        }
    }
}

/// Draw label text embedded in a horizontal dash segment.
fn draw_label_on_horiz(
    grid: &mut [Vec<char>],
    styles: &mut [Vec<CellStyle>],
    start_x: usize,
    end_x: usize,
    y: usize,
    label: &str,
    _span: usize,
    max_x: usize,
    max_y: usize,
) {
    let total = end_x.saturating_sub(start_x);
    let chars: Vec<char> = label.chars().collect();
    let label_len = chars.len();

    if total >= label_len + 2 {
        // Fits with at least 1 dash before
        let pad = 1;
        for x in start_x..(start_x + pad) {
            set_edge(grid, styles, x, y, '─', CellStyle::EdgeLine, max_x, max_y);
        }
        for (i, &ch) in chars.iter().enumerate() {
            set_edge(
                grid,
                styles,
                start_x + pad + i,
                y,
                ch,
                CellStyle::EdgeLabel,
                max_x,
                max_y,
            );
        }
        for x in (start_x + pad + label_len)..end_x {
            set_edge(grid, styles, x, y, '─', CellStyle::EdgeLine, max_x, max_y);
        }
    } else if total > 2 {
        // Truncate
        let avail = total.saturating_sub(1);
        for i in 0..avail.saturating_sub(1).min(chars.len()) {
            set_edge(
                grid,
                styles,
                start_x + i,
                y,
                chars[i],
                CellStyle::EdgeLabel,
                max_x,
                max_y,
            );
        }
        if avail > 1 {
            set_edge(
                grid,
                styles,
                start_x + avail - 1,
                y,
                '…',
                CellStyle::EdgeLabel,
                max_x,
                max_y,
            );
        }
    } else {
        for x in start_x..end_x {
            set_edge(grid, styles, x, y, '─', CellStyle::EdgeLine, max_x, max_y);
        }
    }
}

fn set(
    grid: &mut [Vec<char>],
    styles: &mut [Vec<CellStyle>],
    x: usize,
    y: usize,
    ch: char,
    style: CellStyle,
    max_x: usize,
    max_y: usize,
) {
    if y < max_y && x < max_x {
        grid[y][x] = ch;
        styles[y][x] = style;
    }
}

/// Set a cell for an edge segment. Don't overwrite node or subgraph content.
fn set_edge(
    grid: &mut [Vec<char>],
    styles: &mut [Vec<CellStyle>],
    x: usize,
    y: usize,
    ch: char,
    style: CellStyle,
    max_x: usize,
    max_y: usize,
) {
    if y < max_y && x < max_x {
        match styles[y][x] {
            CellStyle::NodeBorder
            | CellStyle::NodeLabel
            | CellStyle::DiamondLabel
            | CellStyle::CircleLabel
            | CellStyle::SubgraphBorder
            | CellStyle::SubgraphLabel
            | CellStyle::EdgeLabel => {
                // Don't overwrite node cells
            }
            _ => {
                grid[y][x] = ch;
                styles[y][x] = style;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// ANSI rendering
// ---------------------------------------------------------------------------

fn render_styled_line(chars: &[char], cell_styles: &[CellStyle]) -> String {
    let mut out = String::new();
    let mut current = CellStyle::Empty;

    for (&ch, &style) in chars.iter().zip(cell_styles.iter()) {
        if style != current {
            if current != CellStyle::Empty {
                out.push_str("\x1b[0m");
            }
            match style {
                CellStyle::NodeBorder => out.push_str("\x1b[2m"), // dim
                CellStyle::NodeLabel => out.push_str("\x1b[1m"),  // bold
                CellStyle::DiamondLabel => out.push_str("\x1b[1;33m"), // bold yellow
                CellStyle::CircleLabel => out.push_str("\x1b[1;36m"), // bold cyan
                CellStyle::EdgeLine => out.push_str("\x1b[2m"),   // dim
                CellStyle::EdgeLabel => out.push_str("\x1b[3m"),  // italic
                CellStyle::ArrowTip => out.push_str("\x1b[36m"),  // cyan
                CellStyle::SubgraphBorder => out.push_str("\x1b[2m"), // dim
                CellStyle::SubgraphLabel => out.push_str("\x1b[36;1m"), // bold cyan
                CellStyle::Empty => {}
            }
            current = style;
        }
        out.push(ch);
    }

    if current != CellStyle::Empty {
        out.push_str("\x1b[0m");
    }

    let trimmed = out.trim_end();
    if trimmed.is_empty() {
        String::new()
    } else {
        trimmed.to_string()
    }
}
