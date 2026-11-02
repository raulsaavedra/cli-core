use ratatui::style::{Color, Modifier};

use super::{render, FileRef, Rendered, Theme};

fn rendered(source: &str, width: usize) -> Rendered {
    render(source, width, &Theme::default())
}

fn plain(source: &str, width: usize) -> Vec<String> {
    rendered(source, width).plain()
}

fn color_of(result: &Rendered, needle: &str) -> Option<(Option<Color>, bool)> {
    result
        .lines
        .iter()
        .flat_map(|line| line.spans.iter())
        .find(|span| span.content.trim() == needle)
        .map(|span| (span.style.fg, span.style.add_modifier.contains(Modifier::BOLD)))
}

#[test]
fn headings_paragraphs_and_lists_render_without_markup() {
    let lines = plain(
        "## Settled\n\nCockpit owns **every** cell.\n\n- tmux hosts\n- cockpit draws\n",
        40,
    );
    assert!(lines[0].starts_with("Settled ─"));
    assert_eq!(lines[1], "");
    assert_eq!(lines[2], "Cockpit owns every cell.");
    assert_eq!(lines[3], "");
    assert_eq!(lines[4], "• tmux hosts");
    assert_eq!(lines[5], "• cockpit draws");
}

#[test]
fn list_items_wrap_under_their_text() {
    assert_eq!(plain("- one two three four five six\n", 16), vec!["• one two three", "  four five six"]);
}

#[test]
fn nested_and_ordered_lists_indent() {
    assert_eq!(
        plain("1. first\n   - inner\n     - deeper\n2. second\n", 30),
        vec!["1. first", "  • inner", "    • deeper", "2. second"]
    );
}

#[test]
fn nested_items_wrap_under_their_own_text() {
    assert_eq!(
        plain("- outer\n  - inner words that wrap here\n", 20),
        vec!["• outer", "  • inner words that", "    wrap here"]
    );
}

#[test]
fn code_blocks_keep_their_lines() {
    let lines = plain("```\nlet a = 1;\nlet b = 2;\n```\n", 30);
    assert_eq!(lines[0].trim(), "let a = 1;");
    assert_eq!(lines[1].trim(), "let b = 2;");
}

#[test]
fn unlabeled_code_blocks_take_the_code_color() {
    let theme = Theme::default();
    let result = rendered("```\nplain words\n```\n", 30);
    assert_eq!(color_of(&result, "plain words"), Some((Some(theme.code), false)));
}

#[test]
fn labeled_code_blocks_are_syntax_highlighted() {
    let theme = Theme::default();
    let result = rendered("```rust\nfn main() {\n    let answer = 42;\n}\n```\n", 40);
    assert_eq!(result.plain(), vec!["  fn main() {", "      let answer = 42;", "  }"]);
    let colors: std::collections::HashSet<_> = result
        .lines
        .iter()
        .flat_map(|line| line.spans.iter())
        .filter(|span| !span.content.trim().is_empty())
        .map(|span| span.style.fg)
        .collect();
    assert!(colors.len() > 2, "{colors:?}");
    assert!(!colors.contains(&Some(theme.code)));
}

#[test]
fn long_code_lines_end_in_an_ellipsis() {
    let lines = plain("```rust\nlet value = some_function_with_a_long_name(argument);\n```\n", 24);
    assert_eq!(lines, vec!["  let value = some_func…"]);
}

#[test]
fn inline_code_and_links_keep_their_text() {
    assert_eq!(plain("Run `cockpit` from [the docs](https://x).", 60), vec!["Run cockpit from the docs."]);
}

#[test]
fn prose_is_bright_and_bold_takes_the_strong_color() {
    let theme = Theme::default();
    let result = rendered("PR #47 sits on **CI green** with `auth-253`.", 80);
    let words: Vec<(&str, Option<Color>, bool)> = result.lines[0]
        .spans
        .iter()
        .filter(|span| !span.content.trim().is_empty())
        .map(|span| {
            (
                span.content.as_ref(),
                span.style.fg,
                span.style.add_modifier.contains(Modifier::BOLD),
            )
        })
        .collect();
    let strong = Some(theme.strong);
    let code = Some(theme.code);
    let prose = Some(theme.prose);
    assert_eq!(
        words,
        [
            ("PR", prose, false),
            ("#47", prose, false),
            ("sits", prose, false),
            ("on", prose, false),
            ("CI", strong, true),
            ("green", strong, true),
            ("with", prose, false),
            ("auth-253", code, false),
            (".", prose, false),
        ]
    );
}

#[test]
fn each_level_of_emphasis_has_its_own_color() {
    let theme = Theme::default();
    let result = rendered(
        "## Decisions\n\n### Screen\n\n#### Detail\n\n| Piece | Job |\n|---|---|\n| Desk | The screen |",
        80,
    );
    assert_eq!(color_of(&result, "Decisions"), Some((Some(theme.heading), true)));
    assert_eq!(color_of(&result, "Screen"), Some((Some(theme.subheading), true)));
    assert_eq!(color_of(&result, "Detail"), Some((Some(theme.minor), true)));
    assert_eq!(color_of(&result, "Piece"), Some((Some(theme.subheading), true)));
}

#[test]
fn a_theme_recolors_every_part() {
    let theme = Theme {
        heading: Color::Red,
        code: Color::Green,
        ..Theme::default()
    };
    let result = render("# Title\n\nUse `x`.", 40, &theme);
    assert_eq!(color_of(&result, "Title"), Some((Some(Color::Red), true)));
    assert_eq!(color_of(&result, "x"), Some((Some(Color::Green), false)));
}

#[test]
fn rules_carry_the_border_color_on_their_spans() {
    let theme = Theme::default();
    let result = rendered("| a | b |\n|---|---|\n| c | d |\n\n---\n", 30);
    for line in result
        .lines
        .iter()
        .filter(|line| line.spans.iter().any(|span| span.content.contains('─')))
    {
        for span in line.spans.iter().filter(|span| span.content.contains('─')) {
            assert_eq!(span.style.fg, Some(theme.border), "{:?}", span.content);
        }
    }
}

#[test]
fn tables_draw_a_box_and_wrap_their_cells() {
    assert_eq!(
        plain(
            "| Item | Next |\n|---|---|\n| Order 6d27 | Deleting it is Raul's call |\n| WOM | None |\n",
            30
        ),
        [
            "┌────────────┬───────────────┐",
            "│    Item    │     Next      │",
            "├────────────┼───────────────┤",
            "│ Order 6d27 │ Deleting it   │",
            "│            │ is Raul's     │",
            "│            │ call          │",
            "├────────────┼───────────────┤",
            "│ WOM        │ None          │",
            "└────────────┴───────────────┘",
        ]
    );
}

#[test]
fn quotes_carry_a_faint_bar() {
    let theme = Theme::default();
    let result = rendered("> held for review", 40);
    assert_eq!(result.plain(), vec!["▎ held for review"]);
    assert_eq!(result.lines[0].spans[0].style.fg, Some(theme.faint));
}

#[test]
fn headings_record_their_level_text_and_rendered_line() {
    let result = rendered("# Top\n\nSome text that\nwraps.\n\n## Next **one**\n\n### Ünïcode\n", 40);
    let headings: Vec<(usize, &str, usize)> = result
        .headings
        .iter()
        .map(|heading| (heading.level, heading.text.as_str(), heading.line))
        .collect();
    assert_eq!(headings, [(1, "Top", 0), (2, "Next one", 4), (3, "Ünïcode", 6)]);
    let lines = result.plain();
    assert!(lines[4].starts_with("Next one"));
    assert_eq!(lines[6], "Ünïcode");
}

#[test]
fn links_record_where_they_render() {
    let result = rendered("See [the docs](https://docs.example) and [the docs](https://docs.example).", 80);
    assert_eq!(result.links.len(), 1);
    assert_eq!(result.links[0].text, "the docs");
    assert_eq!(result.links[0].href, "https://docs.example");
    let places: Vec<(usize, usize, usize)> = result
        .link_occurrences
        .iter()
        .map(|occurrence| (occurrence.line, occurrence.start_col, occurrence.end_col))
        .collect();
    assert_eq!(places, [(0, 4, 12), (0, 17, 25)]);
}

#[test]
fn bare_urls_are_links_in_the_prose_color() {
    let theme = Theme::default();
    let result = rendered("Open https://github.com/o/r/pull/5. Then rest.", 80);
    assert_eq!(result.plain(), vec!["Open https://github.com/o/r/pull/5. Then rest."]);
    assert_eq!(result.links[0].href, "https://github.com/o/r/pull/5");
    let occurrence = &result.link_occurrences[0];
    assert_eq!((occurrence.line, occurrence.start_col, occurrence.end_col), (0, 5, 34));
    assert_eq!(
        color_of(&result, "https://github.com/o/r/pull/5"),
        Some((Some(theme.prose), false))
    );
}

#[test]
fn a_wrapped_link_records_its_first_line() {
    let result = rendered("Words before [a link that wraps](https://x.example) after.", 24);
    assert_eq!(result.plain(), vec!["Words before a link that", "wraps after."]);
    let occurrence = &result.link_occurrences[0];
    assert_eq!((occurrence.line, occurrence.start_col, occurrence.end_col), (0, 13, 24));
    assert_eq!(occurrence.link.text, "a link that wraps");
}

#[test]
fn links_inside_tables_record_their_cell() {
    let result = rendered(
        "Docs first.\n\n| Name | Where |\n|---|---|\n| Docs | [Docs](https://docs.example) |\n",
        40,
    );
    let occurrence = &result.link_occurrences[0];
    let line = &result.plain()[occurrence.line];
    assert!(line.starts_with("│ Docs"), "{line}");
    let shown: String = line
        .chars()
        .skip(occurrence.start_col)
        .take(occurrence.end_col - occurrence.start_col)
        .collect();
    assert_eq!(shown, "Docs");
    assert!(occurrence.start_col > 5);
}

#[test]
fn backticked_full_paths_are_file_references() {
    let result = rendered(
        "Open `~/src/app/create.ts#createOrder` and `/etc/hosts.conf:12`, not `src/main.rs` or `cargo test`.",
        200,
    );
    let refs: Vec<&FileRef> = result.file_refs.iter().collect();
    assert_eq!(refs.len(), 2);
    assert_eq!(refs[0].path, "~/src/app/create.ts");
    assert_eq!(refs[0].symbol.as_deref(), Some("createOrder"));
    assert_eq!(refs[1].path, "/etc/hosts.conf");
    assert_eq!(refs[1].line, Some(12));
    let occurrence = &result.file_ref_occurrences[0];
    assert_eq!((occurrence.line, occurrence.start_col, occurrence.end_col), (0, 5, 36));
    assert!(result.links.is_empty());
}

#[test]
fn file_links_are_file_references_with_their_label() {
    let result = rendered("See [the order handler](file:///abs/path/create.ts#createOrder).", 80);
    assert!(result.links.is_empty());
    let file_ref = &result.file_refs[0];
    assert_eq!(file_ref.path, "/abs/path/create.ts");
    assert_eq!(file_ref.symbol.as_deref(), Some("createOrder"));
    assert_eq!(file_ref.label.as_deref(), Some("the order handler"));
    let occurrence = &result.file_ref_occurrences[0];
    assert_eq!((occurrence.start_col, occurrence.end_col), (4, 21));
}

#[test]
fn paths_inside_code_blocks_stay_literal() {
    let result = rendered("```\n`/etc/hosts.conf`\n```\n", 40);
    assert!(result.file_refs.is_empty());
}

#[test]
fn repeated_file_references_keep_each_occurrence() {
    let result = rendered("`/a/b.rs:1` here\n\nand `/a/b.rs:1` there", 40);
    assert_eq!(result.file_refs.len(), 1);
    let lines: Vec<usize> = result.file_ref_occurrences.iter().map(|occurrence| occurrence.line).collect();
    assert_eq!(lines, [0, 2]);
}

#[test]
fn task_lists_show_their_boxes() {
    assert_eq!(plain("- [x] done\n- [ ] open\n", 30), vec!["☑ done", "☐ open"]);
}

mod diagrams {
    use std::cell::RefCell;

    use super::super::explain::{Drawing, Drawn};
    use super::super::{diagrams, render_with, Theme};

    #[test]
    fn diagrams_are_found_with_the_room_their_fence_gets() {
        let source = "```flow\nA -> B\n```\n\n- item\n\n  ```map\n  A -> B\n  ```\n\n```rust\nfn main() {}\n```\n";
        assert_eq!(
            diagrams(source, 40),
            vec![
                ("flow".to_string(), "A -> B".to_string(), 40),
                ("map".to_string(), "A -> B".to_string(), 38),
            ]
        );
    }

    fn drawn(text: &str, tone: &str) -> Drawn {
        Drawn {
            text: text.to_string(),
            tone: tone.to_string(),
            language: None,
            room: None,
            href: None,
        }
    }

    #[test]
    fn explain_fences_draw_through_the_command_at_the_column_width() {
        let asked = RefCell::new(Vec::new());
        let draw = |kind: &str, source: &str, width: usize| -> Option<Drawing> {
            asked.borrow_mut().push((kind.to_string(), source.to_string(), width));
            Some(vec![vec![drawn("┌───┐", "lane")], vec![drawn("│ A │", "name")]])
        };
        let theme = Theme::default();
        let result = render_with("Before.\n\n```flow\nA -> B: go\n```\n\nAfter.", 40, &theme, &draw);
        assert_eq!(result.plain(), vec!["Before.", "", "┌───┐", "│ A │", "", "After."]);
        assert_eq!(*asked.borrow(), vec![("flow".to_string(), "A -> B: go".to_string(), 40)]);
        assert_eq!(result.lines[2].spans[1].style.fg, Some(theme.faint));
        assert_eq!(result.lines[3].spans[1].style.fg, Some(theme.heading));
    }

    #[test]
    fn a_failed_drawing_shows_the_fence_as_plain_code() {
        let theme = Theme::default();
        let result = render_with("```map\nA -> B\n```\n", 40, &theme, &|_: &str, _: &str, _: usize| None);
        assert_eq!(result.plain(), vec!["  A -> B"]);
        assert_eq!(result.lines[0].spans[1].style.fg, Some(theme.code));
    }

    #[test]
    fn a_drawing_wider_than_the_column_shows_the_fence_as_plain_code() {
        let draw = |_: &str, _: &str, _: usize| Some(vec![vec![drawn(&"─".repeat(41), "lane")]]);
        let result = render_with("```states\na -> b\n```\n", 40, &Theme::default(), &draw);
        assert_eq!(result.plain(), vec!["  a -> b"]);
    }

    #[test]
    fn a_drawing_inside_a_list_takes_the_item_indent() {
        let asked = RefCell::new(0);
        let draw = |_: &str, _: &str, width: usize| {
            *asked.borrow_mut() = width;
            Some(vec![vec![drawn("box", "plain")]])
        };
        let result = render_with("- item\n\n  ```flow\n  A -> B\n  ```\n", 40, &Theme::default(), &draw);
        assert_eq!(result.plain(), vec!["• item", "  box"]);
        assert_eq!(*asked.borrow(), 38);
    }

    #[test]
    fn an_excerpt_header_is_a_file_reference_where_it_renders() {
        let draw = |_: &str, _: &str, _: usize| {
            Some(vec![
                vec![drawn("╭──────────╮", "border")],
                vec![
                    drawn("│ ", "border"),
                    Drawn {
                        href: Some("file:///repo/a.rs#L3".into()),
                        ..drawn("a.rs:3", "link")
                    },
                    drawn("   ", "plain"),
                    drawn(" │", "border"),
                ],
            ])
        };
        let result = render_with("See `/repo/b.rs:1`.\n\n```code\n/repo/a.rs:3\n```\n", 40, &Theme::default(), &draw);
        let places: Vec<(&str, Option<usize>, usize, usize, usize)> = result
            .file_ref_occurrences
            .iter()
            .map(|occurrence| {
                (
                    occurrence.file_ref.path.as_str(),
                    occurrence.file_ref.line,
                    occurrence.line,
                    occurrence.start_col,
                    occurrence.end_col,
                )
            })
            .collect();
        assert_eq!(places, [("/repo/b.rs", Some(1), 0, 4, 16), ("/repo/a.rs", Some(3), 3, 2, 8)]);
    }
}
