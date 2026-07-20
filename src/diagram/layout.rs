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
/// Sibling branches stop spreading once the diagram has enough whitespace to
/// distinguish their paths without turning one relationship into a wide scan.
const MAX_RESPONSIVE_GAP_X: usize = 20;
const RANK_H: usize = 3;
const MARGIN_X: usize = 1;
/// Minimum text columns a footnote keeps even under a deep indent, so a long
/// node label can't crush its note into a one-word-per-line sliver.
const MIN_TEXT_COLS: usize = 24;
/// Smallest caption width for the footnote block, so footnotes under a tiny
/// diagram still read as a paragraph rather than a narrow ribbon.
const MIN_FOOTNOTE_WIDTH: usize = 56;
const MARGIN_Y: usize = 0;
/// An edge spanning at least this many ranks routes down a margin corridor
/// instead of through the interior, so it can't bisect the dense middle.
/// Shorter spans still interpolate through the interior (which reads fine).
const LONG_SPAN: usize = 4;
/// Columns reserved per corridor lane in a gutter (1 line + 1 gap).
const CORRIDOR_W: usize = 2;

// ---------------------------------------------------------------------------
// Scene: what layout hands to paint
// ---------------------------------------------------------------------------

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
    /// Width of the graph alone (nodes + edges), before footnotes. Footnotes
    /// wrap, so this — not `width` — is what decides whether the diagram fits.
    pub graph_width: usize,
    pub height: usize,
    pub ops: Vec<Op>,
}

// ---------------------------------------------------------------------------
// Internal structures
// ---------------------------------------------------------------------------

/// One thing occupying horizontal space in a rank row.
enum ItemKind {
    Node(usize),
    /// Waypoint for chain `usize` passing through this rank.
    Way(usize),
}

struct Item {
    kind: ItemKind,
    x: usize, // assigned during placement
}

/// A horizontal slot in a rank row during placement. Interior waypoints float
/// and are slotted by barycenter; corridor waypoints live in the gutters.
enum Cell {
    Node(usize),
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
    /// Centered on the source branch immediately above a fan-in bus.
    SourceBranch { x: usize },
    /// Centered on the target branch immediately below a fan-out bus.
    TargetBranch { x: usize },
}

/// A long edge routed down a margin gutter rather than the interior.
struct Corridor {
    ways: Vec<usize>,
    s: usize,
    t: usize,
    kind: EdgeKind,
}

// ---------------------------------------------------------------------------
// Entry
// ---------------------------------------------------------------------------

pub fn compute(model: &Model, viewport: usize) -> Result<Scene, DiagramError> {
    // Notes render as footnotes below the diagram; each annotated node carries
    // a [n] marker keyed to that list, so notes never disturb the graph layout.
    let mut markers_of: HashMap<usize, Vec<usize>> = HashMap::new();
    for (ni, note) in model.notes.iter().enumerate() {
        markers_of.entry(note.on).or_default().push(ni + 1);
    }

    // -- node display geometry ------------------------------------------------
    let mut geoms: Vec<NodeGeom> = model
        .nodes
        .iter()
        .enumerate()
        .map(|(idx, n)| {
            let (mut content, border, border_style, content_style) = match n.kind {
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
            if let Some(nums) = markers_of.get(&idx) {
                let list = nums
                    .iter()
                    .map(usize::to_string)
                    .collect::<Vec<_>>()
                    .join(",");
                content.push_str(&format!(" [{list}]"));
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
    let (node_envelopes, way_envelopes) = branch_label_envelopes(model, &chains, &geoms);

    // -- space negotiation -----------------------------------------------------
    // Place items, route channels, and fit labels. When a label has nowhere to
    // go, the answer is more horizontal room: widen the gaps and re-place.
    // Truncation is never the answer.
    const MAX_ATTEMPTS: usize = 8;
    let base_gap = match viewport {
        0..=48 => 1,
        49..=72 => 2,
        _ => GAP_X,
    };
    let mut placed: Option<Placement> = None;
    let mut last_err: Option<DiagramError> = None;
    for attempt in 0..MAX_ATTEMPTS {
        let gap = base_gap + attempt * 4;
        let extra_canvas = attempt * 8;
        match place_and_route(
            model,
            &mut geoms,
            &chains,
            &ways_in_rank,
            LayoutEnvelope {
                nodes: &node_envelopes,
                ways: &way_envelopes,
                viewport,
            },
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
        target_label_bands,
        corridor_bands,
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
            let target_label_rows = usize::from(target_label_bands[r]);
            // Source stub, routing lanes, optional fan-out label band, then the
            // arrow row immediately above the target rank.
            let h = (1 + lanes_h + target_label_rows + 1).max(2);
            channel_h.push(h);
            y += h;
        }
    }
    let mut height = y + 1;
    let mut width = canvas_w + 2 * MARGIN_X;

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

    // Corridor band pass-throughs: each long edge's vertical across rank bands.
    for &(rank, x, kind) in &corridor_bands {
        let by = rank_y[rank];
        let cells: Vec<(usize, usize, u8)> = (by..by + RANK_H).map(|yy| (x, yy, N | S)).collect();
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

    for (cidx, segs) in channels.iter().enumerate() {
        let top = channel_y[cidx];
        let arrow_row = top + channel_h[cidx] - 1;
        let target_label_row = target_label_bands[cidx].then_some(arrow_row - 1);
        let mut emitted_arrows: HashSet<usize> = HashSet::new();
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
            let v_end = if target_is_way {
                arrow_row
            } else {
                arrow_row - 1
            };

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
                            if label_places[cidx].get(&si).is_some_and(|place| {
                                (matches!(place, LabelPlace::SourceBranch { .. })
                                    && yy == lane_line_row[lane] - 1)
                                    || (matches!(place, LabelPlace::TargetBranch { .. })
                                        && Some(yy) == target_label_row)
                            }) {
                                continue;
                            }
                            cells.push((seg.sx, yy, N | S));
                        }
                    } else {
                        for yy in top..line {
                            if matches!(
                                label_places[cidx].get(&si),
                                Some(LabelPlace::SourceBranch { .. })
                            ) && yy == line - 1
                            {
                                continue;
                            }
                            cells.push((seg.sx, yy, N | S));
                        }
                        let (lo, hi) = (seg.sx.min(seg.tx), seg.sx.max(seg.tx));
                        let going_right = seg.tx > seg.sx;
                        cells.push((seg.sx, line, N | if going_right { E } else { W }));
                        // A bundled lane can carry several branch labels. Every
                        // segment skips every embedded label on the shared bus,
                        // so later strokes cannot paint through earlier text.
                        let skips: Vec<(usize, usize)> = segs
                            .iter()
                            .enumerate()
                            .filter(|(_, other)| other.lane == Some(lane))
                            .filter_map(|(other_index, other)| {
                                let LabelPlace::Embedded { x } =
                                    label_places[cidx].get(&other_index)?
                                else {
                                    return None;
                                };
                                let len = other.label.as_ref()?.chars().count();
                                Some((*x, *x + len))
                            })
                            .collect();
                        for x in (lo + 1)..hi {
                            if skips.iter().any(|&(start, end)| x >= start && x < end) {
                                continue;
                            }
                            cells.push((x, line, E | W));
                        }
                        cells.push((seg.tx, line, S | if going_right { W } else { E }));
                        for yy in (line + 1)..=v_end {
                            if matches!(
                                label_places[cidx].get(&si),
                                Some(LabelPlace::TargetBranch { .. })
                            ) && Some(yy) == target_label_row
                            {
                                continue;
                            }
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

            if !target_is_way && emitted_arrows.insert(seg.tx) {
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
                    LabelPlace::SourceBranch { x } => (x, lane_line_row[lane] - 1),
                    LabelPlace::TargetBranch { x } => (
                        x,
                        target_label_row.expect("target branch label has a channel band"),
                    ),
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

    // The graph's natural width is settled here, before footnotes. Footnotes
    // are wrapped prose: they must never decide whether the graph fits its
    // viewport, so the renderer reports this width separately from the
    // footnote-extended one.
    let graph_width = width;

    // -- footnotes ------------------------------------------------------------
    // One "[n] <node> — <text>" entry per note, below the diagram, as a
    // hanging-indent paragraph: the [n] marker sits in the left gutter, the node
    // name and body open at a fixed column, and every wrapped line aligns under
    // the node name (never under each note's text, which would leave the block's
    // left edge ragged). The block wraps to the graph's own width — a caption
    // beside the diagram, not an edge-to-edge sprawl — capped at the viewport.
    if !model.notes.is_empty() {
        let block_w = graph_width.clamp(MIN_FOOTNOTE_WIDTH.min(viewport.max(1)), viewport.max(1));
        let mut fy = height + 1;
        for (ni, note) in model.notes.iter().enumerate() {
            let label = &model.nodes[note.on].label;
            let (prefix, body_style) = match note.mark {
                NoteMark::Uncertain => ("? ", Style::LabelUncertain),
                NoteMark::Info => ("", Style::LabelNote),
            };
            // Three tiers so a reader maps marker -> node at a glance: the [n]
            // anchor (accent), the node name (bold, as in its box), then the
            // note body (dim, or flagged when uncertain).
            let marker = format!("[{}]", ni + 1);
            let node_x = MARGIN_X + marker.chars().count() + 1;
            let lead = format!("— {prefix}");
            let body_x = node_x + label.chars().count() + 1;
            let first_body_x = body_x + lead.chars().count();

            ops.push(Op::Text {
                x: MARGIN_X,
                y: fy,
                text: marker,
                style: Style::NoteMarker,
            });
            ops.push(Op::Text {
                x: node_x,
                y: fy,
                text: label.clone(),
                style: Style::Label,
            });

            // The first line takes what's left after the node name; wrapped
            // lines (aligned under the node name) take the fuller width, so a
            // long symbol wraps whole instead of being hyphen-split mid-word.
            let first_avail = footnote_text_cols(block_w, viewport, first_body_x);
            let cont_avail = footnote_text_cols(block_w, viewport, node_x);
            for (li, chunk) in wrap_hanging(&note.text, first_avail, cont_avail)
                .iter()
                .enumerate()
            {
                let (cx, text) = if li == 0 {
                    (body_x, format!("{lead}{chunk}"))
                } else {
                    (node_x, chunk.clone())
                };
                width = width.max(cx + text.chars().count() + MARGIN_X);
                ops.push(Op::Text {
                    x: cx,
                    y: fy,
                    text,
                    style: body_style,
                });
                fy += 1;
            }
        }
        height = fy;
    }

    Ok(Scene {
        width,
        graph_width,
        height,
        ops,
    })
}

/// Text columns available to a footnote line starting at `start_x`: aim for the
/// caption `block_w`, never exceed the `viewport` (hard cap), and keep at least
/// `MIN_TEXT_COLS` when there is room so a deep indent still leaves usable space.
fn footnote_text_cols(block_w: usize, viewport: usize, start_x: usize) -> usize {
    let cap = viewport.saturating_sub(start_x + MARGIN_X).max(1);
    let want = block_w.saturating_sub(start_x + MARGIN_X);
    want.max(MIN_TEXT_COLS.min(cap)).min(cap)
}

/// Wrap prose for a hanging-indent paragraph: the first line has `first` columns,
/// every wrapped line after it has `rest`. Words break on whitespace, hard-split
/// only when a single word is wider than its line. Always returns >= 1 line.
fn wrap_hanging(text: &str, first: usize, rest: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut cur_w = 0;
    let avail = |line_idx: usize| {
        if line_idx == 0 {
            first.max(1)
        } else {
            rest.max(1)
        }
    };
    for word in text.split_whitespace() {
        let ww = word.chars().count();
        let w = avail(lines.len());
        if ww > w {
            if !cur.is_empty() {
                lines.push(std::mem::take(&mut cur));
                cur_w = 0;
            }
            let chars: Vec<char> = word.chars().collect();
            let mut i = 0;
            while i < chars.len() {
                let lw = avail(lines.len());
                let end = (i + lw).min(chars.len());
                lines.push(chars[i..end].iter().collect());
                i = end;
            }
            continue;
        }
        let needs = if cur.is_empty() { ww } else { cur_w + 1 + ww };
        if needs > w && !cur.is_empty() {
            lines.push(std::mem::take(&mut cur));
            cur.push_str(word);
            cur_w = ww;
        } else {
            if !cur.is_empty() {
                cur.push(' ');
                cur_w += 1;
            }
            cur.push_str(word);
            cur_w += ww;
        }
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
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
    /// A row below the routing lanes where fan-out labels sit on their target
    /// branches. The row is shared by every fan-out bundle in the channel.
    target_label_bands: Vec<bool>,
    /// Rank-band pass-throughs for corridor (long, margin-routed) edges:
    /// (rank, x, edge kind). Drawn by the emit stage, which knows the y rows.
    corridor_bands: Vec<(usize, usize, EdgeKind)>,
}

struct LayoutEnvelope<'a> {
    nodes: &'a [usize],
    ways: &'a [usize],
    viewport: usize,
}

fn branch_label_envelopes(
    model: &Model,
    chains: &[Chain],
    geoms: &[NodeGeom],
) -> (Vec<usize>, Vec<usize>) {
    let mut node_widths: Vec<usize> = geoms.iter().map(|geom| geom.w).collect();
    let way_count = chains
        .iter()
        .flat_map(|chain| chain.ends.iter())
        .filter_map(|end| match end {
            End::Way(way) => Some(*way + 1),
            End::Node(_) => None,
        })
        .max()
        .unwrap_or(0);
    let mut way_widths = vec![1usize; way_count];

    for chain in chains {
        let edge = &model.edges[chain.edge];
        let Some(label) = &edge.label else { continue };
        let from = chain.ends[0];
        let to = chain.ends[1];
        let source_bundle = chains
            .iter()
            .filter(|other| other.ends[0] == from && model.edges[other.edge].kind == edge.kind)
            .count()
            > 1;
        let target_bundle = chains
            .iter()
            .filter(|other| other.ends[1] == to && model.edges[other.edge].kind == edge.kind)
            .count()
            > 1;
        let anchor = if source_bundle {
            Some(to)
        } else if target_bundle {
            Some(from)
        } else {
            None
        };
        let Some(anchor) = anchor else { continue };
        let required = label.chars().count() + 2;
        match anchor {
            End::Node(node) => node_widths[node] = node_widths[node].max(required),
            End::Way(way) => way_widths[way] = way_widths[way].max(required),
        }
    }

    (node_widths, way_widths)
}

fn place_and_route(
    model: &Model,
    geoms: &mut [NodeGeom],
    chains: &[Chain],
    ways_in_rank: &[Vec<usize>],
    envelope: LayoutEnvelope<'_>,
    gap: usize,
    extra_canvas: usize,
) -> Result<Placement, DiagramError> {
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

    // Long edges (spanning >= LONG_SPAN ranks) route down margin gutters, so
    // they're pulled out of the rank rows entirely — they take no interior
    // space and get a fixed corridor column instead.
    let mut is_corridor_way = vec![false; way_count];
    let mut corridors: Vec<Corridor> = Vec::new();
    for chain in chains {
        if chain.ends.len() < LONG_SPAN + 1 {
            continue;
        }
        let l = chain.ends.len();
        let (End::Node(s), End::Node(t)) = (chain.ends[0], chain.ends[l - 1]) else {
            continue;
        };
        let ways: Vec<usize> = chain
            .ends
            .iter()
            .filter_map(|e| if let End::Way(w) = e { Some(*w) } else { None })
            .collect();
        for &w in &ways {
            is_corridor_way[w] = true;
        }
        corridors.push(Corridor {
            ways,
            s,
            t,
            kind: model.edges[chain.edge].kind,
        });
    }

    // Rank rows as ordered cells: nodes in author order, interior waypoints
    // floating (corridor waypoints are excluded — they live in the gutters).
    let mut orders: Vec<Vec<Cell>> = model
        .ranks
        .iter()
        .enumerate()
        .map(|(r, row)| {
            let mut cells: Vec<Cell> = row.iter().map(|&node| Cell::Node(node)).collect();
            cells.extend(
                ways_in_rank[r]
                    .iter()
                    .filter(|&&w| !is_corridor_way[w])
                    .map(|&w| Cell::Way(w)),
            );
            cells
        })
        .collect();

    let mut way_x = vec![0usize; way_count];
    let gap = responsive_gap(
        &orders,
        geoms,
        envelope.nodes,
        envelope.ways,
        gap,
        envelope.viewport,
    );
    // extra_canvas grows on retry so labels in narrow diagrams get room.
    let mut canvas_w = place_rows(
        &orders,
        geoms,
        &mut way_x,
        envelope.nodes,
        envelope.ways,
        gap,
        extra_canvas,
    );

    // Relax: slot each waypoint toward its interpolated target so a long edge
    // drops through the interior instead of detouring around the edge. Sorting
    // by node *centres* lets a waypoint claim the gap between two boxes; node
    // centres stay monotonic in author order, so only waypoints actually move.
    let center = |node: usize, geoms: &[NodeGeom]| -> usize { geoms[node].x + geoms[node].w / 2 };
    for _ in 0..4 {
        for cells in &mut orders {
            cells.sort_by_key(|c| match *c {
                Cell::Node(node) => center(node, geoms),
                Cell::Way(w) => {
                    let (s, t, i, l) = way_anchor[w];
                    (center(s, geoms) * (l - 1 - i) + center(t, geoms) * i) / (l - 1)
                }
            });
        }
        canvas_w = place_rows(
            &orders,
            geoms,
            &mut way_x,
            envelope.nodes,
            envelope.ways,
            gap,
            extra_canvas,
        );
    }

    // The widest rank establishes the diagram's horizontal frame. Translate
    // every rank above and below it toward the connected rank beside it. This
    // preserves author order and compact spacing while aligning chains such as
    // service -> provider instead of centering each row independently.
    canvas_w = align_rank_blocks(
        model,
        &orders,
        geoms,
        &mut way_x,
        envelope.nodes,
        envelope.ways,
        canvas_w,
    );

    // -- corridor routing for long edges --------------------------------------
    // Assign each corridor to the gutter nearest its endpoints, shift the
    // centered node block past the left gutter, and pin every corridor
    // waypoint to a fixed column so the edge drops as one clean vertical.
    let block_w = canvas_w;
    let block_center = MARGIN_X + block_w / 2;
    let mid_of = |c: &Corridor, geoms: &[NodeGeom]| -> usize {
        (center(c.s, geoms) + center(c.t, geoms)) / 2
    };
    let mut sided: Vec<usize> = (0..corridors.len()).collect();
    sided.sort_by_key(|&i| mid_of(&corridors[i], geoms));
    let mut left: Vec<usize> = Vec::new();
    let mut right: Vec<usize> = Vec::new();
    for i in sided {
        if mid_of(&corridors[i], geoms) <= block_center {
            left.push(i);
        } else {
            right.push(i);
        }
    }
    let left_gutter_w = if left.is_empty() {
        0
    } else {
        left.len() * CORRIDOR_W + 1
    };
    let right_gutter_w = if right.is_empty() {
        0
    } else {
        right.len() * CORRIDOR_W + 1
    };

    if left_gutter_w > 0 {
        for g in geoms.iter_mut() {
            g.x += left_gutter_w;
        }
        for (w, x) in way_x.iter_mut().enumerate() {
            if !is_corridor_way[w] {
                *x += left_gutter_w;
            }
        }
    }

    for (lane, &ci) in left.iter().enumerate() {
        let cx = MARGIN_X + 1 + lane * CORRIDOR_W;
        for &w in &corridors[ci].ways {
            way_x[w] = cx;
        }
    }
    for (lane, &ci) in right.iter().enumerate() {
        let cx = MARGIN_X + left_gutter_w + block_w + 1 + lane * CORRIDOR_W;
        for &w in &corridors[ci].ways {
            way_x[w] = cx;
        }
    }

    let canvas_w = left_gutter_w + block_w + right_gutter_w;

    // Rank-band pass-throughs for corridor edges (channels are handled by the
    // segment router). A waypoint at chain index i sits in rank source_rank + i.
    let mut corridor_bands: Vec<(usize, usize, EdgeKind)> = Vec::new();
    for c in &corridors {
        for &w in &c.ways {
            let (s, _, i, _) = way_anchor[w];
            corridor_bands.push((model.nodes[s].rank + i, way_x[w], c.kind));
        }
    }

    // Flatten to the item list the emit stage walks.
    let rank_items: Vec<Vec<Item>> = orders
        .iter()
        .map(|cells| {
            let mut items = Vec::new();
            for c in cells {
                match c {
                    Cell::Node(node) => items.push(Item {
                        kind: ItemKind::Node(*node),
                        x: geoms[*node].x,
                    }),
                    Cell::Way(w) => items.push(Item {
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
    let mut target_label_bands: Vec<bool> = Vec::new();
    for segs in &channels {
        let lane_count = segs
            .iter()
            .filter_map(|s| s.lane)
            .map(|l| l + 1)
            .max()
            .unwrap_or(0);
        let mut heights = vec![1usize; lane_count];
        let mut places: HashMap<usize, LabelPlace> = HashMap::new();
        let mut embedded_text: Vec<HashSet<usize>> = vec![HashSet::new(); lane_count];
        let mut row_text: Vec<HashSet<usize>> = vec![HashSet::new(); lane_count];
        let mut source_branch_text: Vec<HashSet<usize>> = vec![HashSet::new(); lane_count];
        let mut target_branch_text: HashSet<usize> = HashSet::new();
        let mut has_target_label_band = false;
        for (si, seg) in segs.iter().enumerate() {
            let Some(label) = &seg.label else { continue };
            let lane = seg.lane.expect("labeled segment always has a lane");
            let len = label.chars().count();
            let lo = seg.sx.min(seg.tx);
            let hi = seg.sx.max(seg.tx);
            let source_bundle = segs
                .iter()
                .filter(|other| other.from == seg.from && other.kind == seg.kind)
                .count()
                > 1;
            let target_bundle = segs
                .iter()
                .filter(|other| other.to == seg.to && other.kind == seg.kind)
                .count()
                > 1;

            if source_bundle {
                let x = centered_label_x(seg.tx, len, canvas_w, &target_branch_text).ok_or_else(
                    || DiagramError::Routing(format!("no room for fan-out branch label `{label}`")),
                )?;
                target_branch_text.extend(x.saturating_sub(1)..=x + len);
                has_target_label_band = true;
                places.insert(si, LabelPlace::TargetBranch { x });
                continue;
            }

            if target_bundle {
                let x = centered_label_x(seg.sx, len, canvas_w, &source_branch_text[lane])
                    .ok_or_else(|| {
                        DiagramError::Routing(format!("no room for fan-in branch label `{label}`"))
                    })?;
                source_branch_text[lane].extend(x.saturating_sub(1)..=x + len);
                heights[lane] = 2;
                places.insert(si, LabelPlace::SourceBranch { x });
                continue;
            }

            // Try embedding in the horizontal run first.
            let mut line_avoid = avoid_columns(segs, lane, false);
            line_avoid.extend(embedded_text[lane].iter().copied());
            let run_inner = (lo + 1, hi.saturating_sub(len + 1).max(lo + 1));
            let embedded = (hi > lo + len + 3)
                .then(|| {
                    let preferred = if source_bundle {
                        if seg.tx < seg.sx {
                            seg.tx + 2
                        } else {
                            seg.tx.saturating_sub(len + 2)
                        }
                    } else if target_bundle {
                        if seg.sx < seg.tx {
                            seg.sx + 2
                        } else {
                            seg.sx.saturating_sub(len + 2)
                        }
                    } else {
                        ((lo + hi) / 2).saturating_sub(len / 2)
                    };
                    pick_label_x(preferred, run_inner.0, run_inner.1, len, &line_avoid)
                })
                .flatten();
            if let Some(x) = embedded {
                places.insert(si, LabelPlace::Embedded { x });
                embedded_text[lane].extend(x..x + len);
                continue;
            }

            // Dedicated label row above the line row.
            let mut row_avoid = avoid_columns(segs, lane, true);
            row_avoid.extend(row_text[lane].iter().copied());
            let anchor = if source_bundle {
                seg.tx
            } else if target_bundle {
                seg.sx
            } else {
                (seg.sx + seg.tx) / 2
            };
            let x = pick_label_x(
                anchor.saturating_sub(len / 2),
                MARGIN_X,
                (MARGIN_X + canvas_w).saturating_sub(len),
                len,
                &row_avoid,
            )
            .ok_or_else(|| DiagramError::Routing(format!("no room for edge label `{label}`")))?;
            heights[lane] = 2;
            places.insert(si, LabelPlace::Row { x });
            row_text[lane].extend(x..x + len);
        }
        lane_heights.push(heights);
        label_places.push(places);
        target_label_bands.push(has_target_label_band);
    }

    Ok(Placement {
        rank_items,
        canvas_w,
        channels,
        lane_heights,
        label_places,
        target_label_bands,
        corridor_bands,
    })
}

/// Expand sibling gaps when the viewport has room while preserving a bounded
/// reading width. Every rank must fit the chosen gap, so denser ranks naturally
/// limit the expansion used by the whole graph.
fn responsive_gap(
    orders: &[Vec<Cell>],
    geoms: &[NodeGeom],
    node_envelopes: &[usize],
    way_envelopes: &[usize],
    minimum: usize,
    viewport: usize,
) -> usize {
    if viewport == usize::MAX {
        return minimum;
    }

    let available = viewport.saturating_sub(2 * MARGIN_X);
    let fitting_gap = orders
        .iter()
        .filter(|cells| cells.len() > 1)
        .map(|cells| {
            let content_width: usize = cells
                .iter()
                .map(|cell| cell_width(cell, geoms, node_envelopes, way_envelopes))
                .sum();
            available.saturating_sub(content_width) / (cells.len() - 1)
        })
        .min();

    fitting_gap
        .map(|gap| gap.min(MAX_RESPONSIVE_GAP_X).max(minimum))
        .unwrap_or(minimum)
}

/// Center a branch label on its port. Bundled labels keep this exact attachment;
/// a collision requests a wider placement attempt instead of drifting the text
/// away from the branch it describes.
fn centered_label_x(
    port: usize,
    len: usize,
    canvas_w: usize,
    occupied: &HashSet<usize>,
) -> Option<usize> {
    let x = port.checked_sub(len / 2)?;
    let lo = x.saturating_sub(1);
    let hi = x + len;
    (x >= MARGIN_X
        && x + len <= MARGIN_X + canvas_w
        && !(lo..=hi).any(|column| occupied.contains(&column)))
    .then_some(x)
}

/// Assign x to every node and interior waypoint for the given cell ordering.
/// Rows are centered against the widest row (plus `extra_canvas`). Returns the
/// canvas width. Pure aside from writing the position slices.
fn place_rows(
    orders: &[Vec<Cell>],
    geoms: &mut [NodeGeom],
    way_x: &mut [usize],
    node_envelopes: &[usize],
    way_envelopes: &[usize],
    gap: usize,
    extra_canvas: usize,
) -> usize {
    let row_width = |cells: &[Cell], geoms: &[NodeGeom]| -> usize {
        let mut w = 0;
        for (i, c) in cells.iter().enumerate() {
            if i > 0 {
                w += gap;
            }
            w += cell_width(c, geoms, node_envelopes, way_envelopes);
        }
        w
    };

    let canvas_w = orders
        .iter()
        .map(|cells| row_width(cells, geoms))
        .max()
        .unwrap_or(0)
        .saturating_add(extra_canvas);

    for cells in orders {
        let used = row_width(cells, geoms);
        let mut x = MARGIN_X + (canvas_w - used) / 2;
        for (i, c) in cells.iter().enumerate() {
            if i > 0 {
                x += gap;
            }
            let width = cell_width(c, geoms, node_envelopes, way_envelopes);
            match c {
                Cell::Node(node) => {
                    geoms[*node].x = x + (width - geoms[*node].w) / 2;
                }
                Cell::Way(w) => {
                    way_x[*w] = x + width / 2;
                }
            }
            x += width;
        }
    }
    canvas_w
}

fn cell_width(
    cell: &Cell,
    geoms: &[NodeGeom],
    node_envelopes: &[usize],
    way_envelopes: &[usize],
) -> usize {
    match cell {
        Cell::Node(node) => node_envelopes[*node].max(geoms[*node].w),
        Cell::Way(way) => way_envelopes[*way].max(1),
    }
}

/// Align whole rank blocks from graph relationships while preserving the
/// compact ordering established by [`place_rows`]. The widest rank is stable;
/// ranks above follow their children and ranks below follow their parents.
fn align_rank_blocks(
    model: &Model,
    orders: &[Vec<Cell>],
    geoms: &mut [NodeGeom],
    way_x: &mut [usize],
    node_envelopes: &[usize],
    way_envelopes: &[usize],
    initial_canvas_w: usize,
) -> usize {
    let rank_span = |cells: &[Cell], geoms: &[NodeGeom], way_x: &[usize]| -> usize {
        let Some(first) = cells.first() else { return 0 };
        let Some(last) = cells.last() else { return 0 };
        let (left, _) = cell_bounds(first, geoms, way_x, node_envelopes, way_envelopes);
        let (_, right) = cell_bounds(last, geoms, way_x, node_envelopes, way_envelopes);
        right.saturating_sub(left)
    };

    let anchor = orders
        .iter()
        .enumerate()
        .max_by_key(|(_, cells)| rank_span(cells, geoms, way_x))
        .map(|(rank, _)| rank)
        .unwrap_or(0);

    for rank in (0..anchor).rev() {
        align_rank_toward(
            model,
            &orders[rank],
            geoms,
            way_x,
            node_envelopes,
            way_envelopes,
            true,
        );
    }
    for cells in orders.iter().skip(anchor + 1) {
        align_rank_toward(
            model,
            cells,
            geoms,
            way_x,
            node_envelopes,
            way_envelopes,
            false,
        );
    }

    let right = orders
        .iter()
        .flatten()
        .map(|cell| cell_bounds(cell, geoms, way_x, node_envelopes, way_envelopes).1)
        .max()
        .unwrap_or(MARGIN_X + initial_canvas_w);
    initial_canvas_w.max(right.saturating_sub(MARGIN_X))
}

fn align_rank_toward(
    model: &Model,
    cells: &[Cell],
    geoms: &mut [NodeGeom],
    way_x: &mut [usize],
    node_envelopes: &[usize],
    way_envelopes: &[usize],
    toward_children: bool,
) {
    let mut neighbors: HashSet<usize> = HashSet::new();
    for cell in cells {
        let Cell::Node(node) = cell else { continue };
        for edge in &model.edges {
            let neighbor = if toward_children && edge.from == *node {
                Some(edge.to)
            } else if !toward_children && edge.to == *node {
                Some(edge.from)
            } else {
                None
            };
            if let Some(neighbor) = neighbor {
                neighbors.insert(neighbor);
            }
        }
    }

    if neighbors.is_empty() {
        return;
    }
    let left = cells
        .iter()
        .map(|cell| cell_bounds(cell, geoms, way_x, node_envelopes, way_envelopes).0)
        .min()
        .unwrap_or(MARGIN_X);
    let right = cells
        .iter()
        .map(|cell| cell_bounds(cell, geoms, way_x, node_envelopes, way_envelopes).1)
        .max()
        .unwrap_or(left);
    let mut neighbor_centers: Vec<usize> = neighbors
        .iter()
        .map(|neighbor| geoms[*neighbor].x + geoms[*neighbor].w / 2)
        .collect();
    neighbor_centers.sort_unstable();
    let middle = neighbor_centers.len() / 2;
    let neighbor_center = if neighbor_centers.len().is_multiple_of(2) {
        (neighbor_centers[middle - 1] + neighbor_centers[middle]) / 2
    } else {
        neighbor_centers[middle]
    };
    let mut delta = neighbor_center as isize - ((left + right) / 2) as isize;
    delta = delta.max(MARGIN_X as isize - left as isize);

    if delta == 0 {
        return;
    }
    for cell in cells {
        match cell {
            Cell::Node(node) => geoms[*node].x = geoms[*node].x.saturating_add_signed(delta),
            Cell::Way(way) => way_x[*way] = way_x[*way].saturating_add_signed(delta),
        }
    }
}

fn cell_bounds(
    cell: &Cell,
    geoms: &[NodeGeom],
    way_x: &[usize],
    node_envelopes: &[usize],
    way_envelopes: &[usize],
) -> (usize, usize) {
    match cell {
        Cell::Node(node) => {
            let width = node_envelopes[*node].max(geoms[*node].w);
            let left = geoms[*node].x.saturating_sub((width - geoms[*node].w) / 2);
            (left, left + width)
        }
        Cell::Way(way) => {
            let width = way_envelopes[*way].max(1);
            let left = way_x[*way].saturating_sub(width / 2);
            (left, left + width)
        }
    }
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

    for (node, list) in by_source {
        let mut groups = endpoint_groups(list, segs, |seg| end_center(seg.to));
        groups.sort_by_key(|group| group.iter().map(|&i| end_center(segs[i].to)).min());
        let k = groups.len();
        for (slot, group) in groups.iter().enumerate() {
            let port = spread(&geoms[node], k, slot);
            for &i in group {
                segs[i].sx = port;
            }
        }
    }
    for (node, list) in by_target {
        let mut groups = endpoint_groups(list, segs, |seg| end_center(seg.from));
        groups.sort_by_key(|group| group.iter().map(|&i| end_center(segs[i].from)).min());
        let k = groups.len();
        for (slot, group) in groups.iter().enumerate() {
            let port = spread(&geoms[node], k, slot);
            for &i in group {
                segs[i].tx = port;
            }
        }
    }
}

/// Edges of the same kind that share an endpoint also share a physical port.
/// The resulting common trunk reads as one fan-out or fan-in relationship.
fn endpoint_groups(
    mut indices: Vec<usize>,
    segs: &[Seg],
    opposite_center: impl Fn(&Seg) -> usize,
) -> Vec<Vec<usize>> {
    indices.sort_by_key(|&i| (opposite_center(&segs[i]), segs[i].chain));
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for index in indices {
        if let Some(group) = groups
            .iter_mut()
            .find(|group| segs[group[0]].kind == segs[index].kind)
        {
            group.push(index);
        } else {
            groups.push(vec![index]);
        }
    }
    groups
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
        let straight: Vec<(usize, usize)> = segs
            .iter()
            .enumerate()
            .filter(|(_, s)| s.is_straight())
            .map(|(index, s)| (index, s.sx))
            .collect();
        let straight_cols: HashSet<usize> = straight.iter().map(|(_, column)| *column).collect();

        let mut conflict: Option<(usize, bool)> = None; // (seg idx, fix_source)
        for (i, seg) in segs.iter().enumerate() {
            if seg.is_straight() {
                continue;
            }
            let shared_source = straight.iter().any(|&(straight_index, column)| {
                column == seg.sx
                    && segs[straight_index].from == seg.from
                    && segs[straight_index].kind == seg.kind
            });
            let shared_target = straight.iter().any(|&(straight_index, column)| {
                column == seg.tx
                    && segs[straight_index].to == seg.to
                    && segs[straight_index].kind == seg.kind
            });
            if straight_cols.contains(&seg.sx) && !shared_source {
                conflict = Some((i, true));
                break;
            }
            if straight_cols.contains(&seg.tx) && !shared_target {
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

/// Assign compact routing lanes. Segments with a shared source or target port
/// become one routing unit, giving fan-out and fan-in a common bus. Independent
/// units reuse a lane when their horizontal intervals do not overlap.
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

    struct RouteUnit {
        segments: Vec<usize>,
        lo: usize,
        hi: usize,
    }

    let mut remaining: HashSet<usize> = laned.iter().copied().collect();
    let mut units: Vec<RouteUnit> = Vec::new();
    while let Some(&seed) = remaining.iter().min() {
        let source_group: Vec<usize> = remaining
            .iter()
            .copied()
            .filter(|&i| segs[i].from == segs[seed].from && segs[i].kind == segs[seed].kind)
            .collect();
        let target_group: Vec<usize> = remaining
            .iter()
            .copied()
            .filter(|&i| segs[i].to == segs[seed].to && segs[i].kind == segs[seed].kind)
            .collect();
        let members = if source_group.len() > 1 {
            source_group
        } else if target_group.len() > 1 {
            target_group
        } else {
            vec![seed]
        };
        for member in &members {
            remaining.remove(member);
        }
        let lo = members
            .iter()
            .map(|&i| segs[i].sx.min(segs[i].tx))
            .min()
            .unwrap();
        let hi = members
            .iter()
            .map(|&i| segs[i].sx.max(segs[i].tx))
            .max()
            .unwrap();
        units.push(RouteUnit {
            segments: members,
            lo,
            hi,
        });
    }

    // precedence[a] contains b => unit a must route above unit b because one
    // unit descends at the column where the other rises.
    let unit_count = units.len();
    let mut succs: Vec<Vec<usize>> = vec![Vec::new(); unit_count];
    let mut preds: Vec<Vec<usize>> = vec![Vec::new(); unit_count];
    let mut indeg = vec![0usize; unit_count];
    for a in 0..unit_count {
        for b in 0..unit_count {
            if a == b {
                continue;
            }
            let precedes = units[a].segments.iter().any(|&ai| {
                units[b]
                    .segments
                    .iter()
                    .any(|&bi| segs[ai].sx == segs[bi].tx)
            });
            if precedes && !succs[a].contains(&b) {
                succs[a].push(b);
                preds[b].push(a);
                indeg[b] += 1;
            }
        }
    }

    let mut ready: Vec<usize> = (0..unit_count).filter(|&i| indeg[i] == 0).collect();
    let mut order: Vec<usize> = Vec::with_capacity(unit_count);
    while !ready.is_empty() {
        ready.sort_by_key(|&i| (units[i].lo, units[i].hi, units[i].segments[0]));
        let i = ready.remove(0);
        order.push(i);
        for &j in &succs[i] {
            indeg[j] -= 1;
            if indeg[j] == 0 {
                ready.push(j);
            }
        }
    }
    if order.len() != unit_count {
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

    let mut lanes: Vec<Vec<usize>> = Vec::new();
    let mut unit_lane: Vec<Option<usize>> = vec![None; unit_count];
    for unit in order {
        let min_lane = preds[unit]
            .iter()
            .filter_map(|&pred| unit_lane[pred])
            .map(|lane| lane + 1)
            .max()
            .unwrap_or(0);
        let lane = (min_lane..)
            .find(|&candidate| {
                lanes.get(candidate).is_none_or(|occupants| {
                    occupants.iter().all(|&other| {
                        units[unit].hi + 1 < units[other].lo || units[other].hi + 1 < units[unit].lo
                    })
                })
            })
            .unwrap();
        if lane == lanes.len() {
            lanes.push(Vec::new());
        }
        lanes[lane].push(unit);
        unit_lane[unit] = Some(lane);
        for &segment in &units[unit].segments {
            segs[segment].lane = Some(lane);
        }
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
