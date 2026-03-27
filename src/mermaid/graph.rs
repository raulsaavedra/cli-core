use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Direction {
    TopDown,
    LeftRight,
    BottomTop,
    RightLeft,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum NodeShape {
    Rect,
    Rounded,
    Diamond,
    Circle,
    Default,
}

#[derive(Debug, Clone)]
pub struct Node {
    pub id: String,
    pub label: String,
    pub shape: NodeShape,
    pub subgraph_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum EdgeStyle {
    Arrow,
    Line,
}

#[derive(Debug, Clone)]
pub struct Edge {
    pub from: String,
    pub to: String,
    pub label: Option<String>,
    pub style: EdgeStyle,
}

#[derive(Debug, Clone)]
pub struct Subgraph {
    pub id: String,
    pub label: String,
    pub node_ids: Vec<String>,
}

#[derive(Debug)]
pub struct FlowGraph {
    pub direction: Direction,
    pub nodes: Vec<Node>,
    pub edges: Vec<Edge>,
    pub subgraphs: Vec<Subgraph>,
    node_map: HashMap<String, usize>,
}

impl FlowGraph {
    pub fn new(direction: Direction) -> Self {
        Self {
            direction,
            nodes: Vec::new(),
            edges: Vec::new(),
            subgraphs: Vec::new(),
            node_map: HashMap::new(),
        }
    }

    /// Ensure a node exists. If it doesn't, create one with Default shape and id as label.
    /// Returns the index into `self.nodes`.
    pub fn ensure_node(&mut self, id: &str, subgraph_id: Option<&str>) -> usize {
        if let Some(&idx) = self.node_map.get(id) {
            // Update subgraph_id if not already set and we're inside a subgraph
            if subgraph_id.is_some() && self.nodes[idx].subgraph_id.is_none() {
                self.nodes[idx].subgraph_id = subgraph_id.map(|s| s.to_string());
            }
            idx
        } else {
            let idx = self.nodes.len();
            self.nodes.push(Node {
                id: id.to_string(),
                label: id.to_string(),
                shape: NodeShape::Default,
                subgraph_id: subgraph_id.map(|s| s.to_string()),
            });
            self.node_map.insert(id.to_string(), idx);
            idx
        }
    }

    /// Add or update a node with explicit label and shape.
    pub fn add_node(
        &mut self,
        id: &str,
        label: &str,
        shape: NodeShape,
        subgraph_id: Option<&str>,
    ) -> usize {
        if let Some(&idx) = self.node_map.get(id) {
            self.nodes[idx].label = label.to_string();
            self.nodes[idx].shape = shape;
            if subgraph_id.is_some() && self.nodes[idx].subgraph_id.is_none() {
                self.nodes[idx].subgraph_id = subgraph_id.map(|s| s.to_string());
            }
            idx
        } else {
            let idx = self.nodes.len();
            self.nodes.push(Node {
                id: id.to_string(),
                label: label.to_string(),
                shape,
                subgraph_id: subgraph_id.map(|s| s.to_string()),
            });
            self.node_map.insert(id.to_string(), idx);
            idx
        }
    }

    pub fn add_edge(&mut self, from: &str, to: &str, label: Option<String>, style: EdgeStyle) {
        self.edges.push(Edge {
            from: from.to_string(),
            to: to.to_string(),
            label,
            style,
        });
    }

    pub fn node_index(&self, id: &str) -> Option<usize> {
        self.node_map.get(id).copied()
    }

    /// Remove a node by ID and rebuild indices.
    pub fn remove_node(&mut self, id: &str) {
        if let Some(&idx) = self.node_map.get(id) {
            self.nodes.remove(idx);
            self.node_map.clear();
            for (i, node) in self.nodes.iter().enumerate() {
                self.node_map.insert(node.id.clone(), i);
            }
        }
    }
}
