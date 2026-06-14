//! Terminal-native architecture diagrams.
//!
//! A `sketch` document (JSON) describes typed nodes, edges, and notes; the
//! engine lays it out on a character grid where every element owns the cells
//! it occupies, then paints it once. Rendering is total and loud: every node
//! and edge in the document appears in the output, or rendering fails with an
//! error naming what went wrong. Nothing is ever silently dropped.

mod doc;
mod grid;
mod layout;
mod paint;

pub use doc::DiagramError;

/// A rendered sketch plus the facts a consumer needs to embed it honestly.
#[derive(Debug)]
pub struct Rendered {
    pub lines: Vec<String>,
    pub width: usize,
    pub title: Option<String>,
    /// Ticket reference carried by the document (e.g. "AUTH-42").
    pub ticket: Option<String>,
    pub node_count: usize,
    pub edge_count: usize,
}

/// Render a sketch JSON document to ANSI-styled lines at its natural width.
/// The caller decides what to do when the diagram is wider than its viewport
/// (the markdown renderer shows an honest placeholder instead of truncating).
pub fn render_json(src: &str) -> Result<Rendered, DiagramError> {
    let doc = doc::parse(src)?;
    let model = doc::resolve(doc)?;
    let scene = layout::compute(&model)?;
    let grid = paint::paint(&scene);
    Ok(Rendered {
        lines: grid.to_ansi_lines(),
        width: scene.width,
        title: model.title.clone(),
        ticket: model.ticket.clone(),
        node_count: model.nodes.len(),
        edge_count: model.edges.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strip_ansi(s: &str) -> String {
        let mut out = String::new();
        let mut in_escape = false;
        for ch in s.chars() {
            if ch == '\x1b' {
                in_escape = true;
                continue;
            }
            if in_escape {
                if ch == 'm' {
                    in_escape = false;
                }
                continue;
            }
            out.push(ch);
        }
        out
    }

    fn render_plain(src: &str) -> String {
        let rendered = render_json(src).expect("render should succeed");
        let text = rendered
            .lines
            .iter()
            .map(|l| strip_ansi(l))
            .collect::<Vec<_>>()
            .join("\n");
        println!("{text}\n");
        text
    }

    fn count(haystack: &str, needle: &str) -> usize {
        haystack.match_indices(needle).count()
    }

    #[test]
    fn simple_chain() {
        let text = render_plain(
            r#"{
              "nodes": [
                {"id": "a", "label": "API Gateway"},
                {"id": "b", "label": "Lambda"},
                {"id": "c", "label": "DynamoDB", "kind": "store"}
              ],
              "edges": [
                {"from": "a", "to": "b", "label": "invoke"},
                {"from": "b", "to": "c"}
              ]
            }"#,
        );
        assert_eq!(count(&text, "API Gateway"), 1);
        assert_eq!(count(&text, "Lambda"), 1);
        assert_eq!(count(&text, "DynamoDB"), 1);
        assert_eq!(count(&text, "invoke"), 1);
        assert_eq!(count(&text, "▼"), 2, "one arrowhead per edge");
        assert!(text.contains("╔"), "store renders with a double border");
    }

    #[test]
    fn fan_out_labeled_branches() {
        let text = render_plain(
            r#"{
              "nodes": [
                {"id": "d", "label": "Decision", "kind": "decision"},
                {"id": "l", "label": "Left branch"},
                {"id": "m", "label": "Middle branch"},
                {"id": "r", "label": "Right branch"}
              ],
              "edges": [
                {"from": "d", "to": "l", "label": "left path"},
                {"from": "d", "to": "m", "label": "middle path"},
                {"from": "d", "to": "r", "label": "right path"}
              ]
            }"#,
        );
        assert_eq!(count(&text, "left path"), 1);
        assert_eq!(count(&text, "middle path"), 1);
        assert_eq!(count(&text, "right path"), 1);
        assert_eq!(count(&text, "▼"), 3);
        assert!(text.contains("< Decision >"));
        // The old renderer smashed labels into each other; these must be intact.
        assert!(!text.contains("pathmiddle"));
        assert!(!text.contains("pathright"));
    }

    #[test]
    fn fan_in() {
        let text = render_plain(
            r#"{
              "nodes": [
                {"id": "a", "label": "Service A"},
                {"id": "b", "label": "Service B"},
                {"id": "c", "label": "Service C"},
                {"id": "q", "label": "events", "kind": "queue"}
              ],
              "edges": [
                {"from": "a", "to": "q"},
                {"from": "b", "to": "q"},
                {"from": "c", "to": "q"}
              ]
            }"#,
        );
        assert_eq!(count(&text, "▼"), 3, "three distinct ports, three arrows");
    }

    #[test]
    fn long_edge_spans_ranks() {
        let text = render_plain(
            r#"{
              "nodes": [
                {"id": "top", "label": "Client"},
                {"id": "mid", "label": "BFF"},
                {"id": "bot", "label": "Audit Log", "kind": "store"}
              ],
              "edges": [
                {"from": "top", "to": "mid"},
                {"from": "mid", "to": "bot"},
                {"from": "top", "to": "bot", "label": "raw copy", "kind": "async"}
              ]
            }"#,
        );
        assert_eq!(count(&text, "raw copy"), 1);
        assert_eq!(count(&text, "▼"), 3, "long edge still lands exactly one arrow");
        assert!(text.contains("┆"), "async edge renders dashed");
    }

    #[test]
    fn notes_render_with_marks() {
        let text = render_plain(
            r#"{
              "nodes": [
                {"id": "sns", "label": "SNS topic", "kind": "queue"},
                {"id": "w", "label": "Worker"}
              ],
              "edges": [{"from": "sns", "to": "w", "kind": "event"}],
              "notes": [
                {"on": "sns", "text": "fan-out consumer TBD", "mark": "uncertain"},
                {"on": "w", "text": "idempotent"}
              ]
            }"#,
        );
        assert_eq!(count(&text, "? fan-out consumer TBD"), 1);
        assert_eq!(count(&text, "idempotent"), 1);
        assert!(text.contains("╭"), "notes use rounded borders");
    }

    #[test]
    fn hints_control_layering() {
        let src = r#"{
          "nodes": [
            {"id": "a", "label": "A"},
            {"id": "b", "label": "B"},
            {"id": "c", "label": "C"}
          ],
          "edges": [{"from": "a", "to": "c"}, {"from": "b", "to": "c"}],
          "hints": {"ranks": [["a"], ["b"], ["c"]]}
        }"#;
        let rendered = render_json(src).expect("hinted doc renders");
        let text = rendered
            .lines
            .iter()
            .map(|l| strip_ansi(l))
            .collect::<Vec<_>>()
            .join("\n");
        // a's edge to c crosses b's rank via a waypoint; both arrows land.
        assert_eq!(count(&text, "▼"), 2);
    }

    #[test]
    fn realistic_flow() {
        let text = render_plain(
            r#"{
              "title": "invitation acceptance",
              "nodes": [
                {"id": "link", "label": "Invitation link", "kind": "external"},
                {"id": "cg", "label": "client-global"},
                {"id": "uc", "label": "AcceptInvitation", "kind": "decision"},
                {"id": "new", "label": "Registration form"},
                {"id": "noop", "label": "Already has access", "kind": "external"},
                {"id": "bff", "label": "BFF acceptInvitation"},
                {"id": "sg", "label": "server-global"},
                {"id": "sns", "label": "UserBusinessCreated", "kind": "queue"}
              ],
              "edges": [
                {"from": "link", "to": "cg"},
                {"from": "cg", "to": "uc", "label": "POST /accept"},
                {"from": "uc", "to": "new", "label": "no account"},
                {"from": "uc", "to": "noop", "label": "already linked"},
                {"from": "uc", "to": "sg", "label": "has account"},
                {"from": "new", "to": "bff"},
                {"from": "bff", "to": "sg"},
                {"from": "sg", "to": "sns", "kind": "event"}
              ],
              "hints": {"ranks": [["link"], ["cg"], ["uc"], ["new", "noop"], ["bff"], ["sg"], ["sns"]]}
            }"#,
        );
        for label in [
            "POST /accept",
            "no account",
            "already linked",
            "has account",
        ] {
            assert_eq!(count(&text, label), 1, "label `{label}` intact exactly once");
        }
        assert_eq!(count(&text, "▼"), 8, "every edge lands exactly one arrow");
    }

    // -- loud failures ------------------------------------------------------

    #[test]
    fn error_bad_json() {
        let err = render_json("{nope").unwrap_err();
        assert!(matches!(err, DiagramError::Parse(_)));
    }

    #[test]
    fn error_empty() {
        let err = render_json(r#"{"nodes": []}"#).unwrap_err();
        assert!(matches!(err, DiagramError::Empty));
    }

    #[test]
    fn error_duplicate_id() {
        let err = render_json(
            r#"{"nodes": [{"id": "a", "label": "A"}, {"id": "a", "label": "A2"}]}"#,
        )
        .unwrap_err();
        assert!(matches!(err, DiagramError::DuplicateId(id) if id == "a"));
    }

    #[test]
    fn error_unknown_edge_ref() {
        let err = render_json(
            r#"{"nodes": [{"id": "a", "label": "A"}], "edges": [{"from": "a", "to": "ghost"}]}"#,
        )
        .unwrap_err();
        assert!(matches!(err, DiagramError::UnknownRef { id, .. } if id == "ghost"));
    }

    #[test]
    fn error_cycle_without_hints() {
        let err = render_json(
            r#"{
              "nodes": [{"id": "a", "label": "A"}, {"id": "b", "label": "B"}],
              "edges": [{"from": "a", "to": "b"}, {"from": "b", "to": "a"}]
            }"#,
        )
        .unwrap_err();
        assert!(matches!(err, DiagramError::Cycle(_)));
    }

    #[test]
    fn error_ranks_missing_node() {
        let err = render_json(
            r#"{
              "nodes": [{"id": "a", "label": "A"}, {"id": "b", "label": "B"}],
              "hints": {"ranks": [["a"]]}
            }"#,
        )
        .unwrap_err();
        assert!(matches!(err, DiagramError::RanksMissingNodes(ids) if ids == vec!["b"]));
    }

    #[test]
    fn error_edge_against_hints() {
        let err = render_json(
            r#"{
              "nodes": [{"id": "a", "label": "A"}, {"id": "b", "label": "B"}],
              "edges": [{"from": "b", "to": "a"}],
              "hints": {"ranks": [["a"], ["b"]]}
            }"#,
        )
        .unwrap_err();
        assert!(matches!(err, DiagramError::EdgeNotForward { .. }));
    }

    #[test]
    fn error_note_on_unknown_node() {
        let err = render_json(
            r#"{
              "nodes": [{"id": "a", "label": "A"}],
              "notes": [{"on": "ghost", "text": "hm"}]
            }"#,
        )
        .unwrap_err();
        assert!(matches!(err, DiagramError::UnknownRef { context: "note", .. }));
    }
}
