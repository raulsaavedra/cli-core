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
    /// Width of the graph alone (nodes + edges). Footnotes wrap to the viewport,
    /// so this is what a caller should test against the viewport — not `width`,
    /// which includes the (already wrapped) footnote lines.
    pub graph_width: usize,
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
    render_at(src, usize::MAX)
}

/// Render with a known viewport. The layout uses that room to separate peer
/// branches and center the topology. `graph_width` reports the required canvas;
/// footnotes wrap independently and may affect `width`.
pub fn render_json_in(src: &str, viewport: usize) -> Result<Rendered, DiagramError> {
    render_at(src, viewport)
}

fn render_at(src: &str, viewport: usize) -> Result<Rendered, DiagramError> {
    let doc = doc::parse(src)?;
    let model = doc::resolve(doc)?;
    let scene = layout::compute(&model, viewport)?;
    let grid = paint::paint(&scene);
    Ok(Rendered {
        lines: grid.to_ansi_lines(),
        width: scene.width,
        graph_width: scene.graph_width,
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

    fn dual_bundle_src() -> &'static str {
        r#"{
          "title": "dual bundle routing",
          "nodes": [
            {"id": "client", "label": "Client", "kind": "external"},
            {"id": "operator", "label": "Operator", "kind": "external"},
            {"id": "gateway", "label": "Gateway"},
            {"id": "authority", "label": "Authority"},
            {"id": "identity", "label": "Identity", "kind": "external"},
            {"id": "records", "label": "Records", "kind": "store"},
            {"id": "policy", "label": "Policy Engine"}
          ],
          "edges": [
            {"from": "client", "to": "gateway", "label": "incoming request"},
            {"from": "operator", "to": "authority", "label": "direct management"},
            {"from": "operator", "to": "gateway", "label": "delegated access"},
            {"from": "gateway", "to": "authority", "label": "account operation"},
            {"from": "gateway", "to": "records", "label": "domain operation"},
            {"from": "authority", "to": "identity", "label": "identity state"},
            {"from": "records", "to": "policy", "label": "access policy"},
            {"from": "authority", "to": "records", "label": "state projection", "kind": "async"}
          ],
          "hints": {
            "ranks": [
              ["client", "operator"],
              ["gateway"],
              ["authority"],
              ["identity", "records"],
              ["policy"]
            ]
          }
        }"#
    }

    fn descriptive_bundles_src() -> &'static str {
        r#"{
          "title": "descriptive bundled edges",
          "nodes": [
            {"id": "source", "label": "Source", "kind": "external"},
            {"id": "left", "label": "Adapter A"},
            {"id": "right", "label": "Adapter B"},
            {"id": "broker", "label": "Broker"},
            {"id": "metadata", "label": "Metadata", "kind": "store"},
            {"id": "runtime", "label": "Runtime"},
            {"id": "engine", "label": "Engine"},
            {"id": "target", "label": "Target", "kind": "external"}
          ],
          "edges": [
            {"from": "source", "to": "left", "label": "invoke semantic operation"},
            {"from": "source", "to": "right", "label": "invoke management operation"},
            {"from": "left", "to": "broker", "label": "forward identity and action"},
            {"from": "right", "to": "broker", "label": "forward lifecycle operation"},
            {"from": "broker", "to": "metadata", "label": "persist ownership metadata"},
            {"from": "broker", "to": "runtime", "label": "start or stop managed runtime"},
            {"from": "runtime", "to": "engine", "label": "open command transport"},
            {"from": "engine", "to": "target", "label": "inspect and control target"}
          ],
          "hints": {
            "ranks": [
              ["source"],
              ["left", "right"],
              ["broker"],
              ["metadata", "runtime"],
              ["engine"],
              ["target"]
            ]
          }
        }"#
    }

    fn assert_caption_attached(lines: &[String], prefix: &str) {
        let line = lines
            .iter()
            .find(|line| line.contains(prefix))
            .unwrap_or_else(|| panic!("rendered diagram is missing caption `{prefix}`"));
        assert!(
            line.contains("├─") || line.contains("─┤"),
            "caption `{prefix}` should attach directly to its relationship branch"
        );
    }

    fn normalized_text(lines: &[String]) -> String {
        lines
            .join(" ")
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn assert_words_rendered(text: &str, label: &str) {
        for word in label.split_whitespace() {
            assert!(
                text.contains(word),
                "caption `{label}` is missing word `{word}`"
            );
        }
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
        assert_eq!(
            count(&text, "▼"),
            1,
            "fan-in branches share one target trunk and arrowhead"
        );
    }

    #[test]
    fn dual_bundles_keep_every_relationship_caption_attached() {
        let rendered = render_json_in(dual_bundle_src(), 120)
            .expect("dual source and target bundles should route without false junctions");
        let lines: Vec<String> = rendered.lines.iter().map(|line| strip_ansi(line)).collect();
        let text = normalized_text(&lines);

        for (label, prefix) in [
            ("incoming request", "incoming"),
            ("direct management", "direct"),
            ("delegated access", "delegated"),
            ("account operation", "account"),
            ("domain operation", "domain"),
            ("identity state", "identity"),
            ("access policy", "access"),
            ("state projection", "state"),
        ] {
            assert_words_rendered(&text, label);
            assert_caption_attached(&lines, prefix);
        }
        assert!(text.contains('┄') || text.contains('┆'));
    }

    #[test]
    fn descriptive_branches_use_attached_wrapped_captions() {
        let rendered = render_json_in(descriptive_bundles_src(), 120)
            .expect("descriptive branch labels should participate in layout");
        let lines = rendered
            .lines
            .iter()
            .map(|line| strip_ansi(line))
            .collect::<Vec<_>>();
        let text = normalized_text(&lines);

        for (label, prefix) in [
            ("invoke semantic operation", "semantic"),
            ("invoke management operation", "management"),
            ("forward identity and action", "identity"),
            ("forward lifecycle operation", "lifecycle"),
            ("persist ownership metadata", "persist"),
            ("start or stop managed runtime", "start"),
            ("open command transport", "open"),
            ("inspect and control target", "inspect"),
        ] {
            assert_words_rendered(&text, label);
            assert_caption_attached(&lines, prefix);
        }
        assert_eq!(rendered.graph_width, 120);
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
        assert_eq!(
            count(&text, "▼"),
            3,
            "long edge still lands exactly one arrow"
        );
        assert!(text.contains("┆"), "async edge renders dashed");
    }

    #[test]
    fn notes_render_as_footnotes() {
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
        // Annotated nodes carry a marker keyed to the footnote list.
        assert!(text.contains("SNS topic [1]"), "node shows its marker");
        assert!(text.contains("Worker [2]"));
        // Notes live below the diagram as footnotes, not inline boxes.
        assert!(text.contains("[1] SNS topic — ? fan-out consumer TBD"));
        assert!(text.contains("[2] Worker — idempotent"));
        // No rounded note boxes in the graph any more.
        assert!(
            !text.contains("╭"),
            "notes no longer render as inline boxes"
        );
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
        // A reaches C through a clear interior track across B's rank. Both
        // relationships converge on one target port and share its arrowhead.
        assert_eq!(count(&text, "▼"), 1);
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
            assert_words_rendered(&text, label);
        }
        assert_eq!(
            count(&text, "▼"),
            7,
            "the two server-global inputs share one fan-in arrowhead"
        );
    }

    #[test]
    fn doc_example_renders() {
        // The authoring doc's example must always render, or the doc lies.
        let doc = include_str!("../../docs/sketch.md");
        let marker = "```sketch\n";
        let start = doc
            .find(marker)
            .expect("docs/sketch.md has a sketch example");
        let body = &doc[start + marker.len()..];
        let end = body.find("```").expect("sketch fence closes");
        render_json(&body[..end]).expect("docs/sketch.md example must render");
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
        let err =
            render_json(r#"{"nodes": [{"id": "a", "label": "A"}, {"id": "a", "label": "A2"}]}"#)
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
        assert!(matches!(
            err,
            DiagramError::UnknownRef {
                context: "note",
                ..
            }
        ));
    }
}
