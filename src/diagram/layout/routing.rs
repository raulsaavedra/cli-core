//! Local orthogonal routing between adjacent node ranks.
//!
//! Every channel has three visual regions: source-side split buses, a vertical
//! branch field, and target-side merge buses. A labeled relationship uses its
//! final branch as an inline caption track before reaching the target-side bus.

use std::collections::{HashMap, HashSet};

use super::placement::Track;
use super::{NodeGeom, Op, CAPTION_TRACK_WIDTH, MARGIN_X, MIN_CAPTION_WIDTH};
use crate::diagram::doc::{DiagramError, EdgeKind, Model};
use crate::diagram::grid::{Style, E, N, S, W};

const MAX_CAPTION_WIDTH: usize = 32;
const UNLABELED_TRACK_GAP: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum End {
    Node(usize),
    Track(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct BundleKey {
    end: End,
    kind: EdgeKind,
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
    fn source_key(&self) -> BundleKey {
        BundleKey {
            end: self.from,
            kind: self.kind,
        }
    }

    fn target_key(&self) -> BundleKey {
        BundleKey {
            end: self.to,
            kind: self.kind,
        }
    }

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
}

struct LocalStroke {
    cells: Vec<(usize, usize, u8)>,
    dashed: bool,
    style: Style,
    edge: usize,
    source: BundleKey,
    target: BundleKey,
}

struct LocalLabel {
    x: usize,
    y: usize,
    text: String,
}

pub(super) struct ChannelPlan {
    pub height: usize,
    strokes: Vec<LocalStroke>,
    arrows: Vec<(usize, usize)>,
    crossovers: Vec<(usize, usize)>,
    labels: Vec<LocalLabel>,
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
        for (x, y) in self.arrows {
            ops.push(Op::Arrow { x, y: y + top });
        }
        for label in self.labels {
            ops.push(Op::Text {
                x: label.x,
                y: label.y + top,
                text: label.text,
                style: Style::EdgeLabel,
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
                End::Track(track.expect("long edge has an interior track"))
            };
            let to = if channel + 1 == target_rank {
                End::Node(edge.to)
            } else {
                End::Track(track.expect("long edge has an interior track"))
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

    for (node, indices) in by_source {
        let mut groups = endpoint_groups(indices, segments);
        groups.sort_by_key(|group| {
            group
                .iter()
                .map(|index| endpoint_x(segments[*index].to, nodes, tracks))
                .sum::<usize>()
                / group.len()
        });
        for (slot, group) in groups.iter().enumerate() {
            let port = spread_port(&nodes[node], groups.len(), slot);
            for index in group {
                segments[*index].sx = port;
            }
        }
    }

    for (node, indices) in by_target {
        let mut groups = endpoint_groups(indices, segments);
        groups.sort_by_key(|group| {
            group
                .iter()
                .map(|index| endpoint_x(segments[*index].from, nodes, tracks))
                .sum::<usize>()
                / group.len()
        });
        for (slot, group) in groups.iter().enumerate() {
            let port = spread_port(&nodes[node], groups.len(), slot);
            for index in group {
                segments[*index].tx = port;
            }
        }
    }
}

fn endpoint_groups(mut indices: Vec<usize>, segments: &[Segment]) -> Vec<Vec<usize>> {
    indices.sort_by_key(|index| segments[*index].edge);
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for index in indices {
        if let Some(group) = groups
            .iter_mut()
            .find(|group| segments[group[0]].kind == segments[index].kind)
        {
            group.push(index);
        } else {
            groups.push(vec![index]);
        }
    }
    groups
}

fn spread_port(node: &NodeGeom, count: usize, slot: usize) -> usize {
    let interior = node.w - 2;
    node.x + 1 + (interior * (slot + 1)) / (count + 1)
}

fn assign_anchors(segments: &mut [Segment], width: usize) -> Result<(), DiagramError> {
    if segments.is_empty() {
        return Ok(());
    }

    let mut source_counts: HashMap<BundleKey, usize> = HashMap::new();
    let mut target_counts: HashMap<BundleKey, usize> = HashMap::new();
    for segment in segments.iter() {
        *source_counts.entry(segment.source_key()).or_default() += 1;
        *target_counts.entry(segment.target_key()).or_default() += 1;
    }

    let preferred: Vec<usize> = segments
        .iter()
        .map(|segment| {
            let source_bundle = source_counts[&segment.source_key()] > 1;
            let target_bundle = target_counts[&segment.target_key()] > 1;
            match (source_bundle, target_bundle) {
                (true, false) => segment.tx,
                (false, true) => segment.sx,
                (true, true) => (segment.sx + segment.tx) / 2,
                (false, false) => segment.tx,
            }
        })
        .collect();
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
                CAPTION_TRACK_WIDTH
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
struct BusInterval {
    key: BundleKey,
    lo: usize,
    hi: usize,
    first_edge: usize,
}

fn assign_bus_lanes(segments: &[Segment], source_side: bool) -> (HashMap<BundleKey, usize>, usize) {
    let mut groups: HashMap<BundleKey, Vec<usize>> = HashMap::new();
    for (index, segment) in segments.iter().enumerate() {
        let key = if source_side {
            segment.source_key()
        } else {
            segment.target_key()
        };
        groups.entry(key).or_default().push(index);
    }

    let mut buses = Vec::new();
    for (key, members) in groups {
        let endpoint = if source_side {
            segments[members[0]].sx
        } else {
            segments[members[0]].tx
        };
        let lo = members
            .iter()
            .map(|index| segments[*index].anchor.min(endpoint))
            .min()
            .expect("bundle has members");
        let hi = members
            .iter()
            .map(|index| segments[*index].anchor.max(endpoint))
            .max()
            .expect("bundle has members");
        if lo != hi {
            buses.push(BusInterval {
                key,
                lo,
                hi,
                first_edge: members
                    .iter()
                    .map(|index| segments[*index].edge)
                    .min()
                    .unwrap(),
            });
        }
    }
    buses.sort_by_key(|bus| (bus.lo, bus.hi, bus.first_edge));

    let mut lanes: Vec<Vec<BusInterval>> = Vec::new();
    let mut assigned = HashMap::new();
    for bus in buses {
        let lane = (0..=lanes.len())
            .find(|lane| {
                lanes.get(*lane).is_none_or(|occupants| {
                    occupants
                        .iter()
                        .all(|other| bus.hi + 1 < other.lo || other.hi + 1 < bus.lo)
                })
            })
            .expect("a new bus lane is always available");
        if lane == lanes.len() {
            lanes.push(Vec::new());
        }
        lanes[lane].push(bus);
        assigned.insert(bus.key, lane);
    }
    (assigned, lanes.len())
}

struct Caption {
    segment: usize,
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
    let mut captions = Vec::new();

    for (segment_index, segment) in segments.iter().enumerate() {
        let Some(label) = &segment.label else {
            continue;
        };
        let position = anchors
            .binary_search(&segment.anchor)
            .expect("segment anchor is indexed");
        let left_boundary = if position == 0 {
            MARGIN_X
        } else {
            (anchors[position - 1] + segment.anchor) / 2 + 1
        };
        let right_boundary = if position + 1 == anchors.len() {
            width.saturating_sub(MARGIN_X + 1)
        } else {
            (segment.anchor + anchors[position + 1]) / 2 - 1
        };
        let available = right_boundary.saturating_sub(left_boundary) + 1;
        if available < MIN_CAPTION_WIDTH {
            return Err(DiagramError::Routing(format!(
                "edge label `{label}` needs more branch space"
            )));
        }
        let lines = wrap_words(label, available.min(MAX_CAPTION_WIDTH));
        let block_width = lines
            .iter()
            .map(|line| line.chars().count())
            .max()
            .unwrap_or(0);
        let x = segment.anchor.saturating_sub(block_width / 2).clamp(
            left_boundary,
            right_boundary.saturating_sub(block_width - 1),
        );
        captions.push(Caption {
            segment: segment_index,
            lines,
            row: 0,
            x,
            width: block_width,
            lo: x,
            hi: x + block_width - 1,
        });
    }

    captions.sort_by_key(|caption| (caption.lo, caption.hi, caption.segment));
    let mut occupancy: Vec<Vec<(usize, usize)>> = Vec::new();
    let mut height = 1;
    for caption in &mut captions {
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
                break;
            }
            row += 1;
        }
        caption.row = row;
        while occupancy.len() < row + caption.lines.len() {
            occupancy.push(Vec::new());
        }
        for line in occupancy.iter_mut().skip(row).take(caption.lines.len()) {
            line.push((caption.lo, caption.hi));
        }
        height = height.max(row + caption.lines.len());
    }
    Ok((captions, height))
}

fn plan_channel(segments: Vec<Segment>, width: usize) -> Result<ChannelPlan, DiagramError> {
    if segments.is_empty() {
        return Ok(ChannelPlan {
            height: 2,
            strokes: Vec::new(),
            arrows: Vec::new(),
            crossovers: Vec::new(),
            labels: Vec::new(),
        });
    }

    let (source_lanes, source_lane_count) = assign_bus_lanes(&segments, true);
    let (target_lanes, target_lane_count) = assign_bus_lanes(&segments, false);
    let (captions, caption_height) = plan_captions(&segments, width)?;
    let caption_by_segment = captions
        .iter()
        .enumerate()
        .map(|(caption, plan)| (plan.segment, caption))
        .collect::<HashMap<_, _>>();
    let caption_start = source_lane_count + 2;
    let target_start = caption_start + caption_height + 2;
    let arrow_row = target_start + target_lane_count + 1;
    let height = arrow_row + 1;

    let mut strokes = Vec::new();
    for (segment_index, segment) in segments.iter().enumerate() {
        let source_row = source_lanes.get(&segment.source_key()).map(|lane| lane + 1);
        let target_row = target_lanes
            .get(&segment.target_key())
            .map(|lane| target_start + lane);
        let target_is_track = matches!(segment.to, End::Track(_));
        let vertical_end = if target_is_track {
            arrow_row
        } else {
            arrow_row - 1
        };

        let caption = caption_by_segment
            .get(&segment_index)
            .map(|caption| &captions[*caption]);
        let caption_top = caption.map(|caption| caption_start + caption.row);
        let caption_bottom = caption_top
            .zip(caption)
            .map(|(top, caption)| top + caption.lines.len().saturating_sub(1));

        let mut points = vec![(segment.sx, 0)];
        if let Some(row) = source_row {
            push_point(&mut points, (segment.sx, row));
            push_point(&mut points, (segment.anchor, row));
        } else {
            debug_assert_eq!(segment.sx, segment.anchor);
        }
        if let Some(top) = caption_top {
            push_point(&mut points, (segment.anchor, top - 1));
        } else {
            if let Some(row) = target_row {
                push_point(&mut points, (segment.anchor, row));
                push_point(&mut points, (segment.tx, row));
            } else {
                debug_assert_eq!(segment.anchor, segment.tx);
            }
            push_point(&mut points, (segment.tx, vertical_end));
        }
        strokes.push(LocalStroke {
            cells: trace_polyline(&points),
            dashed: segment.dashed(),
            style: segment.style(),
            edge: segment.edge,
            source: segment.source_key(),
            target: segment.target_key(),
        });

        if let Some(bottom) = caption_bottom {
            let mut points = vec![(segment.anchor, bottom + 1)];
            if let Some(row) = target_row {
                push_point(&mut points, (segment.anchor, row));
                push_point(&mut points, (segment.tx, row));
            } else {
                debug_assert_eq!(segment.anchor, segment.tx);
            }
            push_point(&mut points, (segment.tx, vertical_end));
            strokes.push(LocalStroke {
                cells: trace_polyline(&points),
                dashed: segment.dashed(),
                style: segment.style(),
                edge: segment.edge,
                source: segment.source_key(),
                target: segment.target_key(),
            });
        }
    }

    let mut labels = Vec::new();
    for caption in captions {
        let y = caption_start + caption.row;
        for (line, text) in caption.lines.into_iter().enumerate() {
            let text_width = text.chars().count();
            labels.push(LocalLabel {
                x: caption.x + (caption.width - text_width) / 2,
                y: y + line,
                text,
            });
        }
    }

    let crossovers = find_crossovers(&strokes)?;
    let mut emitted = HashSet::new();
    let mut arrows = Vec::new();
    for segment in &segments {
        if matches!(segment.to, End::Node(_)) && emitted.insert(segment.target_key()) {
            arrows.push((segment.tx, arrow_row));
        }
    }

    Ok(ChannelPlan {
        height,
        strokes,
        arrows,
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
    a.edge == b.edge || a.source == b.source || a.target == b.target
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
    fn inline_caption_interrupts_and_resumes_its_branch() {
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
        .expect("an inline caption should fit on a straight branch");

        let caption_top = plan.labels.iter().map(|label| label.y).min().unwrap();
        let caption_bottom = plan.labels.iter().map(|label| label.y).max().unwrap();
        let cells = plan
            .strokes
            .iter()
            .flat_map(|stroke| stroke.cells.iter())
            .collect::<Vec<_>>();

        assert!(cells
            .iter()
            .any(|(x, y, mask)| { *x == anchor && *y + 1 == caption_top && *mask & (N | S) != 0 }));
        assert!(cells.iter().any(|(x, y, mask)| {
            *x == anchor && *y == caption_bottom + 1 && *mask & (N | S) != 0
        }));
        for label in &plan.labels {
            let label_end = label.x + label.text.chars().count();
            assert!(cells
                .iter()
                .all(|(x, y, _)| *y != label.y || *x < label.x || *x >= label_end));
        }
    }
}
