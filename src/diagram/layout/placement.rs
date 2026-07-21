//! Layered horizontal placement for real nodes and relationship waypoints.
//!
//! Every long relationship contributes a virtual entry to each intermediate
//! rank. Crossing reduction and coordinate assignment therefore see the full
//! graph before any route is painted. Real nodes keep their authored order;
//! virtual entries move through the available gaps to keep long relationships
//! coherent across the ranks they cross.

use std::cmp::Ordering;
use std::collections::HashMap;

use super::{caption_width, NodeGeom, CAPTION_ATTACHMENT_SPAN, MARGIN_X};
use crate::diagram::doc::{DiagramError, Model};

const MIN_NODE_GAP: usize = 6;
const MAX_NODE_GAP: usize = 18;
const VIRTUAL_GAP: usize = 3;
const NATURAL_WIDTH: usize = 96;
const ORDER_SWEEPS: usize = 8;
const POSITION_SWEEPS: usize = 8;
const DIRECT_NODE_WEIGHT: usize = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Entry {
    Node(usize),
    Virtual(usize),
}

#[derive(Debug, Clone)]
pub(super) struct VirtualGeom {
    pub edge: usize,
    pub rank: usize,
    pub x: usize,
    pub accented: bool,
    width: usize,
}

pub(super) struct Placement {
    pub width: usize,
    pub virtuals: Vec<VirtualGeom>,
}

struct LayeredGraph {
    ranks: Vec<Vec<Entry>>,
    virtuals: Vec<VirtualGeom>,
    neighbors: HashMap<Entry, Vec<Entry>>,
}

pub(super) fn place(
    model: &Model,
    nodes: &mut [NodeGeom],
    viewport: usize,
) -> Result<Placement, DiagramError> {
    let territories = node_territories(model, nodes);
    let mut graph = expand_long_edges(model);
    reduce_crossings(model, &mut graph);

    let minimum_rank_width = graph
        .ranks
        .iter()
        .map(|rank| rank_width(&graph, rank, &territories, MIN_NODE_GAP))
        .max()
        .unwrap_or(1);
    let channel_width = minimum_channel_width(model);
    let title_width = model
        .title
        .as_ref()
        .map(|title| title.chars().count())
        .unwrap_or(1);
    let minimum_width = minimum_rank_width
        .max(channel_width)
        .saturating_add(2 * MARGIN_X)
        .max(title_width + 2 * MARGIN_X);
    let width = if viewport == usize::MAX {
        minimum_width.max(NATURAL_WIDTH)
    } else {
        minimum_width.max(viewport.max(1))
    };

    let real_gaps = graph
        .ranks
        .iter()
        .map(|rank| {
            rank.windows(2)
                .filter(|pair| matches!(pair, [Entry::Node(_), Entry::Node(_)]))
                .count()
        })
        .max()
        .unwrap_or(0);
    let extra = width
        .saturating_sub(2 * MARGIN_X)
        .saturating_sub(minimum_rank_width);
    let node_gap = (MIN_NODE_GAP + extra.checked_div(real_gaps).unwrap_or(0)).min(MAX_NODE_GAP);
    let positions = assign_coordinates(&graph, &territories, width, node_gap);

    for (index, node) in nodes.iter_mut().enumerate() {
        let center = positions[&Entry::Node(index)];
        node.x = center.saturating_sub(node.w / 2);
    }
    for (index, virtual_node) in graph.virtuals.iter_mut().enumerate() {
        virtual_node.x = positions[&Entry::Virtual(index)];
    }

    Ok(Placement {
        width,
        virtuals: graph.virtuals,
    })
}

fn expand_long_edges(model: &Model) -> LayeredGraph {
    let mut ranks = model
        .ranks
        .iter()
        .map(|rank| rank.iter().copied().map(Entry::Node).collect::<Vec<_>>())
        .collect::<Vec<_>>();
    let mut virtuals = Vec::new();
    let mut neighbors: HashMap<Entry, Vec<Entry>> = HashMap::new();

    for (edge_index, edge) in model.edges.iter().enumerate() {
        let source_rank = model.nodes[edge.from].rank;
        let target_rank = model.nodes[edge.to].rank;
        let mut chain = vec![Entry::Node(edge.from)];
        for (rank, entries) in ranks
            .iter_mut()
            .enumerate()
            .take(target_rank)
            .skip(source_rank + 1)
        {
            let virtual_node = virtuals.len();
            virtuals.push(VirtualGeom {
                edge: edge_index,
                rank,
                x: 0,
                accented: false,
                width: edge
                    .label
                    .as_deref()
                    .map(relationship_territory)
                    .unwrap_or(1),
            });
            let entry = Entry::Virtual(virtual_node);
            entries.push(entry);
            chain.push(entry);
        }
        chain.push(Entry::Node(edge.to));
        for pair in chain.windows(2) {
            neighbors.entry(pair[0]).or_default().push(pair[1]);
            neighbors.entry(pair[1]).or_default().push(pair[0]);
        }
    }

    LayeredGraph {
        ranks,
        virtuals,
        neighbors,
    }
}

fn reduce_crossings(model: &Model, graph: &mut LayeredGraph) {
    let mut best_ranks = graph.ranks.clone();
    let mut best_crossings = crossing_count(model, graph);
    for _ in 0..ORDER_SWEEPS {
        for rank in 1..graph.ranks.len() {
            reorder_rank(model, graph, rank, rank - 1);
        }
        retain_better_order(model, graph, &mut best_ranks, &mut best_crossings);
        for rank in (0..graph.ranks.len().saturating_sub(1)).rev() {
            reorder_rank(model, graph, rank, rank + 1);
        }
        retain_better_order(model, graph, &mut best_ranks, &mut best_crossings);
    }
    graph.ranks = best_ranks;
}

fn retain_better_order(
    model: &Model,
    graph: &LayeredGraph,
    best_ranks: &mut Vec<Vec<Entry>>,
    best_crossings: &mut usize,
) {
    let crossings = crossing_count(model, graph);
    if crossings < *best_crossings {
        *best_crossings = crossings;
        *best_ranks = graph.ranks.clone();
    }
}

fn crossing_count(model: &Model, graph: &LayeredGraph) -> usize {
    let positions = entry_positions(&graph.ranks);
    let mut crossings = 0;
    for rank in 0..graph.ranks.len().saturating_sub(1) {
        let mut relationships = Vec::new();
        for source in &graph.ranks[rank] {
            for target in graph.neighbors.get(source).into_iter().flatten() {
                if entry_rank(*target, model, graph) == rank + 1 {
                    relationships.push((positions[source], positions[target]));
                }
            }
        }
        for left in 0..relationships.len() {
            for right in left + 1..relationships.len() {
                let (left_source, left_target) = relationships[left];
                let (right_source, right_target) = relationships[right];
                if left_source != right_source
                    && left_target != right_target
                    && (left_source < right_source) != (left_target < right_target)
                {
                    crossings += 1;
                }
            }
        }
    }
    crossings
}

fn reorder_rank(model: &Model, graph: &mut LayeredGraph, rank: usize, adjacent_rank: usize) {
    if graph.ranks[rank].len() < 2 {
        return;
    }
    let positions = entry_positions(&graph.ranks);
    let original_positions = graph.ranks[rank]
        .iter()
        .enumerate()
        .map(|(position, entry)| (*entry, position))
        .collect::<HashMap<_, _>>();
    let mut ordered = graph.ranks[rank].clone();
    ordered.sort_by(|left, right| {
        barycenter(*left, graph, model, &positions, adjacent_rank)
            .cmp(&barycenter(*right, graph, model, &positions, adjacent_rank))
            .then_with(|| original_positions[left].cmp(&original_positions[right]))
    });

    // Hints define real-node order. Crossing reduction may only decide which
    // gaps the virtual relationship entries occupy.
    let real_slots = ordered
        .iter()
        .enumerate()
        .filter_map(|(position, entry)| matches!(entry, Entry::Node(_)).then_some(position))
        .collect::<Vec<_>>();
    for (slot, node) in real_slots.into_iter().zip(model.ranks[rank].iter()) {
        ordered[slot] = Entry::Node(*node);
    }
    graph.ranks[rank] = ordered;
}

#[derive(Clone, Copy, PartialEq, Eq)]
struct Rational {
    numerator: usize,
    denominator: usize,
}

impl Ord for Rational {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.numerator * other.denominator).cmp(&(other.numerator * self.denominator))
    }
}

impl PartialOrd for Rational {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

fn barycenter(
    entry: Entry,
    graph: &LayeredGraph,
    model: &Model,
    positions: &HashMap<Entry, usize>,
    adjacent_rank: usize,
) -> Rational {
    let neighbors = graph
        .neighbors
        .get(&entry)
        .into_iter()
        .flatten()
        .filter(|neighbor| entry_rank(**neighbor, model, graph) == adjacent_rank)
        .collect::<Vec<_>>();
    if neighbors.is_empty() {
        return Rational {
            numerator: positions[&entry],
            denominator: 1,
        };
    }
    Rational {
        numerator: neighbors.iter().map(|neighbor| positions[neighbor]).sum(),
        denominator: neighbors.len(),
    }
}

fn entry_rank(entry: Entry, model: &Model, graph: &LayeredGraph) -> usize {
    match entry {
        Entry::Node(node) => model.nodes[node].rank,
        Entry::Virtual(virtual_node) => graph.virtuals[virtual_node].rank,
    }
}

fn entry_positions(ranks: &[Vec<Entry>]) -> HashMap<Entry, usize> {
    ranks
        .iter()
        .flat_map(|rank| {
            rank.iter()
                .enumerate()
                .map(|(position, entry)| (*entry, position))
        })
        .collect()
}

fn assign_coordinates(
    graph: &LayeredGraph,
    territories: &[usize],
    width: usize,
    node_gap: usize,
) -> HashMap<Entry, usize> {
    let mut positions = HashMap::new();
    for rank in &graph.ranks {
        let span = rank_width(graph, rank, territories, node_gap);
        let mut cursor = (width.saturating_sub(span)) / 2;
        for (index, entry) in rank.iter().enumerate() {
            let entry_width = entry_width(graph, *entry, territories);
            positions.insert(*entry, cursor + entry_width / 2);
            cursor += entry_width;
            if let Some(next) = rank.get(index + 1) {
                cursor += entry_gap(*entry, *next, node_gap);
            }
        }
    }

    for _ in 0..POSITION_SWEEPS {
        for rank in 1..graph.ranks.len() {
            relax_rank(graph, territories, &mut positions, rank, width, node_gap);
        }
        for rank in (0..graph.ranks.len().saturating_sub(1)).rev() {
            relax_rank(graph, territories, &mut positions, rank, width, node_gap);
        }
    }
    positions
}

fn relax_rank(
    graph: &LayeredGraph,
    territories: &[usize],
    positions: &mut HashMap<Entry, usize>,
    rank: usize,
    width: usize,
    node_gap: usize,
) {
    let entries = &graph.ranks[rank];
    if entries.is_empty() {
        return;
    }
    let mut desired = entries
        .iter()
        .map(|entry| {
            let mut neighbors = graph
                .neighbors
                .get(entry)
                .into_iter()
                .flatten()
                .flat_map(|neighbor| {
                    let weight = usize::from(matches!(
                        (*entry, *neighbor),
                        (Entry::Node(_), Entry::Node(_))
                    )) * (DIRECT_NODE_WEIGHT - 1)
                        + 1;
                    std::iter::repeat_n(positions[neighbor], weight)
                })
                .collect::<Vec<_>>();
            if neighbors.is_empty() {
                positions[entry]
            } else {
                median(&mut neighbors)
            }
        })
        .collect::<Vec<_>>();

    let first_width = entry_width(graph, entries[0], territories);
    desired[0] = desired[0].max(MARGIN_X + first_width / 2);
    for index in 1..entries.len() {
        let separation = center_separation(
            graph,
            entries[index - 1],
            entries[index],
            territories,
            node_gap,
        );
        desired[index] = desired[index].max(desired[index - 1] + separation);
    }

    let last = entries.len() - 1;
    let last_width = entry_width(graph, entries[last], territories);
    let maximum = width.saturating_sub(MARGIN_X + last_width.div_ceil(2));
    desired[last] = desired[last].min(maximum);
    for index in (0..last).rev() {
        let separation = center_separation(
            graph,
            entries[index],
            entries[index + 1],
            territories,
            node_gap,
        );
        desired[index] = desired[index].min(desired[index + 1].saturating_sub(separation));
    }

    let minimum = MARGIN_X + first_width / 2;
    if desired[0] < minimum {
        let shift = minimum - desired[0];
        for center in &mut desired {
            *center += shift;
        }
    }
    for (entry, center) in entries.iter().zip(desired) {
        positions.insert(*entry, center);
    }
}

fn node_territories(model: &Model, nodes: &[NodeGeom]) -> Vec<usize> {
    let mut territories = nodes.iter().map(|node| node.w).collect::<Vec<_>>();
    for edge in &model.edges {
        if let Some(label) = &edge.label {
            let caption_territory = caption_width(label) + CAPTION_ATTACHMENT_SPAN + 1;
            territories[edge.to] = territories[edge.to].max(caption_territory);
        }
    }
    territories
}

fn minimum_channel_width(model: &Model) -> usize {
    let mut required = 1;
    for channel in 0..model.ranks.len().saturating_sub(1) {
        let active = model
            .edges
            .iter()
            .filter(|edge| {
                model.nodes[edge.from].rank <= channel && model.nodes[edge.to].rank > channel
            })
            .collect::<Vec<_>>();
        let labeled = active
            .iter()
            .filter_map(|edge| {
                (model.nodes[edge.to].rank == channel + 1)
                    .then_some(edge.label.as_deref())
                    .flatten()
            })
            .map(caption_width)
            .collect::<Vec<_>>();
        if labeled.is_empty() {
            continue;
        }
        let gaps = active.len().saturating_sub(1);
        let caption_gaps = gaps.min(labeled.len() * 2);
        let plain_gaps = gaps - caption_gaps;
        let widest_caption = labeled
            .into_iter()
            .max()
            .unwrap_or(super::MIN_CAPTION_WIDTH);
        let caption_separation = widest_caption + 2 * CAPTION_ATTACHMENT_SPAN + 1;
        let outer_caption_room = widest_caption + CAPTION_ATTACHMENT_SPAN + 1;
        let span = caption_gaps * caption_separation + plain_gaps * VIRTUAL_GAP;
        required = required.max(span + 2 * outer_caption_room);
    }
    required
}

fn rank_width(
    graph: &LayeredGraph,
    rank: &[Entry],
    territories: &[usize],
    node_gap: usize,
) -> usize {
    rank.iter()
        .map(|entry| entry_width(graph, *entry, territories))
        .sum::<usize>()
        + rank
            .windows(2)
            .map(|pair| entry_gap(pair[0], pair[1], node_gap))
            .sum::<usize>()
}

fn entry_width(graph: &LayeredGraph, entry: Entry, territories: &[usize]) -> usize {
    match entry {
        Entry::Node(node) => territories[node],
        Entry::Virtual(virtual_node) => graph.virtuals[virtual_node].width,
    }
}

fn entry_gap(left: Entry, right: Entry, node_gap: usize) -> usize {
    if matches!((left, right), (Entry::Node(_), Entry::Node(_))) {
        node_gap
    } else {
        VIRTUAL_GAP
    }
}

fn center_separation(
    graph: &LayeredGraph,
    left: Entry,
    right: Entry,
    territories: &[usize],
    node_gap: usize,
) -> usize {
    entry_width(graph, left, territories).div_ceil(2)
        + entry_gap(left, right, node_gap)
        + entry_width(graph, right, territories) / 2
}

fn relationship_territory(label: &str) -> usize {
    caption_width(label) + 2 * CAPTION_ATTACHMENT_SPAN + 1
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diagram::doc::{EdgeKind, ModelEdge, ModelNode, NodeKind};

    fn layered_model() -> Model {
        Model {
            title: None,
            ticket: None,
            nodes: vec![
                ModelNode {
                    label: "Source".into(),
                    kind: NodeKind::Service,
                    rank: 0,
                },
                ModelNode {
                    label: "Middle".into(),
                    kind: NodeKind::Service,
                    rank: 1,
                },
                ModelNode {
                    label: "Target".into(),
                    kind: NodeKind::Service,
                    rank: 2,
                },
            ],
            edges: vec![ModelEdge {
                from: 0,
                to: 2,
                label: Some("long relationship".into()),
                kind: EdgeKind::Sync,
            }],
            notes: Vec::new(),
            ranks: vec![vec![0], vec![1], vec![2]],
        }
    }

    #[test]
    fn long_relationships_expand_into_every_intermediate_rank() {
        let graph = expand_long_edges(&layered_model());

        assert_eq!(graph.virtuals.len(), 1);
        assert_eq!(graph.virtuals[0].edge, 0);
        assert_eq!(graph.virtuals[0].rank, 1);
        assert_eq!(
            graph.virtuals[0].width,
            relationship_territory("long relationship")
        );
        assert!(matches!(graph.ranks[1][0], Entry::Node(1)));
        assert!(graph.ranks[1].contains(&Entry::Virtual(0)));
    }

    #[test]
    fn crossing_reduction_preserves_authored_real_node_order() {
        let model = layered_model();
        let mut graph = expand_long_edges(&model);
        reduce_crossings(&model, &mut graph);

        let real_nodes = graph.ranks[1]
            .iter()
            .filter_map(|entry| match entry {
                Entry::Node(node) => Some(*node),
                Entry::Virtual(_) => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(real_nodes, model.ranks[1]);
    }

    #[test]
    fn converging_long_relationships_keep_a_crossing_free_rank_order() {
        let nodes = (0..6)
            .map(|index| ModelNode {
                label: index.to_string(),
                kind: NodeKind::Service,
                rank: [0, 1, 2, 3, 3, 4][index],
            })
            .collect();
        let edge = |from, to| ModelEdge {
            from,
            to,
            label: None,
            kind: EdgeKind::Sync,
        };
        let model = Model {
            title: None,
            ticket: None,
            nodes,
            edges: vec![
                edge(0, 1),
                edge(0, 4),
                edge(1, 2),
                edge(1, 4),
                edge(2, 3),
                edge(2, 5),
                edge(4, 5),
            ],
            notes: Vec::new(),
            ranks: vec![vec![0], vec![1], vec![2], vec![3, 4], vec![5]],
        };
        let mut graph = expand_long_edges(&model);
        reduce_crossings(&model, &mut graph);

        assert_eq!(graph.ranks[1], vec![Entry::Node(1), Entry::Virtual(0)]);
        assert_eq!(
            graph.ranks[2],
            vec![Entry::Node(2), Entry::Virtual(2), Entry::Virtual(1)]
        );
        assert_eq!(
            graph.ranks[3],
            vec![Entry::Node(3), Entry::Virtual(3), Entry::Node(4)]
        );
    }
}
