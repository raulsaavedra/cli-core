//! Local orthogonal routing between adjacent node ranks.
//!
//! Every channel has source-side route lanes, a vertical branch field, and
//! target-side route lanes. Each relationship keeps distinct geometry from its
//! source port through its caption branch and into its target-box ingress.

use std::collections::{HashMap, HashSet};

use super::placement::Track;
use super::{
    caption_width, NodeGeom, Op, CAPTION_ATTACHMENT_SPAN, MARGIN_X, MAX_CAPTION_WIDTH,
    MIN_CAPTION_WIDTH,
};
use crate::diagram::doc::{DiagramError, EdgeKind, Model};
use crate::diagram::grid::{Style, E, N, S, W};

const UNLABELED_TRACK_GAP: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum End {
    Node(usize),
    Track(usize),
}

struct Segment {
    edge: usize,
    from: End,
    to: End,
    sx: usize,
    tx: usize,
    anchor: usize,
    label: Option<String>,
    kind: EdgeKind,
}

impl Segment {
    fn dashed(&self) -> bool {
        matches!(self.kind, EdgeKind::Async | EdgeKind::Event)
    }

    fn style(&self) -> Style {
        if self.kind == EdgeKind::Event {
            Style::EdgeLineEvent
        } else {
            Style::EdgeLine
        }
    }

    fn branch_style(&self) -> Style {
        if self.kind == EdgeKind::Event {
            Style::EdgeLineEvent
        } else {
            Style::EdgeBranch
        }
    }

    fn label_style(&self) -> Style {
        if self.kind == EdgeKind::Event {
            Style::EdgeLabelEvent
        } else {
            Style::EdgeLabel
        }
    }

    fn ingress_style(&self) -> Style {
        if self.kind == EdgeKind::Event {
            Style::IngressEvent
        } else {
            Style::Ingress
        }
    }
}

struct LocalStroke {
    cells: Vec<(usize, usize, u8)>,
    dashed: bool,
    style: Style,
    edge: usize,
}

struct LocalLabel {
    x: usize,
    y: usize,
    text: String,
    style: Style,
}

pub(super) struct ChannelPlan {
    pub height: usize,
    pub ingresses: Vec<Ingress>,
    strokes: Vec<LocalStroke>,
    crossovers: Vec<(usize, usize)>,
    labels: Vec<LocalLabel>,
}

pub(super) struct Ingress {
    pub node: usize,
    pub x: usize,
    pub style: Style,
}

impl ChannelPlan {
    pub fn emit(self, top: usize) -> Vec<Op> {
        let mut ops = Vec::new();
        for stroke in self.strokes {
            ops.push(Op::Stroke {
                cells: stroke
                    .cells
                    .into_iter()
                    .map(|(x, y, mask)| (x, y + top, mask))
                    .collect(),
                dashed: stroke.dashed,
                style: stroke.style,
            });
        }
        for (x, y) in self.crossovers {
            ops.push(Op::Crossover { x, y: y + top });
        }
        for label in self.labels {
            ops.push(Op::Text {
                x: label.x,
                y: label.y + top,
                text: label.text,
                style: label.style,
            });
        }
        ops
    }
}

pub(super) fn route(
    model: &Model,
    nodes: &[NodeGeom],
    tracks: &[Track],
    width: usize,
) -> Result<Vec<ChannelPlan>, DiagramError> {
    let channel_count = model.ranks.len().saturating_sub(1);
    let mut channels: Vec<Vec<Segment>> = (0..channel_count).map(|_| Vec::new()).collect();
    let track_by_edge: HashMap<usize, usize> = tracks
        .iter()
        .enumerate()
        .map(|(index, track)| (track.edge, index))
        .collect();

    for (edge_index, edge) in model.edges.iter().enumerate() {
        let source_rank = model.nodes[edge.from].rank;
        let target_rank = model.nodes[edge.to].rank;
        let track = track_by_edge.get(&edge_index).copied();
        for (channel, segments) in channels
            .iter_mut()
            .enumerate()
            .take(target_rank)
            .skip(source_rank)
        {
            let from = if channel == source_rank {
                End::Node(edge.from)
            } else {
                End::Track(track.expect("long edge has a rank corridor"))
            };
            let to = if channel + 1 == target_rank {
                End::Node(edge.to)
            } else {
                End::Track(track.expect("long edge has a rank corridor"))
            };
            segments.push(Segment {
                edge: edge_index,
                from,
                to,
                sx: 0,
                tx: 0,
                anchor: 0,
                label: if channel + 1 == target_rank {
                    edge.label.clone()
                } else {
                    None
                },
                kind: edge.kind,
            });
        }
    }

    channels
        .into_iter()
        .map(|mut segments| {
            assign_ports(&mut segments, nodes, tracks);
            assign_anchors(&mut segments, width)?;
            align_node_ports_with_branches(&mut segments, nodes);
            plan_channel(segments, width)
        })
        .collect()
}

fn endpoint_x(end: End, nodes: &[NodeGeom], tracks: &[Track]) -> usize {
    match end {
        End::Node(node) => nodes[node].center(),
        End::Track(track) => tracks[track].x,
    }
}

fn assign_ports(segments: &mut [Segment], nodes: &[NodeGeom], tracks: &[Track]) {
    let mut by_source: HashMap<usize, Vec<usize>> = HashMap::new();
    let mut by_target: HashMap<usize, Vec<usize>> = HashMap::new();
    for (index, segment) in segments.iter().enumerate() {
        if let End::Node(node) = segment.from {
            by_source.entry(node).or_default().push(index);
        }
        if let End::Node(node) = segment.to {
            by_target.entry(node).or_default().push(index);
        }
    }

    for segment in segments.iter_mut() {
        if let End::Track(track) = segment.from {
            segment.sx = tracks[track].x;
        }
        if let End::Track(track) = segment.to {
            segment.tx = tracks[track].x;
        }
    }

    for (node, mut indices) in by_source {
        indices.sort_by_key(|index| {
            (
                endpoint_x(segments[*index].to, nodes, tracks),
                segments[*index].edge,
            )
        });
        for (slot, index) in indices.iter().enumerate() {
            segments[*index].sx = spread_port(&nodes[node], indices.len(), slot);
        }
    }

    for (node, mut indices) in by_target {
        indices.sort_by_key(|index| {
            (
                endpoint_x(segments[*index].from, nodes, tracks),
                segments[*index].edge,
            )
        });
        for (slot, index) in indices.iter().enumerate() {
            segments[*index].tx = spread_port(&nodes[node], indices.len(), slot);
        }
    }
}

fn spread_port(node: &NodeGeom, count: usize, slot: usize) -> usize {
    let interior = node.w - 2;
    node.x + 1 + (interior * (slot + 1)) / (count + 1)
}

fn align_node_ports_with_branches(segments: &mut [Segment], nodes: &[NodeGeom]) {
    let mut by_source: HashMap<usize, Vec<usize>> = HashMap::new();
    let mut by_target: HashMap<usize, Vec<usize>> = HashMap::new();
    for (index, segment) in segments.iter().enumerate() {
        if let End::Node(node) = segment.from {
            by_source.entry(node).or_default().push(index);
        }
        if let End::Node(node) = segment.to {
            by_target.entry(node).or_default().push(index);
        }
    }

    for (node, mut indices) in by_source {
        indices.sort_by_key(|index| (segments[*index].anchor, segments[*index].edge));
        for (slot, index) in indices.iter().enumerate() {
            segments[*index].sx = spread_port(&nodes[node], indices.len(), slot);
        }
    }
    for (node, mut indices) in by_target {
        indices.sort_by_key(|index| (segments[*index].anchor, segments[*index].edge));
        for (slot, index) in indices.iter().enumerate() {
            segments[*index].tx = spread_port(&nodes[node], indices.len(), slot);
        }
    }
}

fn assign_anchors(segments: &mut [Segment], width: usize) -> Result<(), DiagramError> {
    if segments.is_empty() {
        return Ok(());
    }

    let preferred: Vec<usize> = segments.iter().map(|segment| segment.tx).collect();
    let mut order: Vec<usize> = (0..segments.len()).collect();
    order.sort_by_key(|index| {
        (
            preferred[*index],
            segments[*index].sx,
            segments[*index].tx,
            segments[*index].edge,
        )
    });

    let left = MARGIN_X + 1;
    let right = width.saturating_sub(MARGIN_X + 2);
    let mut separations = Vec::new();
    for pair in order.windows(2) {
        separations.push(
            if segments[pair[0]].label.is_some() || segments[pair[1]].label.is_some() {
                segments[pair[0]]
                    .label
                    .as_deref()
                    .map(caption_width)
                    .into_iter()
                    .chain(segments[pair[1]].label.as_deref().map(caption_width))
                    .max()
                    .unwrap_or(MIN_CAPTION_WIDTH)
                    + CAPTION_ATTACHMENT_SPAN
                    + 1
            } else {
                UNLABELED_TRACK_GAP
            },
        );
    }

    // Track-to-track segments cross an intermediate rank and keep the physical
    // column selected by placement. Neighboring branches move around that fixed
    // column while preserving their topology order.
    let fixed: Vec<Option<usize>> = order
        .iter()
        .map(|index| match (segments[*index].from, segments[*index].to) {
            (End::Track(from), End::Track(to)) if from == to => Some(segments[*index].sx),
            _ => None,
        })
        .collect();

    let mut lower = vec![left; order.len()];
    for position in 0..order.len() {
        let minimum = if position == 0 {
            left
        } else {
            lower[position - 1] + separations[position - 1]
        };
        lower[position] = match fixed[position] {
            Some(column) if column < minimum => {
                return Err(DiagramError::Routing(
                    "fixed relationship tracks need a wider branch field".into(),
                ));
            }
            Some(column) => column,
            None => minimum,
        };
    }

    let mut upper = vec![right; order.len()];
    for position in (0..order.len()).rev() {
        let maximum = if position + 1 == order.len() {
            right
        } else {
            upper[position + 1]
                .checked_sub(separations[position])
                .ok_or_else(|| {
                    DiagramError::Routing("relationship captions need a wider branch field".into())
                })?
        };
        upper[position] = match fixed[position] {
            Some(column) if column > maximum => {
                return Err(DiagramError::Routing(
                    "fixed relationship tracks need a wider branch field".into(),
                ));
            }
            Some(column) => column,
            None => maximum,
        };
        if lower[position] > upper[position] {
            return Err(DiagramError::Routing(
                "relationship captions need a wider branch field".into(),
            ));
        }
    }

    let mut positions = vec![0usize; order.len()];
    for position in 0..order.len() {
        let minimum = if position == 0 {
            lower[position]
        } else {
            (positions[position - 1] + separations[position - 1]).max(lower[position])
        };
        positions[position] = fixed[position]
            .unwrap_or_else(|| preferred[order[position]].clamp(minimum, upper[position]));
    }
    for (position, segment) in order.into_iter().enumerate() {
        segments[segment].anchor = positions[position];
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct RouteInterval {
    segment: usize,
    lo: usize,
    hi: usize,
    edge: usize,
}

fn assign_bus_lanes(segments: &[Segment], source_side: bool) -> (HashMap<usize, usize>, usize) {
    let mut routes = Vec::new();
    for (index, segment) in segments.iter().enumerate() {
        let endpoint = if source_side { segment.sx } else { segment.tx };
        let lo = segment.anchor.min(endpoint);
        let hi = segment.anchor.max(endpoint);
        if lo != hi {
            routes.push(RouteInterval {
                segment: index,
                lo,
                hi,
                edge: segment.edge,
            });
        }
    }
    // Farther fan-out routes turn first so they pass outside nearer branches.
    // Fan-in reverses that order: nearer routes land first and leave lower
    // lanes clear for branches arriving from farther away.
    if source_side {
        routes.sort_by_key(|route| (std::cmp::Reverse(route.hi - route.lo), route.edge));
    } else {
        routes.sort_by_key(|route| (route.hi - route.lo, route.edge));
    }

    let mut lanes: Vec<Vec<RouteInterval>> = Vec::new();
    let mut assigned = HashMap::new();
    for route in routes {
        let lane = (0..=lanes.len())
            .find(|lane| {
                lanes.get(*lane).is_none_or(|occupants| {
                    occupants
                        .iter()
                        .all(|other| route.hi + 1 < other.lo || other.hi + 1 < route.lo)
                })
            })
            .expect("a new route lane is always available");
        if lane == lanes.len() {
            lanes.push(Vec::new());
        }
        lanes[lane].push(route);
        assigned.insert(route.segment, lane);
    }
    (assigned, lanes.len())
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CaptionSide {
    Left,
    Right,
}

struct Caption {
    segment: usize,
    side: CaptionSide,
    lines: Vec<String>,
    row: usize,
    x: usize,
    width: usize,
    lo: usize,
    hi: usize,
}

fn plan_captions(
    segments: &[Segment],
    width: usize,
) -> Result<(Vec<Caption>, usize), DiagramError> {
    let mut anchors: Vec<usize> = segments.iter().map(|segment| segment.anchor).collect();
    anchors.sort_unstable();
    anchors.dedup();

    let mut labeled_segments = segments
        .iter()
        .enumerate()
        .filter_map(|(index, segment)| {
            segment
                .label
                .as_deref()
                .map(|label| (index, segment, label))
        })
        .collect::<Vec<_>>();
    labeled_segments.sort_by_key(|(_, segment, _)| (segment.anchor, segment.edge));

    let mut captions = Vec::new();
    let mut occupancy: Vec<Vec<(usize, usize)>> = Vec::new();

    for (segment_index, segment, label) in labeled_segments {
        let position = anchors
            .binary_search(&segment.anchor)
            .expect("segment anchor is indexed");
        let left_boundary = if position == 0 {
            MARGIN_X
        } else {
            anchors[position - 1] + CAPTION_ATTACHMENT_SPAN + 1
        };
        let right_boundary = if position + 1 == anchors.len() {
            width.saturating_sub(MARGIN_X + 1)
        } else {
            anchors[position + 1].saturating_sub(CAPTION_ATTACHMENT_SPAN + 1)
        };
        let left_available = segment
            .anchor
            .saturating_sub(left_boundary + CAPTION_ATTACHMENT_SPAN);
        let right_available =
            right_boundary.saturating_sub(segment.anchor + CAPTION_ATTACHMENT_SPAN);
        let outward_side = if segment.anchor < width / 2 {
            CaptionSide::Left
        } else {
            CaptionSide::Right
        };

        let mut candidates = [
            (CaptionSide::Left, left_available),
            (CaptionSide::Right, right_available),
        ]
        .into_iter()
        .filter(|(_, available)| *available >= MIN_CAPTION_WIDTH)
        .map(|(side, available)| {
            let lines = wrap_words(label, available.min(MAX_CAPTION_WIDTH));
            let block_width = lines
                .iter()
                .map(|line| line.chars().count())
                .max()
                .unwrap_or(0);
            let x = match side {
                CaptionSide::Left => segment.anchor - CAPTION_ATTACHMENT_SPAN - block_width,
                CaptionSide::Right => segment.anchor + CAPTION_ATTACHMENT_SPAN + 1,
            };
            let mut caption = Caption {
                segment: segment_index,
                side,
                lines,
                row: 0,
                x,
                width: block_width,
                lo: x,
                hi: x + block_width - 1,
            };
            caption.row = first_open_caption_row(&caption, &occupancy);
            let resulting_height = occupancy.len().max(caption.row + caption.lines.len());
            let width_penalty = MAX_CAPTION_WIDTH - available.min(MAX_CAPTION_WIDTH);
            let side_penalty = usize::from(side != outward_side);
            (
                (
                    resulting_height,
                    caption.lines.len(),
                    width_penalty,
                    side_penalty,
                ),
                caption,
            )
        })
        .collect::<Vec<_>>();
        // Prefer the side that adds the least channel height, then preserve
        // readable line lengths. Outward placement breaks an otherwise equal tie.
        candidates.sort_by_key(|(score, _)| *score);
        let Some((_, caption)) = candidates.into_iter().next() else {
            return Err(DiagramError::Routing(format!(
                "edge label `{label}` needs more space beside its branch"
            )));
        };

        while occupancy.len() < caption.row + caption.lines.len() {
            occupancy.push(Vec::new());
        }
        for line in occupancy
            .iter_mut()
            .skip(caption.row)
            .take(caption.lines.len())
        {
            line.push((caption.lo, caption.hi));
        }
        captions.push(caption);
    }

    Ok((captions, occupancy.len().max(1)))
}

fn first_open_caption_row(caption: &Caption, occupancy: &[Vec<(usize, usize)>]) -> usize {
    let mut row = 0;
    loop {
        let fits = (row..row + caption.lines.len()).all(|line| {
            occupancy.get(line).is_none_or(|intervals| {
                intervals
                    .iter()
                    .all(|(lo, hi)| caption.hi + 1 < *lo || *hi + 1 < caption.lo)
            })
        });
        if fits {
            return row;
        }
        row += 1;
    }
}

fn plan_channel(segments: Vec<Segment>, width: usize) -> Result<ChannelPlan, DiagramError> {
    if segments.is_empty() {
        return Ok(ChannelPlan {
            height: 2,
            ingresses: Vec::new(),
            strokes: Vec::new(),
            crossovers: Vec::new(),
            labels: Vec::new(),
        });
    }

    let (source_lanes, source_lane_count) = assign_bus_lanes(&segments, true);
    let (target_lanes, target_lane_count) = assign_bus_lanes(&segments, false);
    let (captions, caption_height) = plan_captions(&segments, width)?;
    const SOURCE_STEM_ROWS: usize = 1;
    const BAND_GAP_ROWS: usize = 1;
    const TARGET_STEM_ROWS: usize = 0;

    let caption_start = SOURCE_STEM_ROWS + source_lane_count + BAND_GAP_ROWS;
    let target_start = caption_start + caption_height + 1;
    let final_row = target_start + target_lane_count + TARGET_STEM_ROWS;
    let height = final_row + 1;

    let mut strokes = Vec::new();
    for (segment_index, segment) in segments.iter().enumerate() {
        let source_row = source_lanes
            .get(&segment_index)
            .map(|lane| SOURCE_STEM_ROWS + lane);
        let target_row = target_lanes
            .get(&segment_index)
            .map(|lane| target_start + lane);
        let target_is_track = matches!(segment.to, End::Track(_));
        let vertical_end = final_row;

        let mut source_points = vec![(segment.sx, 0)];
        if let Some(row) = source_row {
            push_point(&mut source_points, (segment.sx, row));
            push_point(&mut source_points, (segment.anchor, row));
        } else {
            debug_assert_eq!(segment.sx, segment.anchor);
        }
        if target_is_track {
            if let Some(row) = target_row {
                push_point(&mut source_points, (segment.anchor, row));
                push_point(&mut source_points, (segment.tx, row));
            } else {
                debug_assert_eq!(segment.anchor, segment.tx);
            }
            push_point(&mut source_points, (segment.tx, vertical_end));
            strokes.push(LocalStroke {
                cells: trace_polyline(&source_points),
                dashed: segment.dashed(),
                style: segment.style(),
                edge: segment.edge,
            });
        } else {
            push_point(&mut source_points, (segment.anchor, caption_start));
            strokes.push(LocalStroke {
                cells: trace_polyline(&source_points),
                dashed: segment.dashed(),
                style: segment.style(),
                edge: segment.edge,
            });

            let mut target_points = vec![(segment.anchor, caption_start)];
            if let Some(row) = target_row {
                push_point(&mut target_points, (segment.anchor, row));
                push_point(&mut target_points, (segment.tx, row));
            } else {
                debug_assert_eq!(segment.anchor, segment.tx);
            }
            push_point(&mut target_points, (segment.tx, vertical_end));
            strokes.push(LocalStroke {
                cells: trace_polyline(&target_points),
                dashed: segment.dashed(),
                style: segment.branch_style(),
                edge: segment.edge,
            });
        }
    }

    for caption in &captions {
        let segment = &segments[caption.segment];
        let y = caption_start + caption.row + caption.lines.len() / 2;
        let cells = match caption.side {
            CaptionSide::Left => trace_polyline(&[(segment.anchor - 2, y), (segment.anchor, y)]),
            CaptionSide::Right => trace_polyline(&[(segment.anchor, y), (segment.anchor + 2, y)]),
        };
        strokes.push(LocalStroke {
            cells,
            dashed: segment.dashed(),
            style: segment.branch_style(),
            edge: segment.edge,
        });
    }

    let mut labels = Vec::new();
    for caption in captions {
        let y = caption_start + caption.row;
        for (line, text) in caption.lines.into_iter().enumerate() {
            let text_width = text.chars().count();
            let x = match caption.side {
                CaptionSide::Left => caption.x + caption.width - text_width,
                CaptionSide::Right => caption.x,
            };
            labels.push(LocalLabel {
                x,
                y: y + line,
                text,
                style: segments[caption.segment].label_style(),
            });
        }
    }

    let crossovers = find_crossovers(&strokes)?;
    let mut ingresses = Vec::new();
    for segment in &segments {
        if let End::Node(node) = segment.to {
            ingresses.push(Ingress {
                node,
                x: segment.tx,
                style: segment.ingress_style(),
            });
        }
    }

    Ok(ChannelPlan {
        height,
        ingresses,
        strokes,
        crossovers,
        labels,
    })
}

fn push_point(points: &mut Vec<(usize, usize)>, point: (usize, usize)) {
    if points.last() != Some(&point) {
        points.push(point);
    }
}

fn trace_polyline(points: &[(usize, usize)]) -> Vec<(usize, usize, u8)> {
    let mut masks: HashMap<(usize, usize), u8> = HashMap::new();
    for pair in points.windows(2) {
        let (from_x, from_y) = pair[0];
        let (to_x, to_y) = pair[1];
        if from_x == to_x {
            let (lo, hi) = (from_y.min(to_y), from_y.max(to_y));
            for y in lo..hi {
                *masks.entry((from_x, y)).or_default() |= S;
                *masks.entry((from_x, y + 1)).or_default() |= N;
            }
        } else {
            debug_assert_eq!(from_y, to_y);
            let (lo, hi) = (from_x.min(to_x), from_x.max(to_x));
            for x in lo..hi {
                *masks.entry((x, from_y)).or_default() |= E;
                *masks.entry((x + 1, from_y)).or_default() |= W;
            }
        }
    }
    let mut cells: Vec<_> = masks
        .into_iter()
        .map(|((x, y), mask)| (x, y, mask))
        .collect();
    cells.sort_unstable_by_key(|(x, y, _)| (*y, *x));
    cells
}

fn find_crossovers(strokes: &[LocalStroke]) -> Result<Vec<(usize, usize)>, DiagramError> {
    let mut occupancy: HashMap<(usize, usize), Vec<(usize, u8)>> = HashMap::new();
    for (route, stroke) in strokes.iter().enumerate() {
        for (x, y, mask) in &stroke.cells {
            occupancy.entry((*x, *y)).or_default().push((route, *mask));
        }
    }

    let mut crossovers = HashSet::new();
    for ((x, y), occupants) in occupancy {
        for a in 0..occupants.len() {
            for b in a + 1..occupants.len() {
                let (a_route, a_mask) = occupants[a];
                let (b_route, b_mask) = occupants[b];
                if related(&strokes[a_route], &strokes[b_route]) {
                    continue;
                }
                let a_horizontal = a_mask & (E | W) != 0;
                let a_vertical = a_mask & (N | S) != 0;
                let b_horizontal = b_mask & (E | W) != 0;
                let b_vertical = b_mask & (N | S) != 0;
                if (a_horizontal && b_vertical) || (b_horizontal && a_vertical) {
                    crossovers.insert((x, y));
                } else if (a_horizontal && b_horizontal) || (a_vertical && b_vertical) {
                    return Err(DiagramError::Routing(format!(
                        "independent relationship branches overlap at ({x}, {y})"
                    )));
                }
            }
        }
    }

    let mut crossovers: Vec<_> = crossovers.into_iter().collect();
    crossovers.sort_unstable_by_key(|(x, y)| (*y, *x));
    Ok(crossovers)
}

fn related(a: &LocalStroke, b: &LocalStroke) -> bool {
    a.edge == b.edge
}

fn wrap_words(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        let needed =
            current.chars().count() + usize::from(!current.is_empty()) + word.chars().count();
        if needed > width && !current.is_empty() {
            lines.push(std::mem::take(&mut current));
        }
        if word.chars().count() > width {
            if !current.is_empty() {
                lines.push(std::mem::take(&mut current));
            }
            let chars: Vec<char> = word.chars().collect();
            for chunk in chars.chunks(width.max(1)) {
                lines.push(chunk.iter().collect());
            }
            continue;
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
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
    fn pass_through_track_keeps_its_placement_column() {
        let mut segments = vec![
            Segment {
                edge: 0,
                from: End::Track(0),
                to: End::Track(0),
                sx: 40,
                tx: 40,
                anchor: 0,
                label: None,
                kind: EdgeKind::Sync,
            },
            Segment {
                edge: 1,
                from: End::Node(0),
                to: End::Node(1),
                sx: 20,
                tx: 60,
                anchor: 0,
                label: Some("branch caption".into()),
                kind: EdgeKind::Sync,
            },
        ];

        assign_anchors(&mut segments, 120).expect("tracks should fit");

        assert_eq!(segments[0].anchor, 40);
    }

    #[test]
    fn caption_leader_attaches_to_a_continuous_branch() {
        let anchor = 40;
        let plan = plan_channel(
            vec![Segment {
                edge: 0,
                from: End::Node(0),
                to: End::Node(1),
                sx: anchor,
                tx: anchor,
                anchor,
                label: Some("branch caption".into()),
                kind: EdgeKind::Sync,
            }],
            80,
        )
        .expect("a side caption should fit beside a straight branch");

        let cells = plan
            .strokes
            .iter()
            .flat_map(|stroke| stroke.cells.iter())
            .collect::<Vec<_>>();

        for label in &plan.labels {
            let label_end = label.x + label.text.chars().count();
            assert!(label_end <= anchor || label.x > anchor);
            assert!(
                anchor.saturating_sub(label_end) == CAPTION_ATTACHMENT_SPAN
                    || label.x.saturating_sub(anchor + 1) == CAPTION_ATTACHMENT_SPAN
            );
            assert!(cells
                .iter()
                .any(|(x, y, mask)| { *x == anchor && *y == label.y && *mask & (N | S) != 0 }));
        }
        let attachment_row = plan.labels[plan.labels.len() / 2].y;
        assert!(cells
            .iter()
            .any(|(x, y, mask)| { *x == anchor && *y == attachment_row && *mask & (E | W) != 0 }));
    }

    #[test]
    fn caption_uses_the_side_with_available_territory() {
        let plan_at = |anchor| {
            plan_channel(
                vec![Segment {
                    edge: 0,
                    from: End::Node(0),
                    to: End::Node(1),
                    sx: anchor,
                    tx: anchor,
                    anchor,
                    label: Some("a multiline branch caption keeps every line attached".into()),
                    kind: EdgeKind::Sync,
                }],
                80,
            )
            .expect("the caption should use the open side of the branch")
        };

        let left_edge = plan_at(8);
        assert!(left_edge.labels.len() > 1);
        assert!(left_edge
            .labels
            .iter()
            .all(|label| label.x == 8 + CAPTION_ATTACHMENT_SPAN + 1));

        let right_edge = plan_at(72);
        assert!(right_edge.labels.len() > 1);
        assert!(right_edge
            .labels
            .iter()
            .all(|label| { label.x + label.text.chars().count() == 72 - CAPTION_ATTACHMENT_SPAN }));
    }

    #[test]
    fn route_lanes_order_fan_out_far_first_and_fan_in_near_first() {
        let segments = vec![
            Segment {
                edge: 0,
                from: End::Node(0),
                to: End::Node(1),
                sx: 40,
                tx: 50,
                anchor: 60,
                label: None,
                kind: EdgeKind::Sync,
            },
            Segment {
                edge: 1,
                from: End::Node(0),
                to: End::Node(2),
                sx: 42,
                tx: 40,
                anchor: 90,
                label: None,
                kind: EdgeKind::Sync,
            },
        ];

        let (fan_out, _) = assign_bus_lanes(&segments, true);
        let (fan_in, _) = assign_bus_lanes(&segments, false);

        assert_eq!(fan_out[&1], 0);
        assert_eq!(fan_out[&0], 1);
        assert_eq!(fan_in[&0], 0);
        assert_eq!(fan_in[&1], 1);
    }
}
