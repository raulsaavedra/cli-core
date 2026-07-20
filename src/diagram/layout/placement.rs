//! Topology-shaped horizontal placement.
//!
//! Each rank forms a compact block of node territories. Territories reserve
//! enough room for node boxes and nearby relationship captions. Alternating
//! alignment sweeps move complete rank blocks toward their connected neighbors
//! while preserving authored order. Long edges receive the nearest clear
//! interior track through intervening ranks.

use std::collections::HashSet;

use super::{NodeGeom, CAPTION_TRACK_WIDTH, MARGIN_X};
use crate::diagram::doc::{DiagramError, Model};

const MIN_NODE_GAP: usize = 6;
const MAX_NODE_GAP: usize = 18;
const NATURAL_WIDTH: usize = 96;
const TRACK_CLEARANCE: usize = 2;
const TRACK_SEPARATION: usize = 3;

#[derive(Debug, Clone)]
pub(super) struct Track {
    pub edge: usize,
    pub x: usize,
    pub source_rank: usize,
    pub target_rank: usize,
}

pub(super) struct Placement {
    pub width: usize,
    pub tracks: Vec<Track>,
}

#[derive(Clone, Copy)]
struct Slot {
    center: usize,
    width: usize,
}

pub(super) fn place(
    model: &Model,
    nodes: &mut [NodeGeom],
    viewport: usize,
) -> Result<Placement, DiagramError> {
    let territories = node_territories(model, nodes);
    let widest_rank = model
        .ranks
        .iter()
        .map(|rank| rank_width(rank, &territories, MIN_NODE_GAP))
        .max()
        .unwrap_or(1);
    let channel_width = minimum_channel_width(model);
    let title_width = model
        .title
        .as_ref()
        .map(|title| title.chars().count())
        .unwrap_or(1);
    let long_edges = model
        .edges
        .iter()
        .filter(|edge| model.nodes[edge.to].rank > model.nodes[edge.from].rank + 1)
        .count();
    let track_room = if long_edges == 0 {
        0
    } else {
        2 * TRACK_CLEARANCE + long_edges * TRACK_SEPARATION
    };
    let minimum_width = widest_rank
        .max(channel_width)
        .saturating_add(2 * MARGIN_X + track_room)
        .max(title_width + 2 * MARGIN_X);
    let width = if viewport == usize::MAX {
        minimum_width.max(NATURAL_WIDTH)
    } else {
        minimum_width.max(viewport.max(1))
    };

    let max_rank_nodes = model.ranks.iter().map(Vec::len).max().unwrap_or(1);
    let available = width.saturating_sub(2 * MARGIN_X);
    let expandable_gaps = max_rank_nodes.saturating_sub(1);
    let extra = available.saturating_sub(widest_rank);
    let gap = (MIN_NODE_GAP + extra.checked_div(expandable_gaps).unwrap_or(0)).min(MAX_NODE_GAP);

    let mut slots = place_rank_blocks(model, &territories, width, gap);
    align_rank_blocks(model, &mut slots, width);
    center_complete_graph(&mut slots, width);

    for (index, node) in nodes.iter_mut().enumerate() {
        node.x = slots[index].center.saturating_sub(node.w / 2);
    }

    let tracks = place_long_tracks(model, nodes, width)?;
    Ok(Placement { width, tracks })
}

fn node_territories(model: &Model, nodes: &[NodeGeom]) -> Vec<usize> {
    let mut territories: Vec<usize> = nodes.iter().map(|node| node.w).collect();
    for edge in &model.edges {
        if edge.label.is_some() {
            territories[edge.from] = territories[edge.from].max(CAPTION_TRACK_WIDTH);
            territories[edge.to] = territories[edge.to].max(CAPTION_TRACK_WIDTH);
        }
    }
    territories
}

fn minimum_channel_width(model: &Model) -> usize {
    let mut required = 1;
    for channel in 0..model.ranks.len().saturating_sub(1) {
        let active: Vec<_> = model
            .edges
            .iter()
            .filter(|edge| {
                model.nodes[edge.from].rank <= channel && model.nodes[edge.to].rank > channel
            })
            .collect();
        let labeled = active
            .iter()
            .filter(|edge| model.nodes[edge.to].rank == channel + 1 && edge.label.is_some())
            .count();
        if labeled == 0 {
            continue;
        }

        let gaps = active.len().saturating_sub(1);
        let caption_gaps = gaps.min(labeled * 2);
        let compact_gaps = gaps - caption_gaps;
        let span = caption_gaps * CAPTION_TRACK_WIDTH + compact_gaps * TRACK_SEPARATION;
        let outer_caption_room = 2 * (super::MIN_CAPTION_WIDTH + 3);
        required = required.max(span + outer_caption_room);
    }
    required
}

fn rank_width(rank: &[usize], territories: &[usize], gap: usize) -> usize {
    rank.iter().map(|node| territories[*node]).sum::<usize>() + rank.len().saturating_sub(1) * gap
}

fn place_rank_blocks(model: &Model, territories: &[usize], width: usize, gap: usize) -> Vec<Slot> {
    let mut slots = vec![
        Slot {
            center: 0,
            width: 1
        };
        model.nodes.len()
    ];
    for rank in &model.ranks {
        let span = rank_width(rank, territories, gap);
        let mut x = (width.saturating_sub(span)) / 2;
        for node in rank {
            let territory = territories[*node];
            slots[*node] = Slot {
                center: x + territory / 2,
                width: territory,
            };
            x += territory + gap;
        }
    }
    slots
}

fn align_rank_blocks(model: &Model, slots: &mut [Slot], width: usize) {
    for _ in 0..6 {
        for rank in 0..model.ranks.len() {
            align_rank(model, slots, rank, width);
        }
        for rank in (0..model.ranks.len()).rev() {
            align_rank(model, slots, rank, width);
        }
    }
}

fn align_rank(model: &Model, slots: &mut [Slot], rank: usize, width: usize) {
    let mut deltas = Vec::new();
    for node in &model.ranks[rank] {
        let mut neighbors = Vec::new();
        for edge in &model.edges {
            if edge.from == *node {
                neighbors.push(slots[edge.to].center);
            }
            if edge.to == *node {
                neighbors.push(slots[edge.from].center);
            }
        }
        if !neighbors.is_empty() {
            deltas.push(median(&mut neighbors) as isize - slots[*node].center as isize);
        }
    }
    if deltas.is_empty() {
        return;
    }
    deltas.sort_unstable();
    let mut delta = deltas[deltas.len() / 2];

    let first = model.ranks[rank][0];
    let last = *model.ranks[rank].last().expect("rank is non-empty");
    let left = slots[first].center.saturating_sub(slots[first].width / 2);
    let right = slots[last].center + slots[last].width.div_ceil(2);
    delta = delta.max(MARGIN_X as isize - left as isize);
    delta = delta.min((width.saturating_sub(MARGIN_X + right)) as isize);

    for node in &model.ranks[rank] {
        slots[*node].center = slots[*node].center.saturating_add_signed(delta);
    }
}

fn center_complete_graph(slots: &mut [Slot], width: usize) {
    let left = slots
        .iter()
        .map(|slot| slot.center.saturating_sub(slot.width / 2))
        .min()
        .unwrap_or(MARGIN_X);
    let right = slots
        .iter()
        .map(|slot| slot.center + slot.width.div_ceil(2))
        .max()
        .unwrap_or(width.saturating_sub(MARGIN_X));
    let target = width / 2;
    let current = (left + right) / 2;
    let mut delta = target as isize - current as isize;
    delta = delta.max(MARGIN_X as isize - left as isize);
    delta = delta.min((width.saturating_sub(MARGIN_X + right)) as isize);
    for slot in slots {
        slot.center = slot.center.saturating_add_signed(delta);
    }
}

fn place_long_tracks(
    model: &Model,
    nodes: &[NodeGeom],
    width: usize,
) -> Result<Vec<Track>, DiagramError> {
    let mut tracks = Vec::new();
    let mut occupied = HashSet::new();
    let graph_left = nodes.iter().map(|node| node.x).min().unwrap_or(MARGIN_X);
    let graph_right = nodes
        .iter()
        .map(|node| node.x + node.w - 1)
        .max()
        .unwrap_or(width.saturating_sub(MARGIN_X + 1));

    for (edge_index, edge) in model.edges.iter().enumerate() {
        let source_rank = model.nodes[edge.from].rank;
        let target_rank = model.nodes[edge.to].rank;
        if target_rank <= source_rank + 1 {
            continue;
        }

        let source = nodes[edge.from].center();
        let target = nodes[edge.to].center();
        let x = (MARGIN_X + 1..width.saturating_sub(MARGIN_X + 1))
            .filter(|candidate| {
                !occupied
                    .iter()
                    .any(|track: &usize| track.abs_diff(*candidate) < TRACK_SEPARATION)
                    && (source_rank + 1..target_rank).all(|rank| {
                        model.ranks[rank].iter().all(|node| {
                            let left = nodes[*node].x.saturating_sub(TRACK_CLEARANCE);
                            let right = nodes[*node].x + nodes[*node].w - 1 + TRACK_CLEARANCE;
                            *candidate < left || *candidate > right
                        })
                    })
            })
            .min_by_key(|candidate| {
                let outside = usize::from(*candidate < graph_left || *candidate > graph_right);
                (
                    outside,
                    candidate.abs_diff(target) * 2 + candidate.abs_diff(source),
                    candidate.abs_diff(target),
                    *candidate,
                )
            })
            .ok_or_else(|| {
                DiagramError::Routing(format!(
                    "no clear interior track from `{}` to `{}`",
                    model.nodes[edge.from].id, model.nodes[edge.to].id
                ))
            })?;

        occupied.insert(x);
        tracks.push(Track {
            edge: edge_index,
            x,
            source_rank,
            target_rank,
        });
    }
    Ok(tracks)
}

fn median(values: &mut [usize]) -> usize {
    values.sort_unstable();
    let middle = values.len() / 2;
    if values.len().is_multiple_of(2) {
        (values[middle - 1] + values[middle]) / 2
    } else {
        values[middle]
    }
}
