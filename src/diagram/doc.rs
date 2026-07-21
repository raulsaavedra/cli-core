//! The sketch document: a typed record set describing an architecture diagram.
//!
//! Ids are identity everywhere. No stage of the pipeline ever keys anything by
//! display label. Validation is loud: a document either resolves into a
//! [`Model`] completely or fails with a [`DiagramError`] naming what's wrong.

use std::collections::HashMap;
use std::fmt;

use serde::Deserialize;

// ---------------------------------------------------------------------------
// Raw document (what serde sees inside a ```sketch fence)
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Doc {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub ticket: Option<String>,
    pub nodes: Vec<NodeSpec>,
    #[serde(default)]
    pub edges: Vec<EdgeSpec>,
    #[serde(default)]
    pub notes: Vec<NoteSpec>,
    #[serde(default)]
    pub hints: Hints,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NodeSpec {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub kind: NodeKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NodeKind {
    #[default]
    Service,
    Store,
    Queue,
    External,
    Decision,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EdgeSpec {
    pub from: String,
    pub to: String,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub kind: EdgeKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EdgeKind {
    #[default]
    Sync,
    Async,
    Event,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NoteSpec {
    /// Node id this note is anchored to.
    pub on: String,
    pub text: String,
    #[serde(default)]
    pub mark: NoteMark,
}

#[derive(Debug, Clone, Copy, PartialEq, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NoteMark {
    #[default]
    Info,
    Uncertain,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Hints {
    /// Explicit layering: outer Vec is rank order (top to bottom), inner Vec
    /// is left-to-right order within the rank. When present it must cover
    /// every node exactly once.
    #[serde(default)]
    pub ranks: Option<Vec<Vec<String>>>,
}

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub enum DiagramError {
    Parse(String),
    Empty,
    DuplicateId(String),
    UnknownRef { context: &'static str, id: String },
    RanksMissingNodes(Vec<String>),
    RanksDuplicate(String),
    EdgeNotForward { from: String, to: String },
    Cycle(Vec<String>),
    Routing(String),
}

impl fmt::Display for DiagramError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Parse(msg) => write!(f, "invalid sketch JSON: {msg}"),
            Self::Empty => write!(f, "sketch has no nodes"),
            Self::DuplicateId(id) => write!(f, "duplicate node id `{id}`"),
            Self::UnknownRef { context, id } => {
                write!(f, "{context} references unknown node id `{id}`")
            }
            Self::RanksMissingNodes(ids) => {
                write!(f, "hints.ranks does not place node(s): {}", ids.join(", "))
            }
            Self::RanksDuplicate(id) => {
                write!(f, "hints.ranks places node `{id}` more than once")
            }
            Self::EdgeNotForward { from, to } => write!(
                f,
                "edge `{from}` -> `{to}` does not flow downward; adjust hints.ranks (back-edges are not supported yet)"
            ),
            Self::Cycle(ids) => write!(
                f,
                "edges form a cycle through {}; add hints.ranks or break the cycle",
                ids.join(" -> ")
            ),
            Self::Routing(msg) => write!(f, "could not route diagram: {msg}"),
        }
    }
}

impl std::error::Error for DiagramError {}

// ---------------------------------------------------------------------------
// Resolved model (indices, never strings)
// ---------------------------------------------------------------------------

#[derive(Debug)]
pub struct ModelNode {
    pub label: String,
    pub kind: NodeKind,
    pub rank: usize,
}

#[derive(Debug)]
pub struct ModelEdge {
    pub from: usize,
    pub to: usize,
    pub label: Option<String>,
    pub kind: EdgeKind,
}

#[derive(Debug)]
pub struct ModelNote {
    pub on: usize,
    pub text: String,
    pub mark: NoteMark,
}

#[derive(Debug)]
pub struct Model {
    pub title: Option<String>,
    pub ticket: Option<String>,
    pub nodes: Vec<ModelNode>,
    pub edges: Vec<ModelEdge>,
    pub notes: Vec<ModelNote>,
    /// Node indices per rank, in left-to-right order.
    pub ranks: Vec<Vec<usize>>,
}

pub fn parse(src: &str) -> Result<Doc, DiagramError> {
    serde_json::from_str(src).map_err(|e| DiagramError::Parse(e.to_string()))
}

pub fn resolve(doc: Doc) -> Result<Model, DiagramError> {
    if doc.nodes.is_empty() {
        return Err(DiagramError::Empty);
    }

    let mut index: HashMap<&str, usize> = HashMap::new();
    for (i, n) in doc.nodes.iter().enumerate() {
        if index.insert(n.id.as_str(), i).is_some() {
            return Err(DiagramError::DuplicateId(n.id.clone()));
        }
    }

    let lookup = |context: &'static str, id: &str| -> Result<usize, DiagramError> {
        index
            .get(id)
            .copied()
            .ok_or_else(|| DiagramError::UnknownRef {
                context,
                id: id.to_string(),
            })
    };

    let edges: Vec<ModelEdge> = doc
        .edges
        .iter()
        .map(|e| {
            Ok(ModelEdge {
                from: lookup("edge", &e.from)?,
                to: lookup("edge", &e.to)?,
                label: e.label.clone(),
                kind: e.kind,
            })
        })
        .collect::<Result<_, DiagramError>>()?;

    let notes: Vec<ModelNote> = doc
        .notes
        .iter()
        .map(|n| {
            Ok(ModelNote {
                on: lookup("note", &n.on)?,
                text: n.text.clone(),
                mark: n.mark,
            })
        })
        .collect::<Result<_, DiagramError>>()?;

    let rank_of = match &doc.hints.ranks {
        Some(hinted) => ranks_from_hints(&doc, hinted, &index)?,
        None => ranks_from_topology(&doc, &edges)?,
    };

    // Edges must flow strictly downward.
    for e in &edges {
        if rank_of[e.from] >= rank_of[e.to] {
            return Err(DiagramError::EdgeNotForward {
                from: doc.nodes[e.from].id.clone(),
                to: doc.nodes[e.to].id.clone(),
            });
        }
    }

    let max_rank = rank_of.iter().copied().max().unwrap_or(0);
    let mut ranks: Vec<Vec<usize>> = vec![Vec::new(); max_rank + 1];
    match &doc.hints.ranks {
        Some(hinted) => {
            // Preserve the author's left-to-right order.
            for (r, row) in hinted.iter().enumerate() {
                for id in row {
                    ranks[r].push(index[id.as_str()]);
                }
            }
        }
        None => {
            for (i, &r) in rank_of.iter().enumerate() {
                ranks[r].push(i);
            }
        }
    }
    // Drop empty trailing ranks the hints may have left (e.g. an empty array).
    ranks.retain(|r| !r.is_empty());

    // Re-derive rank index after retention so ModelNode.rank is consistent.
    let mut final_rank = vec![0usize; doc.nodes.len()];
    for (r, row) in ranks.iter().enumerate() {
        for &i in row {
            final_rank[i] = r;
        }
    }

    let nodes = doc
        .nodes
        .into_iter()
        .enumerate()
        .map(|(i, n)| ModelNode {
            label: n.label,
            kind: n.kind,
            rank: final_rank[i],
        })
        .collect();

    Ok(Model {
        title: doc.title,
        ticket: doc.ticket,
        nodes,
        edges,
        notes,
        ranks,
    })
}

fn ranks_from_hints(
    doc: &Doc,
    hinted: &[Vec<String>],
    index: &HashMap<&str, usize>,
) -> Result<Vec<usize>, DiagramError> {
    let mut rank_of: Vec<Option<usize>> = vec![None; doc.nodes.len()];
    for (r, row) in hinted.iter().enumerate() {
        for id in row {
            let &i = index
                .get(id.as_str())
                .ok_or_else(|| DiagramError::UnknownRef {
                    context: "hints.ranks",
                    id: id.clone(),
                })?;
            if rank_of[i].is_some() {
                return Err(DiagramError::RanksDuplicate(id.clone()));
            }
            rank_of[i] = Some(r);
        }
    }
    let missing: Vec<String> = rank_of
        .iter()
        .enumerate()
        .filter(|(_, r)| r.is_none())
        .map(|(i, _)| doc.nodes[i].id.clone())
        .collect();
    if !missing.is_empty() {
        return Err(DiagramError::RanksMissingNodes(missing));
    }
    Ok(rank_of.into_iter().map(|r| r.unwrap()).collect())
}

/// Longest-path layering over a DAG; loud error on cycles.
fn ranks_from_topology(doc: &Doc, edges: &[ModelEdge]) -> Result<Vec<usize>, DiagramError> {
    let n = doc.nodes.len();

    // Kahn's algorithm for cycle detection + topological order.
    let mut indegree = vec![0usize; n];
    for e in edges {
        indegree[e.to] += 1;
    }
    let mut queue: Vec<usize> = (0..n).filter(|&i| indegree[i] == 0).collect();
    let mut topo: Vec<usize> = Vec::with_capacity(n);
    let mut head = 0;
    while head < queue.len() {
        let u = queue[head];
        head += 1;
        topo.push(u);
        for e in edges.iter().filter(|e| e.from == u) {
            indegree[e.to] -= 1;
            if indegree[e.to] == 0 {
                queue.push(e.to);
            }
        }
    }
    if topo.len() != n {
        let cycle: Vec<String> = (0..n)
            .filter(|&i| indegree[i] > 0)
            .map(|i| doc.nodes[i].id.clone())
            .collect();
        return Err(DiagramError::Cycle(cycle));
    }

    let mut rank = vec![0usize; n];
    for &u in &topo {
        for e in edges.iter().filter(|e| e.from == u) {
            rank[e.to] = rank[e.to].max(rank[u] + 1);
        }
    }
    Ok(rank)
}
