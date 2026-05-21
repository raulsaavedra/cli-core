use std::collections::HashMap;

use super::graph::{Direction, EdgeStyle, FlowGraph, NodeShape};

// ---------------------------------------------------------------------------
// Layout tree types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BorderStyle {
    Solid,
    Rounded,
    Dashed,
}

#[derive(Debug)]
pub struct LayoutBox {
    pub border: Option<BorderStyle>,
    pub label: Option<String>,
    pub content: Option<BoxContent>,
    pub padding: Padding,
    pub children: Vec<PlacedChild>,
    pub width: usize,
    pub height: usize,
}

#[derive(Debug)]
pub struct BoxContent {
    pub text: String,
    pub style: ContentStyle,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ContentStyle {
    NodeLabel,
    DiamondLabel,
    CircleLabel,
}

#[derive(Debug, Clone, Copy)]
pub struct Padding {
    pub top: usize,
    pub left: usize,
}

impl Padding {
    fn zero() -> Self {
        Self { top: 0, left: 0 }
    }
}

#[derive(Debug)]
pub struct PlacedChild {
    pub x: usize,
    pub y: usize,
    pub layout_box: LayoutBox,
}

#[derive(Debug)]
pub struct LayoutEdge {
    pub from_x: usize,
    pub from_y: usize,
    pub to_x: usize,
    pub to_y: usize,
    pub style: EdgeStyle,
    pub label: Option<String>,
    pub lane: usize,
}

#[derive(Debug)]
pub struct LayoutResult {
    pub root: LayoutBox,
    pub edges: Vec<LayoutEdge>,
}

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

const NODE_HEIGHT: usize = 3;
const NODE_PAD: usize = 4;
const GAP_X: usize = 4;
const GAP_Y: usize = 4;
const SG_PAD: usize = 2; // inner padding around subgraph children
const SG_HEADER: usize = 1; // row for subgraph label on the border

// ---------------------------------------------------------------------------
// Public entry
// ---------------------------------------------------------------------------

pub fn compute(graph: &FlowGraph, max_width: usize) -> Option<LayoutResult> {
    let is_horizontal = matches!(graph.direction, Direction::LeftRight | Direction::RightLeft);

    // Build node → subgraph mapping
    let node_to_sg = build_node_sg_map(graph);

    // 1. Layout each subgraph internally
    let sg_boxes: Vec<LayoutBox> = graph
        .subgraphs
        .iter()
        .enumerate()
        .map(|(i, _)| layout_subgraph(graph, i, &node_to_sg, is_horizontal))
        .collect();

    // 2. Build outer graph (regular nodes + compound subgraph nodes) and lay it out
    let root = layout_outer(graph, &node_to_sg, sg_boxes, is_horizontal);

    if root.width > max_width {
        return None;
    }

    // 3. Route edges using absolute positions
    let positions = collect_absolute_positions(&root, 0, 0);
    let edges = route_all_edges(graph, &positions, is_horizontal);

    Some(LayoutResult { root, edges })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn build_node_sg_map(graph: &FlowGraph) -> HashMap<usize, usize> {
    let mut map = HashMap::new();
    for (sg_idx, sg) in graph.subgraphs.iter().enumerate() {
        for nid in &sg.node_ids {
            if let Some(idx) = graph.node_index(nid) {
                map.insert(idx, sg_idx);
            }
        }
    }
    map
}

fn make_node_box(node: &super::graph::Node) -> LayoutBox {
    let (border, style, display) = match node.shape {
        NodeShape::Rounded => (
            BorderStyle::Rounded,
            ContentStyle::NodeLabel,
            node.label.clone(),
        ),
        NodeShape::Diamond => (
            BorderStyle::Solid,
            ContentStyle::DiamondLabel,
            format!("< {} >", node.label),
        ),
        NodeShape::Circle => (
            BorderStyle::Solid,
            ContentStyle::CircleLabel,
            format!("(( {} ))", node.label),
        ),
        _ => (
            BorderStyle::Solid,
            ContentStyle::NodeLabel,
            node.label.clone(),
        ),
    };

    let text_w = display.chars().count();
    let box_w = text_w + NODE_PAD;

    LayoutBox {
        border: Some(border),
        label: None,
        content: Some(BoxContent {
            text: display,
            style,
        }),
        padding: Padding::zero(),
        children: Vec::new(),
        width: box_w,
        height: NODE_HEIGHT,
    }
}

/// Generic item for the layout algorithm
struct Item {
    width: usize,
    height: usize,
}

struct ItemEdge {
    from: usize,
    to: usize,
    has_label: bool,
}

/// Assign layers, order, and position a set of items connected by edges.
/// Returns Vec<(item_index, x, y)>.
fn layout_items(
    items: &[Item],
    edges: &[ItemEdge],
    is_horizontal: bool,
    gap_y: usize,
) -> Vec<(usize, usize, usize)> {
    let n = items.len();
    if n == 0 {
        return Vec::new();
    }

    let layers = assign_layers(n, edges);

    // Order within layers (barycenter heuristic)
    let max_layer = layers.iter().copied().max().unwrap_or(0);
    let mut layer_nodes: Vec<Vec<usize>> = vec![Vec::new(); max_layer + 1];
    for (idx, &l) in layers.iter().enumerate() {
        layer_nodes[l].push(idx);
    }

    for _ in 0..2 {
        for l in 1..layer_nodes.len() {
            let prev_positions: HashMap<usize, usize> = layer_nodes[l - 1]
                .iter()
                .enumerate()
                .map(|(pos, &idx)| (idx, pos))
                .collect();

            let mut bary: Vec<(usize, f64)> = layer_nodes[l]
                .iter()
                .map(|&node_idx| {
                    let mut sum = 0.0;
                    let mut count = 0;
                    for edge in edges {
                        let from = edge.from;
                        let to = edge.to;
                        if to == node_idx {
                            if let Some(&pos) = prev_positions.get(&from) {
                                sum += pos as f64;
                                count += 1;
                            }
                        }
                    }
                    let bc = if count > 0 {
                        sum / count as f64
                    } else {
                        node_idx as f64
                    };
                    (node_idx, bc)
                })
                .collect();

            bary.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());
            layer_nodes[l] = bary.into_iter().map(|(idx, _)| idx).collect();
        }
    }

    // Position items
    let mut result: Vec<(usize, usize, usize)> = Vec::new();

    if is_horizontal {
        let mut x = 0;
        for layer in &layer_nodes {
            if layer.is_empty() {
                continue;
            }
            let max_w = layer.iter().map(|&i| items[i].width).max().unwrap_or(0);
            let mut y = 0;
            for &idx in layer {
                let cx = x + (max_w - items[idx].width) / 2;
                result.push((idx, cx, y));
                y += items[idx].height + gap_y;
            }
            x += max_w + GAP_X;
        }
    } else {
        let mut y = 0;
        let layer_widths: Vec<usize> = layer_nodes
            .iter()
            .map(|layer| layer_width(layer, items))
            .collect();
        let canvas_w = layer_widths.iter().copied().max().unwrap_or(0);

        for (layer_idx, layer) in layer_nodes.iter().enumerate() {
            if layer.is_empty() {
                continue;
            }
            let max_h = layer
                .iter()
                .map(|&i| items[i].height)
                .max()
                .unwrap_or(NODE_HEIGHT);
            let mut x = canvas_w.saturating_sub(layer_widths[layer_idx]) / 2;
            for &idx in layer {
                result.push((idx, x, y));
                x += items[idx].width + GAP_X;
            }
            let labeled_lanes = if layer_idx + 1 < layer_nodes.len() {
                labeled_edges_to_layer(edges, &layers, layer_idx + 1)
            } else {
                0
            };
            let dynamic_gap = if labeled_lanes > 1 {
                gap_y.max(labeled_lanes * 3)
            } else {
                gap_y
            };
            y += max_h + dynamic_gap;
        }
    }

    result
}

fn assign_layers(n: usize, edges: &[ItemEdge]) -> Vec<usize> {
    // Longest-path layering. Iterates until no layer changes, guaranteeing
    // correct layers even when a node's layer increases after its successors
    // were processed.
    let mut layers = vec![0usize; n];
    let mut iter = 0;
    loop {
        let mut changed = false;
        for edge in edges {
            if edge.from < n && edge.to < n {
                let new_layer = layers[edge.from] + 1;
                if new_layer > layers[edge.to] {
                    layers[edge.to] = new_layer;
                    changed = true;
                }
            }
        }
        iter += 1;
        if !changed || iter > n * n {
            break;
        }
    }
    layers
}

fn layer_width(layer: &[usize], items: &[Item]) -> usize {
    let item_width: usize = layer.iter().map(|&idx| items[idx].width).sum();
    let gap_width = layer.len().saturating_sub(1) * GAP_X;
    item_width + gap_width
}

fn labeled_edges_to_layer(edges: &[ItemEdge], layers: &[usize], layer: usize) -> usize {
    edges
        .iter()
        .filter(|edge| edge.has_label && layers.get(edge.to).copied() == Some(layer))
        .count()
}

// ---------------------------------------------------------------------------
// Subgraph internal layout
// ---------------------------------------------------------------------------

fn layout_subgraph(
    graph: &FlowGraph,
    sg_idx: usize,
    _node_to_sg: &HashMap<usize, usize>,
    is_horizontal: bool,
) -> LayoutBox {
    let sg = &graph.subgraphs[sg_idx];

    // Gather internal node indices
    let internal_idxs: Vec<usize> = sg
        .node_ids
        .iter()
        .filter_map(|id| graph.node_index(id))
        .collect();

    // Build items
    let items: Vec<Item> = internal_idxs
        .iter()
        .map(|&idx| {
            let b = make_node_box(&graph.nodes[idx]);
            Item {
                width: b.width,
                height: b.height,
            }
        })
        .collect();

    // Internal edges (both endpoints inside this subgraph)
    let idx_set: HashMap<usize, usize> = internal_idxs
        .iter()
        .enumerate()
        .map(|(local, &global)| (global, local))
        .collect();

    let edges: Vec<ItemEdge> = graph
        .edges
        .iter()
        .filter_map(|e| {
            let fi = graph.node_index(&e.from)?;
            let ti = graph.node_index(&e.to)?;
            let lfi = idx_set.get(&fi)?;
            let lti = idx_set.get(&ti)?;
            Some(ItemEdge {
                from: *lfi,
                to: *lti,
                has_label: e.label.is_some(),
            })
        })
        .collect();

    let positions = layout_items(&items, &edges, is_horizontal, 2);

    // Build children
    let children: Vec<PlacedChild> = positions
        .iter()
        .map(|&(local_idx, x, y)| {
            let global_idx = internal_idxs[local_idx];
            PlacedChild {
                x,
                y,
                layout_box: make_node_box(&graph.nodes[global_idx]),
            }
        })
        .collect();

    let inner_w = children
        .iter()
        .map(|c| c.x + c.layout_box.width)
        .max()
        .unwrap_or(0);
    let inner_h = children
        .iter()
        .map(|c| c.y + c.layout_box.height)
        .max()
        .unwrap_or(0);

    let label_w = sg.label.chars().count() + 4;
    let border = 2; // top + bottom border
    let total_w = (inner_w + 2 * SG_PAD + border).max(label_w);
    let total_h = inner_h + SG_HEADER + 2 * SG_PAD + border;

    LayoutBox {
        border: Some(BorderStyle::Dashed),
        label: Some(sg.label.clone()),
        content: None,
        padding: Padding {
            top: SG_HEADER + SG_PAD,
            left: SG_PAD,
        },
        children,
        width: total_w,
        height: total_h,
    }
}

// ---------------------------------------------------------------------------
// Outer layout (regular nodes + compound subgraph boxes)
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
enum OuterKey {
    Node(usize),
    Subgraph(usize),
}

fn layout_outer(
    graph: &FlowGraph,
    node_to_sg: &HashMap<usize, usize>,
    mut sg_boxes: Vec<LayoutBox>,
    is_horizontal: bool,
) -> LayoutBox {
    // Build outer items
    let mut outer_keys: Vec<OuterKey> = Vec::new();
    let mut outer_items: Vec<Item> = Vec::new();

    // Maps from graph-space to outer-item-space
    let mut node_to_outer: HashMap<usize, usize> = HashMap::new();
    let mut sg_to_outer: HashMap<usize, usize> = HashMap::new();

    // Regular nodes (not in any subgraph)
    for (idx, node) in graph.nodes.iter().enumerate() {
        if node_to_sg.contains_key(&idx) {
            continue;
        }
        let b = make_node_box(node);
        let outer_idx = outer_items.len();
        outer_items.push(Item {
            width: b.width,
            height: b.height,
        });
        outer_keys.push(OuterKey::Node(idx));
        node_to_outer.insert(idx, outer_idx);
    }

    // Compound subgraph nodes
    for (sg_idx, sg_box) in sg_boxes.iter().enumerate() {
        let outer_idx = outer_items.len();
        outer_items.push(Item {
            width: sg_box.width,
            height: sg_box.height,
        });
        outer_keys.push(OuterKey::Subgraph(sg_idx));
        sg_to_outer.insert(sg_idx, outer_idx);
    }

    // Build outer edges
    let mut outer_edges: Vec<ItemEdge> = Vec::new();
    let mut seen_edges: HashMap<(usize, usize), usize> = HashMap::new();

    for edge in &graph.edges {
        let from_global = match graph.node_index(&edge.from) {
            Some(idx) => idx,
            None => continue,
        };
        let to_global = match graph.node_index(&edge.to) {
            Some(idx) => idx,
            None => continue,
        };

        let from_outer = if let Some(&sg_idx) = node_to_sg.get(&from_global) {
            sg_to_outer.get(&sg_idx).copied()
        } else {
            node_to_outer.get(&from_global).copied()
        };

        let to_outer = if let Some(&sg_idx) = node_to_sg.get(&to_global) {
            sg_to_outer.get(&sg_idx).copied()
        } else {
            node_to_outer.get(&to_global).copied()
        };

        if let (Some(f), Some(t)) = (from_outer, to_outer) {
            if f != t {
                if let Some(&edge_idx) = seen_edges.get(&(f, t)) {
                    outer_edges[edge_idx].has_label |= edge.label.is_some();
                } else {
                    let edge_idx = outer_edges.len();
                    outer_edges.push(ItemEdge {
                        from: f,
                        to: t,
                        has_label: edge.label.is_some(),
                    });
                    seen_edges.insert((f, t), edge_idx);
                }
            }
        }
    }

    // Layout outer items
    let positions = layout_items(&outer_items, &outer_edges, is_horizontal, GAP_Y);

    // Build root children
    let mut root_children: Vec<PlacedChild> = Vec::new();

    for &(item_idx, x, y) in &positions {
        match outer_keys[item_idx] {
            OuterKey::Node(node_idx) => {
                root_children.push(PlacedChild {
                    x,
                    y,
                    layout_box: make_node_box(&graph.nodes[node_idx]),
                });
            }
            OuterKey::Subgraph(sg_idx) => {
                // Take the pre-computed subgraph box (replace with empty placeholder)
                let sg_box = std::mem::replace(
                    &mut sg_boxes[sg_idx],
                    LayoutBox {
                        border: None,
                        label: None,
                        content: None,
                        padding: Padding::zero(),
                        children: Vec::new(),
                        width: 0,
                        height: 0,
                    },
                );
                root_children.push(PlacedChild {
                    x,
                    y,
                    layout_box: sg_box,
                });
            }
        }
    }

    let total_w = root_children
        .iter()
        .map(|c| c.x + c.layout_box.width)
        .max()
        .unwrap_or(0);
    let total_h = root_children
        .iter()
        .map(|c| c.y + c.layout_box.height)
        .max()
        .unwrap_or(0);

    LayoutBox {
        border: None,
        label: None,
        content: None,
        padding: Padding::zero(),
        children: root_children,
        width: total_w,
        height: total_h,
    }
}

// ---------------------------------------------------------------------------
// Absolute position collection (for edge routing)
// ---------------------------------------------------------------------------

fn collect_absolute_positions(
    layout_box: &LayoutBox,
    abs_x: usize,
    abs_y: usize,
) -> HashMap<String, (usize, usize, usize, usize)> {
    let mut result = HashMap::new();

    if let Some(ref content) = layout_box.content {
        result.insert(
            content.text.clone(),
            (abs_x, abs_y, layout_box.width, layout_box.height),
        );
    }

    let border_offset = if layout_box.border.is_some() { 1 } else { 0 };
    let inner_x = abs_x + border_offset + layout_box.padding.left;
    let inner_y = abs_y + border_offset + layout_box.padding.top;

    for child in &layout_box.children {
        let child_pos =
            collect_absolute_positions(&child.layout_box, inner_x + child.x, inner_y + child.y);
        result.extend(child_pos);
    }

    result
}

// ---------------------------------------------------------------------------
// Edge routing
// ---------------------------------------------------------------------------

fn route_all_edges(
    graph: &FlowGraph,
    positions: &HashMap<String, (usize, usize, usize, usize)>,
    is_horizontal: bool,
) -> Vec<LayoutEdge> {
    let id_to_label: HashMap<&str, String> = graph
        .nodes
        .iter()
        .map(|n| {
            let display = match n.shape {
                NodeShape::Diamond => format!("< {} >", n.label),
                NodeShape::Circle => format!("(( {} ))", n.label),
                _ => n.label.clone(),
            };
            (n.id.as_str(), display)
        })
        .collect();

    let mut edges: Vec<LayoutEdge> = graph
        .edges
        .iter()
        .filter_map(|edge| {
            let from_label = id_to_label.get(edge.from.as_str())?;
            let to_label = id_to_label.get(edge.to.as_str())?;
            let &(fx, fy, fw, fh) = positions.get(from_label)?;
            let &(tx, ty, tw, th) = positions.get(to_label)?;

            let (from_x, from_y, to_x, to_y) = if is_horizontal {
                (fx + fw + 1, fy + fh / 2, tx, ty + th / 2)
            } else {
                (fx + fw / 2, fy + fh, tx + tw / 2, ty)
            };

            Some(LayoutEdge {
                from_x,
                from_y,
                to_x,
                to_y,
                style: edge.style,
                label: edge.label.clone(),
                lane: 0,
            })
        })
        .collect();

    if !is_horizontal {
        assign_top_down_lanes(&mut edges);
    }

    edges
}

fn assign_top_down_lanes(edges: &mut [LayoutEdge]) {
    let mut target_row_groups: HashMap<usize, Vec<usize>> = HashMap::new();
    for (idx, edge) in edges.iter().enumerate() {
        if edge.label.is_some() {
            target_row_groups.entry(edge.to_y).or_default().push(idx);
        }
    }

    for group in target_row_groups.values_mut() {
        group.sort_by_key(|&idx| {
            let edge = &edges[idx];
            let label_len = edge
                .label
                .as_ref()
                .map(|label| label.chars().count())
                .unwrap_or(0);
            (
                edge.from_x.min(edge.to_x),
                edge.from_x.abs_diff(edge.to_x),
                label_len,
            )
        });

        for (lane, &edge_idx) in group.iter().enumerate() {
            edges[edge_idx].lane = lane;
        }
    }
}
