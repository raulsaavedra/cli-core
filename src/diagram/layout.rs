//! Top-down architecture-diagram layout.
//!
//! Authored ranks establish vertical stages. Horizontal placement follows the
//! graph's branch structure, and each channel renders local split buses,
//! relationship branches, and merge buses. A label occupies the final branch
//! entering its target; the branch ends above the caption and resumes below it.

mod placement;
mod routing;

use std::collections::{HashMap, HashSet};

use super::doc::{DiagramError, EdgeKind, Model, NodeKind, NoteMark};
use super::grid::{Style, N, S};

pub(super) const MARGIN_X: usize = 1;
pub(super) const MIN_CAPTION_WIDTH: usize = 12;
pub(super) const CAPTION_TRACK_WIDTH: usize = 32;
const MARGIN_Y: usize = 0;
const RANK_H: usize = 3;
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
    },
    Stroke {
        cells: Vec<(usize, usize, u8)>,
        dashed: bool,
        style: Style,
    },
    Arrow {
        x: usize,
        y: usize,
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
}

pub fn compute(model: &Model, viewport: usize) -> Result<Scene, DiagramError> {
    let mut nodes = build_node_geometry(model);
    reserve_node_ports(model, &mut nodes);

    let placement = placement::place(model, &mut nodes, viewport)?;
    let channels = routing::route(model, &nodes, &placement.tracks, placement.width)?;

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
                w: content.chars().count() + 4,
                content,
                border,
                border_style,
                content_style,
            }
        })
        .collect()
}

fn reserve_node_ports(model: &Model, nodes: &mut [NodeGeom]) {
    let mut outgoing: Vec<HashSet<EdgeKind>> = vec![HashSet::new(); nodes.len()];
    let mut incoming: Vec<HashSet<EdgeKind>> = vec![HashSet::new(); nodes.len()];
    for edge in &model.edges {
        outgoing[edge.from].insert(edge.kind);
        incoming[edge.to].insert(edge.kind);
    }
    for (index, node) in nodes.iter_mut().enumerate() {
        let ports = outgoing[index].len().max(incoming[index].len());
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
