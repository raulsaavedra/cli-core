//! Top-down architecture-diagram layout.
//!
//! Authored ranks establish vertical stages. Horizontal placement follows the
//! graph's branch structure, and every authored relationship keeps its own ports,
//! route, and target ingress. The final branch carries its caption into a port
//! embedded in the target box, while long relationships clear only the ranks
//! they cross. Final composition balances the complete routed graph.

mod placement;
mod routing;

use std::collections::HashMap;

use super::doc::{DiagramError, EdgeKind, Model, NodeKind, NoteMark};
use super::grid::{Style, N, S};

pub(super) const MARGIN_X: usize = 1;
pub(super) const MIN_CAPTION_WIDTH: usize = 12;
pub(super) const MAX_CAPTION_WIDTH: usize = 30;
pub(super) const CAPTION_ATTACHMENT_SPAN: usize = 3;
const MARGIN_Y: usize = 0;
const RANK_H: usize = 3;
const MIN_NODE_WIDTH: usize = 11;
const MIN_TEXT_COLS: usize = 24;
const FOOTNOTE_WIDTH: usize = 56;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BorderKind {
    Solid,
    Double,
}

#[derive(Debug)]
pub enum Op {
    Box {
        x: usize,
        y: usize,
        w: usize,
        h: usize,
        border: BorderKind,
        border_style: Style,
        content: String,
        content_style: Style,
        ingresses: Vec<IngressPort>,
    },
    Stroke {
        cells: Vec<(usize, usize, u8)>,
        dashed: bool,
        style: Style,
    },
    Crossover {
        x: usize,
        y: usize,
    },
    Text {
        x: usize,
        y: usize,
        text: String,
        style: Style,
    },
}

#[derive(Debug, Clone, Copy)]
pub struct IngressPort {
    pub x: usize,
    pub style: Style,
}

#[derive(Debug)]
pub struct Scene {
    pub width: usize,
    /// Width of the graph before footnotes. Consumers use this value to decide
    /// whether the graph fits because footnote prose already wraps to viewport.
    pub graph_width: usize,
    pub height: usize,
    pub ops: Vec<Op>,
}

pub(super) struct NodeGeom {
    pub x: usize,
    pub w: usize,
    pub content: String,
    pub border: BorderKind,
    pub border_style: Style,
    pub content_style: Style,
}

impl NodeGeom {
    pub fn center(&self) -> usize {
        self.x + self.w / 2
    }

    #[cfg(test)]
    fn test_at(x: usize, w: usize) -> Self {
        Self {
            x,
            w,
            content: "test".into(),
            border: BorderKind::Solid,
            border_style: Style::Border,
            content_style: Style::Label,
        }
    }
}

pub fn compute(model: &Model, viewport: usize) -> Result<Scene, DiagramError> {
    let mut nodes = build_node_geometry(model);
    reserve_node_ports(model, &mut nodes);

    let placement = placement::place(model, &mut nodes, viewport)?;
    let channels = routing::route(model, &nodes, &placement.tracks, placement.width)?;
    let mut ingresses_by_node: HashMap<usize, Vec<IngressPort>> = HashMap::new();
    for channel in &channels {
        for ingress in &channel.ingresses {
            ingresses_by_node
                .entry(ingress.node)
                .or_default()
                .push(IngressPort {
                    x: ingress.x,
                    style: ingress.style,
                });
        }
    }

    let title_rows = usize::from(model.title.is_some()) * 2;
    let mut y = MARGIN_Y + title_rows;
    let mut rank_y = Vec::with_capacity(model.ranks.len());
    let mut channel_y = Vec::with_capacity(channels.len());
    for (rank, _) in model.ranks.iter().enumerate() {
        rank_y.push(y);
        y += RANK_H;
        if let Some(channel) = channels.get(rank) {
            channel_y.push(y);
            y += channel.height;
        }
    }

    let mut ops = Vec::new();
    if let Some(title) = &model.title {
        ops.push(Op::Text {
            x: MARGIN_X,
            y: MARGIN_Y,
            text: title.clone(),
            style: Style::Title,
        });
    }
    let graph_start = ops.len();

    for (rank, node_indices) in model.ranks.iter().enumerate() {
        for node in node_indices {
            let geom = &nodes[*node];
            ops.push(Op::Box {
                x: geom.x,
                y: rank_y[rank],
                w: geom.w,
                h: RANK_H,
                border: geom.border,
                border_style: geom.border_style,
                content: geom.content.clone(),
                content_style: geom.content_style,
                ingresses: ingresses_by_node.remove(node).unwrap_or_default(),
            });
        }
    }

    // A long relationship enters its track below the source, passes through
    // clear space in each intervening rank, and leaves above the target.
    for track in &placement.tracks {
        let edge = &model.edges[track.edge];
        let (dashed, style) = edge_style(edge.kind);
        for rank_top in rank_y
            .iter()
            .take(track.target_rank)
            .skip(track.source_rank + 1)
        {
            ops.push(Op::Stroke {
                cells: (*rank_top..*rank_top + RANK_H)
                    .map(|y| (track.x, y, N | S))
                    .collect(),
                dashed,
                style,
            });
        }
    }

    for (channel, top) in channels.into_iter().zip(channel_y) {
        ops.extend(channel.emit(top));
    }

    center_graph_ops(&mut ops[graph_start..], placement.width);

    let graph_width = placement.width;
    let mut width = graph_width;
    let mut height = y + 1;
    append_footnotes(model, viewport, &mut width, &mut height, &mut ops);

    Ok(Scene {
        width,
        graph_width,
        height,
        ops,
    })
}

fn center_graph_ops(ops: &mut [Op], width: usize) {
    let Some((left, right)) = ops
        .iter()
        .filter_map(op_x_bounds)
        .reduce(|(left, right), (op_left, op_right)| (left.min(op_left), right.max(op_right)))
    else {
        return;
    };

    let target_center = width / 2;
    let current_center = (left + right) / 2;
    let mut delta = target_center as isize - current_center as isize;
    delta = delta.max(MARGIN_X as isize - left as isize);
    delta = delta.min(width.saturating_sub(MARGIN_X + 1) as isize - right as isize);
    if delta == 0 {
        return;
    }
    for op in ops {
        shift_op_x(op, delta);
    }
}

fn op_x_bounds(op: &Op) -> Option<(usize, usize)> {
    match op {
        Op::Box { x, w, .. } => Some((*x, *x + *w - 1)),
        Op::Stroke { cells, .. } => cells
            .iter()
            .map(|(x, _, _)| *x)
            .min()
            .zip(cells.iter().map(|(x, _, _)| *x).max()),
        Op::Crossover { x, .. } => Some((*x, *x)),
        Op::Text { x, text, .. } => Some((*x, *x + text.chars().count().saturating_sub(1))),
    }
}

fn shift_op_x(op: &mut Op, delta: isize) {
    match op {
        Op::Box { x, ingresses, .. } => {
            *x = x.saturating_add_signed(delta);
            for ingress in ingresses {
                ingress.x = ingress.x.saturating_add_signed(delta);
            }
        }
        Op::Stroke { cells, .. } => {
            for (x, _, _) in cells {
                *x = x.saturating_add_signed(delta);
            }
        }
        Op::Crossover { x, .. } | Op::Text { x, .. } => {
            *x = x.saturating_add_signed(delta);
        }
    }
}

fn build_node_geometry(model: &Model) -> Vec<NodeGeom> {
    let mut markers_by_node: HashMap<usize, Vec<usize>> = HashMap::new();
    for (note, spec) in model.notes.iter().enumerate() {
        markers_by_node.entry(spec.on).or_default().push(note + 1);
    }

    model
        .nodes
        .iter()
        .enumerate()
        .map(|(index, node)| {
            let (mut content, border, border_style, content_style) = match node.kind {
                NodeKind::Service => (
                    node.label.clone(),
                    BorderKind::Solid,
                    Style::Border,
                    Style::Label,
                ),
                NodeKind::Store => (
                    node.label.clone(),
                    BorderKind::Double,
                    Style::BorderStore,
                    Style::Label,
                ),
                NodeKind::Queue => (
                    node.label.clone(),
                    BorderKind::Solid,
                    Style::BorderQueue,
                    Style::Label,
                ),
                NodeKind::External => (
                    node.label.clone(),
                    BorderKind::Solid,
                    Style::BorderExternal,
                    Style::LabelExternal,
                ),
                NodeKind::Decision => (
                    format!("< {} >", node.label),
                    BorderKind::Solid,
                    Style::Border,
                    Style::LabelDecision,
                ),
            };
            if let Some(markers) = markers_by_node.get(&index) {
                let markers = markers
                    .iter()
                    .map(usize::to_string)
                    .collect::<Vec<_>>()
                    .join(",");
                content.push_str(&format!(" [{markers}]"));
            }
            NodeGeom {
                x: 0,
                w: (content.chars().count() + 4).max(MIN_NODE_WIDTH),
                content,
                border,
                border_style,
                content_style,
            }
        })
        .collect()
}

pub(super) fn caption_width(text: &str) -> usize {
    let text_width = text.chars().count();
    let target_lines = match text_width {
        0..=24 => 1,
        25..=58 => 2,
        _ => 3,
    };
    let longest_word = text
        .split_whitespace()
        .map(|word| word.chars().count())
        .max()
        .unwrap_or(1);
    let minimum = MIN_CAPTION_WIDTH.max(longest_word).min(MAX_CAPTION_WIDTH);

    (minimum..=MAX_CAPTION_WIDTH)
        .find(|width| wrapped_line_count(text, *width) <= target_lines)
        .unwrap_or(MAX_CAPTION_WIDTH)
}

fn wrapped_line_count(text: &str, width: usize) -> usize {
    let mut lines = usize::from(!text.is_empty());
    let mut used = 0;
    for word in text.split_whitespace() {
        let word_width = word.chars().count();
        let needed = used + usize::from(used > 0) + word_width;
        if used > 0 && needed > width {
            lines += 1;
            used = word_width;
        } else {
            used = needed;
        }
    }
    lines.max(1)
}

fn reserve_node_ports(model: &Model, nodes: &mut [NodeGeom]) {
    let mut outgoing = vec![0usize; nodes.len()];
    let mut incoming = vec![0usize; nodes.len()];
    for edge in &model.edges {
        outgoing[edge.from] += 1;
        incoming[edge.to] += 1;
    }
    for (index, node) in nodes.iter_mut().enumerate() {
        let ports = outgoing[index].max(incoming[index]);
        if ports > 1 {
            node.w = node.w.max(2 * ports + 3);
        }
    }
}

fn edge_style(kind: EdgeKind) -> (bool, Style) {
    (
        matches!(kind, EdgeKind::Async | EdgeKind::Event),
        if kind == EdgeKind::Event {
            Style::EdgeLineEvent
        } else {
            Style::EdgeLine
        },
    )
}

fn append_footnotes(
    model: &Model,
    viewport: usize,
    width: &mut usize,
    height: &mut usize,
    ops: &mut Vec<Op>,
) {
    if model.notes.is_empty() {
        return;
    }

    let viewport = viewport.max(1);
    let block_width = FOOTNOTE_WIDTH.min(viewport);
    let mut y = *height + 1;
    for (index, note) in model.notes.iter().enumerate() {
        let node_label = &model.nodes[note.on].label;
        let (mark, body_style) = match note.mark {
            NoteMark::Uncertain => ("? ", Style::LabelUncertain),
            NoteMark::Info => ("", Style::LabelNote),
        };
        let marker = format!("[{}]", index + 1);
        let node_x = MARGIN_X + marker.chars().count() + 1;
        let lead = format!("— {mark}");
        let body_x = node_x + node_label.chars().count() + 1;
        let first_body_x = body_x + lead.chars().count();

        ops.push(Op::Text {
            x: MARGIN_X,
            y,
            text: marker,
            style: Style::NoteMarker,
        });
        ops.push(Op::Text {
            x: node_x,
            y,
            text: node_label.clone(),
            style: Style::Label,
        });

        let first_width = footnote_text_cols(block_width, viewport, first_body_x);
        let continuation_width = footnote_text_cols(block_width, viewport, node_x);
        for (line, chunk) in wrap_hanging(&note.text, first_width, continuation_width)
            .into_iter()
            .enumerate()
        {
            let (x, text) = if line == 0 {
                (body_x, format!("{lead}{chunk}"))
            } else {
                (node_x, chunk)
            };
            *width = (*width).max(x + text.chars().count() + MARGIN_X);
            ops.push(Op::Text {
                x,
                y,
                text,
                style: body_style,
            });
            y += 1;
        }
    }
    *height = y;
}

fn footnote_text_cols(block_width: usize, viewport: usize, start_x: usize) -> usize {
    let cap = viewport.saturating_sub(start_x + MARGIN_X).max(1);
    let desired = block_width.saturating_sub(start_x + MARGIN_X);
    desired.max(MIN_TEXT_COLS.min(cap)).min(cap)
}

fn wrap_hanging(text: &str, first: usize, rest: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    let mut current_width = 0;
    let available = |line: usize| {
        if line == 0 {
            first.max(1)
        } else {
            rest.max(1)
        }
    };

    for word in text.split_whitespace() {
        let word_width = word.chars().count();
        let line_width = available(lines.len());
        if word_width > line_width {
            if !current.is_empty() {
                lines.push(std::mem::take(&mut current));
                current_width = 0;
            }
            let chars: Vec<char> = word.chars().collect();
            let mut start = 0;
            while start < chars.len() {
                let line_width = available(lines.len());
                let end = (start + line_width).min(chars.len());
                lines.push(chars[start..end].iter().collect());
                start = end;
            }
            continue;
        }

        let needed = if current.is_empty() {
            word_width
        } else {
            current_width + 1 + word_width
        };
        if needed > line_width && !current.is_empty() {
            lines.push(std::mem::take(&mut current));
            current.push_str(word);
            current_width = word_width;
        } else {
            if !current.is_empty() {
                current.push(' ');
                current_width += 1;
            }
            current.push_str(word);
            current_width += word_width;
        }
    }

    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caption_measure_uses_the_texts_natural_wrapped_width() {
        let short = caption_width("role assignments");
        let medium = caption_width("account and authentication requests");
        let long = caption_width(
            "monitors computers, publishes authentication, views desktops, and takes control",
        );

        assert_eq!(wrapped_line_count("role assignments", short), 1);
        assert!(wrapped_line_count("account and authentication requests", medium) <= 2);
        assert!(
            wrapped_line_count(
                "monitors computers, publishes authentication, views desktops, and takes control",
                long,
            ) <= 3
        );
        assert!(short < medium);
        assert!(medium <= long);
    }

    #[test]
    fn final_composition_centers_boxes_and_their_ingresses_together() {
        let mut ops = vec![Op::Box {
            x: 10,
            y: 0,
            w: 11,
            h: RANK_H,
            border: BorderKind::Solid,
            border_style: Style::Border,
            content: "Node".into(),
            content_style: Style::Label,
            ingresses: vec![IngressPort {
                x: 15,
                style: Style::Ingress,
            }],
        }];

        center_graph_ops(&mut ops, 100);

        let Op::Box { x, ingresses, .. } = &ops[0] else {
            panic!("test scene contains one box");
        };
        assert_eq!(*x, 45);
        assert_eq!(ingresses[0].x, 50);
    }
}
