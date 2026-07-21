//! Local orthogonal routing between adjacent node ranks.
//!
//! Every channel has source-side route lanes, a vertical branch field, and
//! target-side route lanes. Each relationship keeps distinct geometry from its
//! source port through its caption branch and into its target-box ingress.

use std::collections::{HashMap, HashSet};

use super::placement::VirtualGeom;
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
    Virtual(usize),
}

#[derive(Clone)]
struct Segment {
    edge: usize,
    from: End,
    to: End,
    sx: usize,
    tx: usize,
    anchor: usize,
    label: Option<String>,
    accented: bool,
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

    fn route_style(&self) -> Style {
        if self.accented {
            self.branch_style()
        } else {
            self.style()
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
    virtuals: &mut [VirtualGeom],
    width: usize,
) -> Result<Vec<ChannelPlan>, DiagramError> {
    let channel_count = model.ranks.len().saturating_sub(1);
    let mut channels: Vec<Vec<Segment>> = (0..channel_count).map(|_| Vec::new()).collect();
    let virtual_by_edge_rank: HashMap<(usize, usize), usize> = virtuals
        .iter()
        .enumerate()
        .map(|(index, virtual_node)| ((virtual_node.edge, virtual_node.rank), index))
        .collect();

    for (edge_index, edge) in model.edges.iter().enumerate() {
        let source_rank = model.nodes[edge.from].rank;
        let target_rank = model.nodes[edge.to].rank;
        for (channel, segments) in channels
            .iter_mut()
            .enumerate()
            .take(target_rank)
            .skip(source_rank)
        {
            let from = if channel == source_rank {
                End::Node(edge.from)
            } else {
                End::Virtual(virtual_by_edge_rank[&(edge_index, channel)])
            };
            let to = if channel + 1 == target_rank {
                End::Node(edge.to)
            } else {
                End::Virtual(virtual_by_edge_rank[&(edge_index, channel + 1)])
            };
            segments.push(Segment {
                edge: edge_index,
                from,
                to,
                sx: 0,
                tx: 0,
                anchor: 0,
                label: None,
                accented: false,
                kind: edge.kind,
            });
        }
    }

    for segments in &mut channels {
        assign_ports(segments, nodes, virtuals);
        assign_anchors(segments, width)?;
        align_node_ports_with_branches(segments, nodes);
    }
    place_relationship_captions(model, &mut channels, nodes, virtuals, width)?;
    for segment in channels.iter().flatten() {
        if segment.label.is_some() || segment.accented {
            if let End::Virtual(virtual_node) = segment.to {
                virtuals[virtual_node].accented = true;
            }
        }
    }
    for segments in &mut channels {
        assign_ports(segments, nodes, virtuals);
        assign_anchors(segments, width)?;
        align_node_ports_with_branches(segments, nodes);
    }

    channels
        .into_iter()
        .map(|segments| plan_channel(segments, width))
        .collect()
}

fn place_relationship_captions(
    model: &Model,
    channels: &mut [Vec<Segment>],
    nodes: &[NodeGeom],
    virtuals: &[VirtualGeom],
    width: usize,
) -> Result<(), DiagramError> {
    // A caption belongs to the complete relationship. Every segment competes
    // to host it using the geometry that the full routed graph would produce.
    for (edge_index, edge) in model.edges.iter().enumerate() {
        let Some(label) = edge.label.as_deref() else {
            continue;
        };
        let required = caption_width(label);
        let source_rank = model.nodes[edge.from].rank;
        let mut candidates = Vec::new();
        for (channel, segments) in channels.iter().enumerate() {
            let Some((segment_index, segment)) = segments
                .iter()
                .enumerate()
                .find(|(_, segment)| segment.edge == edge_index)
            else {
                continue;
            };
            let mut anchors = segments
                .iter()
                .map(|segment| segment.anchor)
                .collect::<Vec<_>>();
            anchors.sort_unstable();
            anchors.dedup();
            let (left, right) = caption_availability(segment.anchor, &anchors, width);
            let available = left.max(right).min(MAX_CAPTION_WIDTH);
            let deficit = required.saturating_sub(available);
            let lines = wrap_words(label, available.max(1)).len();
            let crossings = caption_candidate_crossings(
                channels,
                channel,
                segment_index,
                label,
                nodes,
                virtuals,
                width,
            );
            candidates.push((
                (
                    crossings,
                    deficit,
                    lines,
                    segments.len(),
                    channel.saturating_sub(source_rank),
                    std::cmp::Reverse(available),
                ),
                channel,
                segment_index,
            ));
        }
        candidates.sort_by_key(|(score, _, _)| *score);
        let Some((_, host_channel, host_segment)) = candidates.into_iter().next() else {
            return Err(DiagramError::Routing(format!(
                "relationship caption `{label}` has no route segment"
            )));
        };
        channels[host_channel][host_segment].label = Some(label.to_string());
        for segments in channels.iter_mut().skip(host_channel + 1) {
            if let Some(segment) = segments
                .iter_mut()
                .find(|segment| segment.edge == edge_index)
            {
                segment.accented = true;
            }
        }
    }
    Ok(())
}

fn caption_candidate_crossings(
    channels: &[Vec<Segment>],
    host_channel: usize,
    host_segment: usize,
    label: &str,
    nodes: &[NodeGeom],
    virtuals: &[VirtualGeom],
    width: usize,
) -> usize {
    // Caption width changes branch spacing and can alter routes in neighboring
    // channels, so score the complete candidate layout rather than one channel.
    let mut candidate = channels.to_vec();
    candidate[host_channel][host_segment].label = Some(label.to_string());

    let mut crossings = 0;
    for segments in &mut candidate {
        assign_ports(segments, nodes, virtuals);
        if assign_anchors(segments, width).is_err() {
            return usize::MAX;
        }
        align_node_ports_with_branches(segments, nodes);
        match plan_channel(segments.clone(), width) {
            Ok(channel) => crossings += channel.crossovers.len(),
            Err(_) => return usize::MAX,
        }
    }
    crossings
}

fn endpoint_x(end: End, nodes: &[NodeGeom], virtuals: &[VirtualGeom]) -> usize {
    match end {
        End::Node(node) => nodes[node].center(),
        End::Virtual(virtual_node) => virtuals[virtual_node].x,
    }
}

fn assign_ports(segments: &mut [Segment], nodes: &[NodeGeom], virtuals: &[VirtualGeom]) {
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
        if let End::Virtual(virtual_node) = segment.from {
            segment.sx = virtuals[virtual_node].x;
        }
        if let End::Virtual(virtual_node) = segment.to {
            segment.tx = virtuals[virtual_node].x;
        }
    }

    for (node, mut indices) in by_source {
        indices.sort_by_key(|index| {
            (
                endpoint_x(segments[*index].to, nodes, virtuals),
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
                endpoint_x(segments[*index].from, nodes, virtuals),
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

    let outer_caption_room = segments
        .iter()
        .filter_map(|segment| segment.label.as_deref().map(caption_width))
        .max()
        .map(|caption| caption + CAPTION_ATTACHMENT_SPAN)
        .unwrap_or(0);
    let left = MARGIN_X + 1 + outer_caption_room;
    let right = width.saturating_sub(MARGIN_X + 2 + outer_caption_room);
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
                    + 2 * CAPTION_ATTACHMENT_SPAN
                    + 1
            } else {
                UNLABELED_TRACK_GAP
            },
        );
    }

    let mut lower = vec![left; order.len()];
    for position in 0..order.len() {
        lower[position] = if position == 0 {
            left
        } else {
            lower[position - 1] + separations[position - 1]
        };
    }

    let mut upper = vec![right; order.len()];
    for position in (0..order.len()).rev() {
        upper[position] = if position + 1 == order.len() {
            right
        } else {
            upper[position + 1]
                .checked_sub(separations[position])
                .ok_or_else(|| {
                    DiagramError::Routing("relationship captions need a wider branch field".into())
                })?
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
        positions[position] = preferred[order[position]].clamp(minimum, upper[position]);
    }
    for (position, segment) in order.into_iter().enumerate() {
        segments[segment].anchor = positions[position];
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct RouteInterval {
    segment: usize,
    start: usize,
    lo: usize,
    hi: usize,
    edge: usize,
}

fn assign_bus_lanes(
    segments: &[Segment],
    source_side: bool,
) -> Result<(HashMap<usize, usize>, usize), DiagramError> {
    let mut routes = Vec::new();
    for (index, segment) in segments.iter().enumerate() {
        let (start, end) = if source_side {
            (segment.sx, segment.anchor)
        } else {
            (segment.anchor, segment.tx)
        };
        let lo = start.min(end);
        let hi = start.max(end);
        if lo != hi {
            routes.push(RouteInterval {
                segment: index,
                start,
                lo,
                hi,
                edge: segment.edge,
            });
        }
    }

    // A horizontal turn must happen after every vertical stem it crosses has
    // already turned away. These dependencies produce planar outside-in bends
    // for both fan-out and fan-in, independent of route length or direction.
    let mut predecessors: HashMap<usize, HashSet<usize>> = HashMap::new();
    for route in &routes {
        for other in &routes {
            if route.segment != other.segment && route.lo < other.start && other.start < route.hi {
                predecessors
                    .entry(route.segment)
                    .or_default()
                    .insert(other.segment);
            }
        }
    }

    let mut lanes: Vec<Vec<RouteInterval>> = Vec::new();
    let mut assigned = HashMap::new();
    let mut pending = routes;
    while !pending.is_empty() {
        let Some(next) = pending
            .iter()
            .enumerate()
            .filter(|(_, route)| {
                predecessors.get(&route.segment).is_none_or(|required| {
                    required
                        .iter()
                        .all(|segment| assigned.contains_key(segment))
                })
            })
            .min_by_key(|(_, route)| (route.start, route.hi, route.edge))
            .map(|(index, _)| index)
        else {
            return Err(DiagramError::Routing(
                "relationship order crosses inside a rank channel".into(),
            ));
        };
        let route = pending.remove(next);
        let first_lane = predecessors
            .get(&route.segment)
            .into_iter()
            .flatten()
            .map(|segment| assigned[segment] + 1)
            .max()
            .unwrap_or(0);
        let lane = (first_lane..=lanes.len())
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
    Ok((assigned, lanes.len()))
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

fn caption_availability(anchor: usize, anchors: &[usize], width: usize) -> (usize, usize) {
    let position = anchors
        .binary_search(&anchor)
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
    (
        anchor.saturating_sub(left_boundary + CAPTION_ATTACHMENT_SPAN),
        right_boundary.saturating_sub(anchor + CAPTION_ATTACHMENT_SPAN),
    )
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
        let (left_available, right_available) =
            caption_availability(segment.anchor, &anchors, width);
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

    let (source_lanes, source_lane_count) = assign_bus_lanes(&segments, true)?;
    let (target_lanes, target_lane_count) = assign_bus_lanes(&segments, false)?;
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
        let target_is_virtual = matches!(segment.to, End::Virtual(_));
        let vertical_end = final_row;

        let mut source_points = vec![(segment.sx, 0)];
        if let Some(row) = source_row {
            push_point(&mut source_points, (segment.sx, row));
            push_point(&mut source_points, (segment.anchor, row));
        } else {
            debug_assert_eq!(segment.sx, segment.anchor);
        }
        if target_is_virtual && segment.label.is_none() {
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
                style: segment.route_style(),
                edge: segment.edge,
            });
        } else {
            push_point(&mut source_points, (segment.anchor, caption_start));
            strokes.push(LocalStroke {
                cells: trace_polyline(&source_points),
                dashed: segment.dashed(),
                style: segment.route_style(),
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
    fn virtual_segment_uses_its_target_waypoint_as_the_anchor() {
        let mut segments = vec![
            Segment {
                edge: 0,
                from: End::Virtual(0),
                to: End::Virtual(1),
                sx: 40,
                tx: 44,
                anchor: 0,
                label: None,
                accented: false,
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
                accented: false,
                kind: EdgeKind::Sync,
            },
        ];

        assign_anchors(&mut segments, 120).expect("waypoints should fit");

        assert_eq!(segments[0].anchor, 44);
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
                accented: false,
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
                    accented: false,
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
    fn route_lanes_turn_crossed_vertical_stems_first() {
        let segments = vec![
            Segment {
                edge: 0,
                from: End::Node(0),
                to: End::Node(1),
                sx: 40,
                tx: 50,
                anchor: 60,
                label: None,
                accented: false,
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
                accented: false,
                kind: EdgeKind::Sync,
            },
        ];

        let (fan_out, _) = assign_bus_lanes(&segments, true).expect("fan-out lanes");
        let (fan_in, _) = assign_bus_lanes(&segments, false).expect("fan-in lanes");

        assert_eq!(fan_out[&1], 0);
        assert_eq!(fan_out[&0], 1);
        assert_eq!(fan_in[&0], 0);
        assert_eq!(fan_in[&1], 1);
    }
}
