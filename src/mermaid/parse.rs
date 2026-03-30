use super::graph::{Direction, EdgeStyle, FlowGraph, NodeShape, Subgraph};

/// Parse mermaid flowchart content into a FlowGraph.
/// Returns None if the content is not a flowchart or is fundamentally malformed.
pub fn parse_flowchart(content: &str) -> Option<FlowGraph> {
    let lines: Vec<&str> = content.lines().collect();

    // Find the first non-blank, non-comment line — must be the direction declaration
    let mut start = 0;
    for (i, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("%%") {
            continue;
        }
        start = i;
        break;
    }

    let direction = parse_direction(lines.get(start)?)?;
    let mut graph = FlowGraph::new(direction);
    let mut subgraph_stack: Vec<String> = Vec::new();

    for line in &lines[start + 1..] {
        // Split on semicolons for multiple statements per line
        for segment in line.split(';') {
            let trimmed = segment.trim();
            if trimmed.is_empty() || trimmed.starts_with("%%") {
                continue;
            }

            // Skip directive lines
            if trimmed.starts_with("style ")
                || trimmed.starts_with("classDef ")
                || trimmed.starts_with("class ")
                || trimmed.starts_with("click ")
                || trimmed.starts_with("linkStyle ")
            {
                continue;
            }

            // Skip `direction` inside subgraphs
            if trimmed.starts_with("direction ") {
                continue;
            }

            // Handle subgraph
            if trimmed.starts_with("subgraph ") {
                let rest = trimmed["subgraph ".len()..].trim();
                let (id, label) = parse_subgraph_header(rest);
                let sg_id = id.to_string();
                graph.subgraphs.push(Subgraph {
                    id: sg_id.clone(),
                    label: label.to_string(),
                    node_ids: Vec::new(),
                });
                subgraph_stack.push(sg_id);
                continue;
            }

            if trimmed == "end" {
                subgraph_stack.pop();
                continue;
            }

            // Parse node/edge chain
            let current_sg = subgraph_stack.last().map(|s| s.as_str());
            parse_statement(trimmed, &mut graph, current_sg);
        }
    }

    // Populate subgraph node_ids from the nodes that reference them
    for sg in &mut graph.subgraphs {
        sg.node_ids = graph
            .nodes
            .iter()
            .filter(|n| n.subgraph_id.as_deref() == Some(&sg.id))
            .map(|n| n.id.clone())
            .collect();
    }

    // Resolve phantom nodes: when a subgraph ID is used as an edge endpoint,
    // redirect edges to the first/last internal node and remove the phantom.
    resolve_subgraph_refs(&mut graph);

    Some(graph)
}

/// Parse `graph TD` or `flowchart LR` etc.
fn parse_direction(line: &str) -> Option<Direction> {
    let trimmed = line.trim();
    let rest = if trimmed.starts_with("flowchart ") {
        trimmed["flowchart ".len()..].trim()
    } else if trimmed.starts_with("graph ") {
        trimmed["graph ".len()..].trim()
    } else {
        return None;
    };

    match rest {
        "TD" | "TB" => Some(Direction::TopDown),
        "LR" => Some(Direction::LeftRight),
        "BT" => Some(Direction::BottomTop),
        "RL" => Some(Direction::RightLeft),
        _ => None,
    }
}

/// Parse `subgraph ID[Label]` or `subgraph ID` header.
/// Returns (id, label).
fn parse_subgraph_header(rest: &str) -> (&str, &str) {
    // Check for `ID[Label]`
    if let Some(bracket_pos) = rest.find('[') {
        let id = rest[..bracket_pos].trim();
        let label_end = rest.rfind(']').unwrap_or(rest.len());
        let label = &rest[bracket_pos + 1..label_end];
        (id, label)
    } else {
        // Just an ID, label = ID
        let id = rest.trim();
        (id, id)
    }
}

/// Parse a single statement (a node definition or a chain of edges).
fn parse_statement(line: &str, graph: &mut FlowGraph, subgraph_id: Option<&str>) {
    let bytes = line.as_bytes();

    // Parse the first node
    let (first_id, first_label, first_shape, new_pos) = match parse_node_ref(line, 0) {
        Some(result) => result,
        None => return,
    };
    let mut pos = new_pos;

    // Register the first node
    if first_shape != NodeShape::Default {
        graph.add_node(&first_id, &first_label, first_shape, subgraph_id);
    } else {
        graph.ensure_node(&first_id, subgraph_id);
    }

    let mut prev_id = first_id;

    // Parse chain: edge_op -> node -> edge_op -> node -> ...
    loop {
        // Skip whitespace
        while pos < bytes.len() && bytes[pos] == b' ' {
            pos += 1;
        }
        if pos >= bytes.len() {
            break;
        }

        // Try to parse an edge operator
        let (style, label, new_pos) = match parse_edge_op(line, pos) {
            Some(result) => result,
            None => break,
        };
        pos = new_pos;

        // Skip whitespace
        while pos < bytes.len() && bytes[pos] == b' ' {
            pos += 1;
        }

        // Parse the next node
        let (next_id, next_label, next_shape, new_pos) = match parse_node_ref(line, pos) {
            Some(result) => result,
            None => break,
        };
        pos = new_pos;

        // Register the next node
        if next_shape != NodeShape::Default {
            graph.add_node(&next_id, &next_label, next_shape, subgraph_id);
        } else {
            graph.ensure_node(&next_id, subgraph_id);
        }

        graph.add_edge(&prev_id, &next_id, label, style);
        prev_id = next_id;
    }
}

/// Parse a node reference at position `pos`.
/// Returns (id, label, shape, new_pos) or None.
fn parse_node_ref(line: &str, pos: usize) -> Option<(String, String, NodeShape, usize)> {
    let bytes = line.as_bytes();
    if pos >= bytes.len() {
        return None;
    }

    // Parse node ID: alphanumeric + underscore + hyphen
    let id_start = pos;
    let mut i = pos;
    while i < bytes.len()
        && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_' || bytes[i] == b'-')
    {
        i += 1;
    }
    if i == id_start {
        return None;
    }
    let id = line[id_start..i].to_string();

    // Check for shape brackets
    if i < bytes.len() {
        match bytes[i] {
            b'[' => {
                // Rect: [label]
                let label_start = i + 1;
                if let Some(end) = find_closing(line, label_start, b'[', b']') {
                    let label = clean_label(&line[label_start..end]);
                    return Some((id, label, NodeShape::Rect, end + 1));
                }
            }
            b'(' => {
                // Check for Circle: ((label))
                if i + 1 < bytes.len() && bytes[i + 1] == b'(' {
                    let label_start = i + 2;
                    if let Some(inner_end) = find_closing(line, label_start, b'(', b')') {
                        // Expect another closing paren
                        if inner_end + 1 < bytes.len() && bytes[inner_end + 1] == b')' {
                            let label = clean_label(&line[label_start..inner_end]);
                            return Some((id, label, NodeShape::Circle, inner_end + 2));
                        }
                    }
                }
                // Rounded: (label)
                let label_start = i + 1;
                if let Some(end) = find_closing(line, label_start, b'(', b')') {
                    let label = clean_label(&line[label_start..end]);
                    return Some((id, label, NodeShape::Rounded, end + 1));
                }
            }
            b'{' => {
                // Diamond: {label}
                let label_start = i + 1;
                if let Some(end) = find_closing(line, label_start, b'{', b'}') {
                    let label = clean_label(&line[label_start..end]);
                    return Some((id, label, NodeShape::Diamond, end + 1));
                }
            }
            _ => {}
        }
    }

    // No shape — Default, label = id
    Some((id.clone(), id, NodeShape::Default, i))
}

/// Find the position of the matching closing bracket, handling nesting.
fn find_closing(line: &str, start: usize, open: u8, close: u8) -> Option<usize> {
    let bytes = line.as_bytes();
    let mut depth = 1;
    let mut i = start;
    while i < bytes.len() {
        if bytes[i] == open {
            depth += 1;
        } else if bytes[i] == close {
            depth -= 1;
            if depth == 0 {
                return Some(i);
            }
        }
        i += 1;
    }
    None
}

/// Clean up a label: replace `<br/>` and `<br>` with space, trim.
fn clean_label(raw: &str) -> String {
    raw.replace("<br/>", " ")
        .replace("<br>", " ")
        .replace("<br />", " ")
        .trim()
        .to_string()
}

/// Parse an edge operator at position `pos`.
/// Handles: `-->`, `---`, `-->|label|`, `-- label -->`, `-- label ---`
/// Returns (style, optional_label, new_pos) or None.
fn parse_edge_op(line: &str, pos: usize) -> Option<(EdgeStyle, Option<String>, usize)> {
    let bytes = line.as_bytes();
    if pos + 2 >= bytes.len() {
        return None;
    }

    // Must start with --
    if bytes[pos] != b'-' || bytes[pos + 1] != b'-' {
        return None;
    }

    // `-->|label|` : arrow with pipe-delimited label
    if pos + 2 < bytes.len() && bytes[pos + 2] == b'>' {
        let after_arrow = pos + 3;
        if after_arrow < bytes.len() && bytes[after_arrow] == b'|' {
            // Find closing pipe
            if let Some(pipe_end) = line[after_arrow + 1..].find('|') {
                let label = line[after_arrow + 1..after_arrow + 1 + pipe_end]
                    .trim()
                    .to_string();
                let new_pos = after_arrow + 1 + pipe_end + 1;
                return Some((EdgeStyle::Arrow, Some(label), new_pos));
            }
        }
        // Plain `-->`
        return Some((EdgeStyle::Arrow, None, after_arrow));
    }

    // `---` : plain line (no arrow)
    if pos + 2 < bytes.len() && bytes[pos + 2] == b'-' {
        // Check for `---|label|`
        let after_line = pos + 3;
        if after_line < bytes.len() && bytes[after_line] == b'|' {
            if let Some(pipe_end) = line[after_line + 1..].find('|') {
                let label = line[after_line + 1..after_line + 1 + pipe_end]
                    .trim()
                    .to_string();
                let new_pos = after_line + 1 + pipe_end + 1;
                return Some((EdgeStyle::Line, Some(label), new_pos));
            }
        }
        return Some((EdgeStyle::Line, None, after_line));
    }

    // `-- label -->` or `-- label ---`: text label between dashes
    if pos + 2 < bytes.len() && bytes[pos + 2] == b' ' {
        // Scan forward for --> or ---
        let rest = &line[pos + 3..];
        if let Some(arrow_pos) = rest.find("-->") {
            let label = rest[..arrow_pos].trim().to_string();
            let label = if label.is_empty() { None } else { Some(label) };
            return Some((EdgeStyle::Arrow, label, pos + 3 + arrow_pos + 3));
        }
        if let Some(line_pos) = rest.find("---") {
            let label = rest[..line_pos].trim().to_string();
            let label = if label.is_empty() { None } else { Some(label) };
            return Some((EdgeStyle::Line, label, pos + 3 + line_pos + 3));
        }
    }

    None
}

/// When a subgraph ID is used as an edge endpoint (e.g. `NEW --> BFF` where BFF
/// is a subgraph), the parser creates a phantom node. This function redirects those
/// edges to the subgraph's first (incoming) or last (outgoing) internal node,
/// then removes the phantom.
fn resolve_subgraph_refs(graph: &mut super::graph::FlowGraph) {
    // Collect redirects first to avoid borrow conflicts
    let mut redirects: Vec<(String, String, String)> = Vec::new(); // (sg_id, first_internal, last_internal)

    for sg in &graph.subgraphs {
        if sg.node_ids.is_empty() {
            continue;
        }
        // Check if a standalone node exists with this subgraph's ID
        // that is NOT itself inside the subgraph
        if let Some(node_idx) = graph.node_index(&sg.id) {
            let node_in_sg = graph.nodes[node_idx].subgraph_id.as_deref() == Some(&sg.id);
            if !node_in_sg {
                let first = sg.node_ids[0].clone();
                let last = sg.node_ids.last().unwrap().clone();
                redirects.push((sg.id.clone(), first, last));
            }
        }
    }

    for (sg_id, first_id, last_id) in &redirects {
        for edge in &mut graph.edges {
            if edge.to == *sg_id {
                edge.to = first_id.clone();
            }
            if edge.from == *sg_id {
                edge.from = last_id.clone();
            }
        }
        graph.remove_node(sg_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mermaid::graph::NodeShape;

    #[test]
    fn simple_graph_td() {
        let content = "graph TD\n  A --> B";
        let g = parse_flowchart(content).unwrap();
        assert_eq!(g.direction, Direction::TopDown);
        assert_eq!(g.nodes.len(), 2);
        assert_eq!(g.edges.len(), 1);
        assert_eq!(g.edges[0].from, "A");
        assert_eq!(g.edges[0].to, "B");
        assert_eq!(g.edges[0].style, EdgeStyle::Arrow);
    }

    #[test]
    fn flowchart_keyword() {
        let content = "flowchart LR\n  A --> B";
        let g = parse_flowchart(content).unwrap();
        assert_eq!(g.direction, Direction::LeftRight);
    }

    #[test]
    fn node_shapes() {
        let content = "graph TD\n  A[Rect] --> B(Rounded)\n  B --> C{Diamond}\n  C --> D((Circle))";
        let g = parse_flowchart(content).unwrap();
        assert_eq!(g.nodes[0].shape, NodeShape::Rect);
        assert_eq!(g.nodes[0].label, "Rect");
        assert_eq!(g.nodes[1].shape, NodeShape::Rounded);
        assert_eq!(g.nodes[2].shape, NodeShape::Diamond);
        assert_eq!(g.nodes[3].shape, NodeShape::Circle);
    }

    #[test]
    fn edge_labels() {
        let content = "graph TD\n  A -->|Yes| B\n  A -->|No| C";
        let g = parse_flowchart(content).unwrap();
        assert_eq!(g.edges[0].label.as_deref(), Some("Yes"));
        assert_eq!(g.edges[1].label.as_deref(), Some("No"));
    }

    #[test]
    fn chained_edges() {
        let content = "graph TD\n  A --> B --> C";
        let g = parse_flowchart(content).unwrap();
        assert_eq!(g.edges.len(), 2);
        assert_eq!(g.edges[0].from, "A");
        assert_eq!(g.edges[0].to, "B");
        assert_eq!(g.edges[1].from, "B");
        assert_eq!(g.edges[1].to, "C");
    }

    #[test]
    fn subgraph_parsing() {
        let content =
            "flowchart TB\n  A --> B\n  subgraph SG[My Group]\n    B --> C\n  end\n  C --> D";
        let g = parse_flowchart(content).unwrap();
        assert_eq!(g.subgraphs.len(), 1);
        assert_eq!(g.subgraphs[0].id, "SG");
        assert_eq!(g.subgraphs[0].label, "My Group");
        // B and C are inside the subgraph
        let sg_nodes: Vec<_> = g
            .nodes
            .iter()
            .filter(|n| n.subgraph_id.as_deref() == Some("SG"))
            .map(|n| n.id.as_str())
            .collect();
        assert!(sg_nodes.contains(&"C"));
    }

    #[test]
    fn br_in_labels() {
        let content = "graph TD\n  A[Hello<br/>World] --> B";
        let g = parse_flowchart(content).unwrap();
        assert_eq!(g.nodes[0].label, "Hello World");
    }

    #[test]
    fn non_flowchart_returns_none() {
        let content = "sequenceDiagram\n  Alice->>Bob: Hello";
        assert!(parse_flowchart(content).is_none());
    }

    #[test]
    fn comments_and_directives_skipped() {
        let content = "graph TD\n  %% comment\n  style A fill:#fff\n  A --> B";
        let g = parse_flowchart(content).unwrap();
        assert_eq!(g.nodes.len(), 2);
        assert_eq!(g.edges.len(), 1);
    }

    #[test]
    fn text_label_edge() {
        let content = "graph TD\n  A -- label text --> B";
        let g = parse_flowchart(content).unwrap();
        assert_eq!(g.edges[0].label.as_deref(), Some("label text"));
        assert_eq!(g.edges[0].style, EdgeStyle::Arrow);
    }

    #[test]
    fn semicolon_separator() {
        let content = "graph TD\n  A --> B; B --> C";
        let g = parse_flowchart(content).unwrap();
        assert_eq!(g.edges.len(), 2);
    }

    #[test]
    fn subgraph_edge_resolution() {
        let content = "flowchart TB\n  A --> SG1\n  subgraph SG1[My Group]\n    B1 --> B2\n  end\n  SG1 --> C";
        let g = parse_flowchart(content).unwrap();
        // Phantom node SG1 should be removed
        assert!(
            g.node_index("SG1").is_none(),
            "phantom node SG1 should be removed"
        );
        // Edge A --> SG1 should become A --> B1 (first internal node)
        assert_eq!(g.edges[0].from, "A");
        assert_eq!(g.edges[0].to, "B1");
        // Edge SG1 --> C should become B2 --> C (last internal node)
        let last_edge = g.edges.iter().find(|e| e.to == "C").unwrap();
        assert_eq!(last_edge.from, "B2");
    }
}
