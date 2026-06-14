//! Top-down layout as space negotiation.
//!
//! Every visual element — box, edge run, label, arrowhead — is allocated cells
//! it owns before anything is painted. Ranks become bands of rows; between
//! consecutive bands sits a routing channel whose height is derived from the
//! lanes and labels that must pass through it. Edges attach to per-edge ports.
//! Long edges travel through invisible waypoints in intermediate ranks.
//!
//! The output is a [`Scene`]: a fully determined set of paint operations.
//! Paint cannot fail; everything that can go wrong goes wrong here, loudly.

use std::collections::{HashMap, HashSet};

use super::doc::{DiagramError, EdgeKind, Model, NodeKind, NoteMark};
use super::grid::{Style, E, N, S, W};

const GAP_X: usize = 4;
const TETHER_LEN: usize = 2;
const RANK_H: usize = 3;
const MARGIN_X: usize = 1;
const MARGIN_Y: usize = 0;

// ---------------------------------------------------------------------------
// Scene: what layout hands to paint
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BorderKind {
    Solid,
    Double,
    Rounded,
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
    pub height: usize,
    pub ops: Vec<Op>,
}

// ---------------------------------------------------------------------------
// Internal structures
// ---------------------------------------------------------------------------

/// One thing occupying horizontal space in a rank row.
enum ItemKind {
    Node(usize),
    Note(usize),
    /// Waypoint for chain `usize` passing through this rank.
    Way(usize),
}

struct Item {
    kind: ItemKind,
    width: usize,
    x: usize, // assigned during placement
}

/// A horizontal slot in a rank row during placement. A node carries its notes
/// so they stay grouped; waypoints float and are slotted by barycenter.
enum Cell {
    Node { node: usize, notes: Vec<usize> },
    Way(usize),
}

/// One endpoint of a channel segment.
#[derive(Clone, Copy, PartialEq)]
enum End {
    Node(usize),
    Way(usize),
}

/// An edge broken into rank-adjacent segments.
struct Chain {
    edge: usize,
    ends: Vec<End>, // node, [way...], node
}

/// A segment crossing one channel.
struct Seg {
    chain: usize,
    from: End,
    to: End,
    sx: usize, // source port x (absolute)
    tx: usize, // target port x (absolute)
    label: Option<String>,
    kind: EdgeKind,
    /// Lane index in the channel, None for straight unlabeled segments.
    lane: Option<usize>,
}

impl Seg {
    fn is_straight(&self) -> bool {
        self.sx == self.tx && self.label.is_none()
    }
    fn dashed(&self) -> bool {
        matches!(self.kind, EdgeKind::Async | EdgeKind::Event)
    }
    fn style(&self) -> Style {
        match self.kind {
            EdgeKind::Event => Style::EdgeLineEvent,
            _ => Style::EdgeLine,
        }
    }
}

struct NodeGeom {
    x: usize,
    w: usize,
    content: String,
    border: BorderKind,
    border_style: Style,
    content_style: Style,
}

/// Where a label ended up.
enum LabelPlace {
    /// Embedded in the horizontal run on the line row.
    Embedded { x: usize },
    /// On a dedicated row above the line row.
    Row { x: usize },
}

// ---------------------------------------------------------------------------
// Entry
// ---------------------------------------------------------------------------

pub fn compute(model: &Model) -> Result<Scene, DiagramError> {
    // -- node display geometry ------------------------------------------------
    let mut geoms: Vec<NodeGeom> = model
        .nodes
        .iter()
        .map(|n| {
            let (content, border, border_style, content_style) = match n.kind {
                NodeKind::Service => (
                    n.label.clone(),
                    BorderKind::Solid,
                    Style::Border,
                    Style::Label,
                ),
                NodeKind::Store => (
                    n.label.clone(),
                    BorderKind::Double,
                    Style::BorderStore,
                    Style::Label,
                ),
                NodeKind::Queue => (
                    n.label.clone(),
                    BorderKind::Solid,
                    Style::BorderQueue,
                    Style::Label,
                ),
                NodeKind::External => (
                    n.label.clone(),
                    BorderKind::Solid,
                    Style::BorderExternal,
                    Style::LabelExternal,
                ),
                NodeKind::Decision => (
                    format!("< {} >", n.label),
                    BorderKind::Solid,
                    Style::Border,
                    Style::LabelDecision,
                ),
            };
            NodeGeom {
                x: 0,
                w: content.chars().count() + 4,
                content,
                border,
                border_style,
                content_style,
            }
        })
        .collect();

    // -- chains and waypoints --------------------------------------------------
    let mut chains: Vec<Chain> = Vec::new();
    let mut ways_in_rank: Vec<Vec<usize>> = vec![Vec::new(); model.ranks.len()];
    let mut way_rank: Vec<usize> = Vec::new(); // way id -> rank
    let mut way_edge: Vec<usize> = Vec::new(); // way id -> edge index
    for (ei, e) in model.edges.iter().enumerate() {
        let (rf, rt) = (model.nodes[e.from].rank, model.nodes[e.to].rank);
        let mut ends = vec![End::Node(e.from)];
        for r in (rf + 1)..rt {
            let wid = way_rank.len();
            way_rank.push(r);
            way_edge.push(ei);
            ways_in_rank[r].push(wid);
            ends.push(End::Way(wid));
        }
        ends.push(End::Node(e.to));
        chains.push(Chain { edge: ei, ends });
    }

    // -- port demand widens narrow boxes --------------------------------------
    let mut out_count = vec![0usize; model.nodes.len()];
    let mut in_count = vec![0usize; model.nodes.len()];
    for e in &model.edges {
        out_count[e.from] += 1;
        in_count[e.to] += 1;
    }
    for (i, g) in geoms.iter_mut().enumerate() {
        let need = out_count[i].max(in_count[i]);
        if need > 1 {
            g.w = g.w.max(2 * need + 3);
        }
    }

    // -- space negotiation -----------------------------------------------------
    // Place items, route channels, and fit labels. When a label has nowhere to
    // go, the answer is more horizontal room: widen the gaps and re-place.
    // Truncation is never the answer.
    let mut notes_of: HashMap<usize, Vec<usize>> = HashMap::new();
    for (ni, note) in model.notes.iter().enumerate() {
        notes_of.entry(note.on).or_default().push(ni);
    }

    const MAX_ATTEMPTS: usize = 8;
    let mut placed: Option<Placement> = None;
    let mut last_err: Option<DiagramError> = None;
    for attempt in 0..MAX_ATTEMPTS {
        let gap = GAP_X + attempt * 4;
        let extra_canvas = attempt * 8;
        match place_and_route(
            model,
            &mut geoms,
            &chains,
            &ways_in_rank,
            &notes_of,
            gap,
            extra_canvas,
        ) {
            Ok(p) => {
                placed = Some(p);
                break;
            }
            Err(e @ DiagramError::Routing(_)) => last_err = Some(e),
            Err(e) => return Err(e),
        }
    }
    let Placement {
        rank_items,
        canvas_w,
        channels,
        lane_heights,
        label_places,
    } = match placed {
        Some(p) => p,
        None => return Err(last_err.expect("retry loop always records its error")),
    };

    // -- vertical placement ---------------------------------------------------
    let n_channels = model.ranks.len().saturating_sub(1);
    let mut y = MARGIN_Y;
    let title_rows = if model.title.is_some() { 2 } else { 0 };
    y += title_rows;
    let mut rank_y: Vec<usize> = Vec::new();
    let mut channel_y: Vec<usize> = Vec::new(); // top row of each channel
    let mut channel_h: Vec<usize> = Vec::new();
    for r in 0..model.ranks.len() {
        rank_y.push(y);
        y += RANK_H;
        if r < n_channels {
            channel_y.push(y);
            let lanes_h: usize = lane_heights[r].iter().sum();
            // stub row + lanes + arrow row; min 2 keeps a visible gap.
            let h = (1 + lanes_h + 1).max(2);
            channel_h.push(h);
            y += h;
        }
    }
    let height = y + 1;
    let width = canvas_w + 2 * MARGIN_X;

    // -- emit ---------------------------------------------------------------------
    let mut ops: Vec<Op> = Vec::new();

    if let Some(title) = &model.title {
        ops.push(Op::Text {
            x: MARGIN_X,
            y: MARGIN_Y,
            text: title.clone(),
            style: Style::Title,
        });
    }

    for (r, items) in rank_items.iter().enumerate() {
        let by = rank_y[r];
        for item in items {
            match item.kind {
                ItemKind::Node(n) => {
                    let g = &geoms[n];
                    ops.push(Op::Box {
                        x: g.x,
                        y: by,
                        w: g.w,
                        h: RANK_H,
                        border: g.border,
                        border_style: g.border_style,
                        content: g.content.clone(),
                        content_style: g.content_style,
                    });
                }
                ItemKind::Note(ni) => {
                    let note = &model.notes[ni];
                    let (content, style) = match note.mark {
                        NoteMark::Uncertain => {
                            (format!("? {}", note.text), Style::LabelUncertain)
                        }
                        NoteMark::Info => (note.text.clone(), Style::LabelNote),
                    };
                    ops.push(Op::Box {
                        x: item.x,
                        y: by,
                        w: item.width,
                        h: RANK_H,
                        border: BorderKind::Rounded,
                        border_style: Style::BorderNote,
                        content,
                        content_style: style,
                    });
                    // Tether from anchor to note at mid height.
                    let cells: Vec<(usize, usize, u8)> = (item.x - TETHER_LEN..item.x)
                        .map(|x| (x, by + 1, E | W))
                        .collect();
                    ops.push(Op::Stroke {
                        cells,
                        dashed: true,
                        style: Style::Tether,
                    });
                }
                ItemKind::Way(wid) => {
                    // Pass-through vertical across the rank band, styled like
                    // the edge it belongs to.
                    let kind = model.edges[way_edge[wid]].kind;
                    let cells: Vec<(usize, usize, u8)> =
                        (by..by + RANK_H).map(|yy| (item.x, yy, N | S)).collect();
                    ops.push(Op::Stroke {
                        cells,
                        dashed: matches!(kind, EdgeKind::Async | EdgeKind::Event),
                        style: if kind == EdgeKind::Event {
                            Style::EdgeLineEvent
                        } else {
                            Style::EdgeLine
                        },
                    });
                }
            }
        }
    }

    for (cidx, segs) in channels.iter().enumerate() {
        let top = channel_y[cidx];
        let arrow_row = top + channel_h[cidx] - 1;
        // Precompute line rows per lane.
        let mut lane_line_row: Vec<usize> = Vec::new();
        let mut row = top + 1; // row `top` is the stub row
        for &h in &lane_heights[cidx] {
            lane_line_row.push(row + h - 1); // label row (if any) sits above
            row += h;
        }

        for (si, seg) in segs.iter().enumerate() {
            let dashed = seg.dashed();
            let style = seg.style();
            let mut cells: Vec<(usize, usize, u8)> = Vec::new();

            // Rows the target-side vertical must reach.
            let target_is_way = matches!(seg.to, End::Way(_));
            let v_end = if target_is_way { arrow_row } else { arrow_row - 1 };

            match seg.lane {
                None => {
                    // Straight unlabeled: one column, top..v_end.
                    for yy in top..=v_end {
                        cells.push((seg.sx, yy, N | S));
                    }
                }
                Some(lane) => {
                    let line = lane_line_row[lane];
                    if seg.sx == seg.tx {
                        // Labeled straight: still one column.
                        for yy in top..=v_end {
                            cells.push((seg.sx, yy, N | S));
                        }
                    } else {
                        for yy in top..line {
                            cells.push((seg.sx, yy, N | S));
                        }
                        let (lo, hi) = (seg.sx.min(seg.tx), seg.sx.max(seg.tx));
                        let going_right = seg.tx > seg.sx;
                        cells.push((seg.sx, line, N | if going_right { E } else { W }));
                        // Horizontal run, skipping embedded label cells.
                        let skip = match label_places[cidx].get(&si) {
                            Some(LabelPlace::Embedded { x }) => {
                                let len = seg.label.as_ref().unwrap().chars().count();
                                Some((*x, *x + len))
                            }
                            _ => None,
                        };
                        for x in (lo + 1)..hi {
                            if let Some((sx0, sx1)) = skip {
                                if x >= sx0 && x < sx1 {
                                    continue;
                                }
                            }
                            cells.push((x, line, E | W));
                        }
                        cells.push((seg.tx, line, S | if going_right { W } else { E }));
                        for yy in (line + 1)..=v_end {
                            cells.push((seg.tx, yy, N | S));
                        }
                    }
                }
            }

            ops.push(Op::Stroke {
                cells,
                dashed,
                style,
            });

            if !target_is_way {
                ops.push(Op::Arrow {
                    x: seg.tx,
                    y: arrow_row,
                });
            }

            if let Some(label) = &seg.label {
                let lane = seg.lane.expect("labeled segment always has a lane");
                let (x, y) = match label_places[cidx][&si] {
                    LabelPlace::Embedded { x } => (x, lane_line_row[lane]),
                    LabelPlace::Row { x } => (x, lane_line_row[lane] - 1),
                };
                ops.push(Op::Text {
                    x,
                    y,
                    text: label.clone(),
                    style: Style::EdgeLabel,
                });
            }
        }
    }

    Ok(Scene {
        width,
        height,
        ops,
    })
}

// ---------------------------------------------------------------------------
// Placement: one negotiation attempt at a given gap width
// ---------------------------------------------------------------------------

struct Placement {
    rank_items: Vec<Vec<Item>>,
    canvas_w: usize,
    channels: Vec<Vec<Seg>>,
    /// lane_heights[channel][lane] = 1, or 2 when the lane carries a label row.
    lane_heights: Vec<Vec<usize>>,
    label_places: Vec<HashMap<usize, LabelPlace>>,
}

fn place_and_route(
    model: &Model,
    geoms: &mut [NodeGeom],
    chains: &[Chain],
    ways_in_rank: &[Vec<usize>],
    notes_of: &HashMap<usize, Vec<usize>>,
    gap: usize,
    extra_canvas: usize,
) -> Result<Placement, DiagramError> {
    let note_w: Vec<usize> = model
        .notes
        .iter()
        .map(|n| {
            let prefix = if n.mark == NoteMark::Uncertain { 2 } else { 0 };
            n.text.chars().count() + prefix + 4
        })
        .collect();

    let way_count: usize = ways_in_rank.iter().map(|w| w.len()).sum();

    // For each waypoint: the chain's true end nodes and where it sits between
    // them (i of l). It interpolates along the straight source->target line
    // rather than chasing its siblings (which start equally adrift, so a
    // neighbour-barycentre would just reinforce the detour).
    let mut way_anchor: Vec<(usize, usize, usize, usize)> = vec![(0, 0, 0, 1); way_count];
    for chain in chains {
        let l = chain.ends.len();
        let (End::Node(s), End::Node(t)) = (chain.ends[0], chain.ends[l - 1]) else {
            continue;
        };
        for (i, end) in chain.ends.iter().enumerate() {
            if let End::Way(w) = *end {
                way_anchor[w] = (s, t, i, l);
            }
        }
    }

    // Rank rows as ordered cells: a node carries its notes; waypoints float.
    let mut orders: Vec<Vec<Cell>> = model
        .ranks
        .iter()
        .enumerate()
        .map(|(r, row)| {
            let mut cells: Vec<Cell> = row
                .iter()
                .map(|&node| Cell::Node {
                    node,
                    notes: notes_of.get(&node).cloned().unwrap_or_default(),
                })
                .collect();
            cells.extend(ways_in_rank[r].iter().map(|&w| Cell::Way(w)));
            cells
        })
        .collect();

    let mut way_x = vec![0usize; way_count];
    let mut note_x = vec![0usize; model.notes.len()];

    // extra_canvas grows on retry so labels in narrow diagrams get room.
    let mut canvas_w = place_rows(&orders, geoms, &mut way_x, &mut note_x, &note_w, gap, extra_canvas);

    // Relax: slot each waypoint toward its interpolated target so a long edge
    // drops through the interior instead of detouring around the edge. Sorting
    // by node *centres* lets a waypoint claim the gap between two boxes; node
    // centres stay monotonic in author order, so only waypoints actually move.
    let center = |node: usize, geoms: &[NodeGeom]| -> usize {
        geoms[node].x + geoms[node].w / 2
    };
    for _ in 0..4 {
        for cells in &mut orders {
            cells.sort_by_key(|c| match *c {
                Cell::Node { node, .. } => center(node, geoms),
                Cell::Way(w) => {
                    let (s, t, i, l) = way_anchor[w];
                    (center(s, geoms) * (l - 1 - i) + center(t, geoms) * i) / (l - 1)
                }
            });
        }
        canvas_w = place_rows(&orders, geoms, &mut way_x, &mut note_x, &note_w, gap, extra_canvas);
    }

    // Flatten to the item list the emit stage walks.
    let rank_items: Vec<Vec<Item>> = orders
        .iter()
        .map(|cells| {
            let mut items = Vec::new();
            for c in cells {
                match c {
                    Cell::Node { node, notes } => {
                        items.push(Item {
                            width: geoms[*node].w,
                            kind: ItemKind::Node(*node),
                            x: geoms[*node].x,
                        });
                        for &ni in notes {
                            items.push(Item {
                                width: note_w[ni],
                                kind: ItemKind::Note(ni),
                                x: note_x[ni],
                            });
                        }
                    }
                    Cell::Way(w) => items.push(Item {
                        width: 1,
                        kind: ItemKind::Way(*w),
                        x: way_x[*w],
                    }),
                }
            }
            items
        })
        .collect();

    // -- segments per channel --------------------------------------------------
    let n_channels = model.ranks.len().saturating_sub(1);
    let mut channels: Vec<Vec<Seg>> = (0..n_channels).map(|_| Vec::new()).collect();
    for (ci, chain) in chains.iter().enumerate() {
        let e = &model.edges[chain.edge];
        let base_rank = model.nodes[e.from].rank;
        for pos in 0..chain.ends.len() - 1 {
            channels[base_rank + pos].push(Seg {
                chain: ci,
                from: chain.ends[pos],
                to: chain.ends[pos + 1],
                sx: 0,
                tx: 0,
                label: if pos == 0 { e.label.clone() } else { None },
                kind: e.kind,
                lane: None,
            });
        }
    }

    // -- ports -------------------------------------------------------------------
    let end_center = |end: End| -> usize {
        match end {
            End::Node(n) => geoms[n].x + geoms[n].w / 2,
            End::Way(w) => way_x[w],
        }
    };
    for segs in &mut channels {
        assign_ports(segs, geoms, &way_x, end_center);
        nudge_conflicts(segs, geoms, &model.nodes, &way_x)?;
        assign_lanes(segs, model)?;
    }

    // -- labels + channel heights --------------------------------------------------
    let mut lane_heights: Vec<Vec<usize>> = Vec::new();
    let mut label_places: Vec<HashMap<usize, LabelPlace>> = Vec::new();
    for segs in &channels {
        let lane_count = segs
            .iter()
            .filter_map(|s| s.lane)
            .map(|l| l + 1)
            .max()
            .unwrap_or(0);
        let mut heights = vec![1usize; lane_count];
        let mut places: HashMap<usize, LabelPlace> = HashMap::new();
        for (si, seg) in segs.iter().enumerate() {
            let Some(label) = &seg.label else { continue };
            let lane = seg.lane.expect("labeled segment always has a lane");
            let len = label.chars().count();
            let lo = seg.sx.min(seg.tx);
            let hi = seg.sx.max(seg.tx);

            // Try embedding in the horizontal run first.
            let line_avoid = avoid_columns(segs, lane, false);
            let run_inner = (lo + 1, hi.saturating_sub(len + 1).max(lo + 1));
            let embedded = (hi > lo + len + 3)
                .then(|| {
                    let mid = (lo + hi) / 2;
                    pick_label_x(
                        mid.saturating_sub(len / 2),
                        run_inner.0,
                        run_inner.1,
                        len,
                        &line_avoid,
                    )
                })
                .flatten();
            if let Some(x) = embedded {
                places.insert(si, LabelPlace::Embedded { x });
                continue;
            }

            // Dedicated label row above the line row.
            let row_avoid = avoid_columns(segs, lane, true);
            let mid = (seg.sx + seg.tx) / 2;
            let x = pick_label_x(
                mid.saturating_sub(len / 2),
                MARGIN_X,
                (MARGIN_X + canvas_w).saturating_sub(len),
                len,
                &row_avoid,
            )
            .ok_or_else(|| DiagramError::Routing(format!("no room for edge label `{label}`")))?;
            heights[lane] = 2;
            places.insert(si, LabelPlace::Row { x });
        }
        lane_heights.push(heights);
        label_places.push(places);
    }

    Ok(Placement {
        rank_items,
        canvas_w,
        channels,
        lane_heights,
        label_places,
    })
}

/// Assign x to every node, note, and waypoint for the given cell ordering.
/// Rows are centered against the widest row (plus `extra_canvas`). Returns the
/// canvas width. Pure aside from writing the position slices.
fn place_rows(
    orders: &[Vec<Cell>],
    geoms: &mut [NodeGeom],
    way_x: &mut [usize],
    note_x: &mut [usize],
    note_w: &[usize],
    gap: usize,
    extra_canvas: usize,
) -> usize {
    let row_width = |cells: &[Cell], geoms: &[NodeGeom]| -> usize {
        let mut w = 0;
        for (i, c) in cells.iter().enumerate() {
            if i > 0 {
                w += gap;
            }
            match c {
                Cell::Node { node, notes } => {
                    w += geoms[*node].w;
                    for &ni in notes {
                        w += TETHER_LEN + note_w[ni];
                    }
                }
                Cell::Way(_) => w += 1,
            }
        }
        w
    };

    let canvas_w = orders
        .iter()
        .map(|cells| row_width(cells, geoms))
        .max()
        .unwrap_or(0)
        + extra_canvas;

    for cells in orders {
        let used = row_width(cells, geoms);
        let mut x = MARGIN_X + (canvas_w - used) / 2;
        for (i, c) in cells.iter().enumerate() {
            if i > 0 {
                x += gap;
            }
            match c {
                Cell::Node { node, notes } => {
                    geoms[*node].x = x;
                    x += geoms[*node].w;
                    for &ni in notes {
                        x += TETHER_LEN;
                        note_x[ni] = x;
                        x += note_w[ni];
                    }
                }
                Cell::Way(w) => {
                    way_x[*w] = x;
                    x += 1;
                }
            }
        }
    }
    canvas_w
}

// ---------------------------------------------------------------------------
// Ports
// ---------------------------------------------------------------------------

fn assign_ports(
    segs: &mut [Seg],
    geoms: &[NodeGeom],
    way_x: &[usize],
    end_center: impl Fn(End) -> usize,
) {
    // Group outgoing segments per source node, sorted by where they're headed.
    let mut by_source: HashMap<usize, Vec<usize>> = HashMap::new();
    let mut by_target: HashMap<usize, Vec<usize>> = HashMap::new();
    for (i, seg) in segs.iter().enumerate() {
        match seg.from {
            End::Node(n) => by_source.entry(n).or_default().push(i),
            End::Way(w) => {
                // waypoint: fixed column
                let _ = w;
            }
        }
        if let End::Node(n) = seg.to {
            by_target.entry(n).or_default().push(i);
        }
    }

    for (i, seg) in segs.iter_mut().enumerate() {
        let _ = i;
        if let End::Way(w) = seg.from {
            seg.sx = way_x[w];
        }
        if let End::Way(w) = seg.to {
            seg.tx = way_x[w];
        }
    }

    let spread = |g: &NodeGeom, k: usize, slot: usize| -> usize {
        // Interior cells are [x+1, x+w-2]; distribute k ports evenly.
        let interior = g.w - 2;
        g.x + 1 + (interior * (slot + 1)) / (k + 1)
    };

    for (node, mut list) in by_source {
        list.sort_by_key(|&i| (end_center(segs[i].to), segs[i].chain));
        let k = list.len();
        for (slot, &i) in list.iter().enumerate() {
            segs[i].sx = spread(&geoms[node], k, slot);
        }
    }
    for (node, mut list) in by_target {
        list.sort_by_key(|&i| (end_center(segs[i].from), segs[i].chain));
        let k = list.len();
        for (slot, &i) in list.iter().enumerate() {
            segs[i].tx = spread(&geoms[node], k, slot);
        }
    }
}

/// Straight segments own their column for the full channel height; no other
/// segment's vertical may share that x. Nudge ports until clean or fail loud.
fn nudge_conflicts(
    segs: &mut [Seg],
    geoms: &[NodeGeom],
    nodes: &[super::doc::ModelNode],
    _way_x: &[usize],
) -> Result<(), DiagramError> {
    for _round in 0..16 {
        let straight_cols: HashSet<usize> = segs
            .iter()
            .filter(|s| s.is_straight())
            .map(|s| s.sx)
            .collect();

        let mut conflict: Option<(usize, bool)> = None; // (seg idx, fix_source)
        for (i, seg) in segs.iter().enumerate() {
            if seg.is_straight() {
                continue;
            }
            if straight_cols.contains(&seg.sx) {
                conflict = Some((i, true));
                break;
            }
            if straight_cols.contains(&seg.tx) {
                conflict = Some((i, false));
                break;
            }
        }

        let Some((i, fix_source)) = conflict else {
            return Ok(());
        };

        let taken: HashSet<usize> = segs
            .iter()
            .enumerate()
            .filter(|&(j, _)| j != i)
            .flat_map(|(_, s)| [s.sx, s.tx])
            .collect();
        let (cur, end) = if fix_source {
            (segs[i].sx, segs[i].from)
        } else {
            (segs[i].tx, segs[i].to)
        };
        let range = match end {
            End::Node(n) => (geoms[n].x + 1, geoms[n].x + geoms[n].w - 2),
            End::Way(_) => (cur, cur), // waypoint columns can't move
        };
        let candidate = (1..=(range.1 - range.0).max(1))
            .flat_map(|d| [cur.checked_sub(d), cur.checked_add(d)])
            .flatten()
            .find(|&x| {
                x >= range.0 && x <= range.1 && !straight_cols.contains(&x) && !taken.contains(&x)
            });
        match candidate {
            Some(x) => {
                if fix_source {
                    segs[i].sx = x;
                } else {
                    segs[i].tx = x;
                }
            }
            None => {
                let id = match end {
                    End::Node(n) => nodes[n].id.clone(),
                    End::Way(_) => "waypoint".to_string(),
                };
                return Err(DiagramError::Routing(format!(
                    "cannot find a free port column at `{id}`"
                )));
            }
        }
    }
    Err(DiagramError::Routing(
        "port conflict resolution did not converge".into(),
    ))
}

/// One segment per lane. Order is a topological sort of the precedence
/// constraint "S descends at the column T rises in" (S.sx == T.tx => S above T),
/// tie-broken by leftmost interval for stable output.
fn assign_lanes(segs: &mut [Seg], model: &Model) -> Result<(), DiagramError> {
    let laned: Vec<usize> = segs
        .iter()
        .enumerate()
        .filter(|(_, s)| !s.is_straight())
        .map(|(i, _)| i)
        .collect();
    let n = laned.len();
    if n == 0 {
        return Ok(());
    }

    // precedence[a] contains b  =>  lane(a) < lane(b)
    let mut succs: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut indeg = vec![0usize; n];
    for (ai, &a) in laned.iter().enumerate() {
        for (bi, &b) in laned.iter().enumerate() {
            if ai != bi && segs[a].sx == segs[b].tx {
                succs[ai].push(bi);
                indeg[bi] += 1;
            }
        }
    }

    let mut ready: Vec<usize> = (0..n).filter(|&i| indeg[i] == 0).collect();
    let mut order: Vec<usize> = Vec::with_capacity(n);
    while !ready.is_empty() {
        // Deterministic: smallest leftmost interval first.
        ready.sort_by_key(|&i| {
            let s = &segs[laned[i]];
            (s.sx.min(s.tx), s.sx.max(s.tx), s.chain)
        });
        let i = ready.remove(0);
        order.push(i);
        for &j in &succs[i].clone() {
            indeg[j] -= 1;
            if indeg[j] == 0 {
                ready.push(j);
            }
        }
    }
    if order.len() != n {
        // A precedence cycle means two segments swap columns; this needs the
        // port nudging to break it — rare enough to refuse loudly for now.
        let ids: Vec<String> = laned
            .iter()
            .map(|&i| {
                let e = &model.edges[segs[i].chain];
                let _ = e;
                format!("segment {}", i)
            })
            .collect();
        return Err(DiagramError::Routing(format!(
            "channel routing cycle between {}",
            ids.join(", ")
        )));
    }

    for (lane, &i) in order.iter().enumerate() {
        segs[laned[i]].lane = Some(lane);
    }
    Ok(())
}

/// Columns a label on lane `lane` must not cover.
/// `label_row`: true for the dedicated row above the line row.
fn avoid_columns(segs: &[Seg], lane: usize, label_row: bool) -> HashSet<usize> {
    let mut cols = HashSet::new();
    for s in segs {
        if s.is_straight() {
            cols.insert(s.sx);
            continue;
        }
        let Some(l) = s.lane else { continue };
        // Source vertical spans from the stub row down to its line row.
        let source_crosses = if label_row { l >= lane } else { l > lane };
        if source_crosses {
            cols.insert(s.sx);
        }
        // Target vertical spans from its line row down to the arrow row.
        if l < lane {
            cols.insert(s.tx);
        }
        // Corner cells on this very lane.
        if !label_row && l == lane {
            cols.insert(s.sx);
            cols.insert(s.tx);
        }
    }
    cols
}

/// Find the closest x to `preferred` within [min_x, max_x] such that the label
/// (plus a one-cell margin each side) avoids the given columns.
fn pick_label_x(
    preferred: usize,
    min_x: usize,
    max_x: usize,
    len: usize,
    avoid: &HashSet<usize>,
) -> Option<usize> {
    if max_x < min_x {
        return None;
    }
    let fits = |x: usize| -> bool {
        let lo = x.saturating_sub(1);
        let hi = x + len; // inclusive margin cell on the right
        !(lo..=hi).any(|c| avoid.contains(&c))
    };
    let preferred = preferred.clamp(min_x, max_x);
    if fits(preferred) {
        return Some(preferred);
    }
    for d in 1..=(max_x - min_x) {
        if preferred >= min_x + d && fits(preferred - d) {
            return Some(preferred - d);
        }
        if preferred + d <= max_x && fits(preferred + d) {
            return Some(preferred + d);
        }
    }
    None
}
