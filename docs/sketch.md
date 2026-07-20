# Sketch diagrams

`cli-core` renders architecture and flow diagrams from a fenced `sketch` JSON block
in any Markdown it draws: ticket descriptions and notes, skill-tree lessons and
decks, `mdv` files, anything that goes through `cli_core::render`. The agent
authors the JSON; the human reviews the rendered diagram spatially. A sketch is
shared understanding, never a source of truth.

Rendering is total and loud: every node and edge appears, or rendering fails with
a visible `✗ sketch:` line naming what went wrong. Nothing is silently dropped.

## Example

```sketch
{
  "title": "token refresh",
  "ticket": "AUTH-12",
  "nodes": [
    {"id": "client", "label": "Client", "kind": "external"},
    {"id": "bff", "label": "BFF"},
    {"id": "auth", "label": "Auth service"},
    {"id": "cache", "label": "Session store", "kind": "store"},
    {"id": "bus", "label": "TokenRefreshed", "kind": "queue"}
  ],
  "edges": [
    {"from": "client", "to": "bff", "label": "POST /refresh"},
    {"from": "bff", "to": "auth", "label": "validate"},
    {"from": "auth", "to": "cache"},
    {"from": "auth", "to": "bus", "kind": "event"}
  ],
  "notes": [
    {"on": "cache", "text": "TTL 30d"},
    {"on": "bus", "text": "consumer TBD", "mark": "uncertain"}
  ]
}
```

It renders top to bottom: an external Client into the BFF, into Auth, which writes
the session store and emits an event onto a queue. Notes become numbered footnotes;
the `[n]` marker on a node ties it to its footnote.

## Layout behavior

The document describes relationships; the renderer owns their geometry:

- Ranks establish vertical stages. Nodes within a rank keep their declared
  left-to-right order and form a compact block aligned toward connected ranks.
- Each node owns a horizontal territory for its box and nearby relationship
  captions. Available viewport width increases separation between peer branches
  until those territories read clearly; remaining width becomes outer margin.
- Same-kind relationships that share a source use one split trunk and bus.
  Same-kind relationships that share a target use one merge bus and trunk.
- A relationship label occupies the final branch entering its target. The
  branch ends above the wrapped italic caption and resumes below it before the
  target-side merge bus.
- A relationship that skips authored ranks follows the nearest clear interior
  track through those rank bands. The track stays close to its source and target
  branch.
- Fixed node order can make some routes cross. A `╪` marks two independent
  routes passing over one another; junction glyphs are reserved for shared
  endpoint bundles.
- Notes render below the graph as numbered, hanging-indent paragraphs at a
  readable caption width. Their prose does not widen the graph.

Use `hints.ranks` when the vertical grouping or left-to-right order carries
meaning. Horizontal coordinates, ports, buses, and labels remain automatic.

## Schema

`deny_unknown_fields` is on — an unknown key is a hard parse error. The fields:

| Field | Required | Notes |
| --- | --- | --- |
| `nodes[]` | yes | one or more typed boxes |
| `nodes[].id` | yes | identity; referenced everywhere, never the label |
| `nodes[].label` | yes | display text |
| `nodes[].kind` | no | `service` (default), `store`, `queue`, `external`, `decision` |
| `edges[]` | no | directed connections |
| `edges[].from` / `.to` | yes | node ids |
| `edges[].label` | no | edge text |
| `edges[].kind` | no | `sync` (default), `async`, `event` |
| `notes[]` | no | footnotes |
| `notes[].on` | yes | node id the note annotates |
| `notes[].text` | yes | footnote body |
| `notes[].mark` | no | `info` (default), `uncertain` |
| `title` | no | header text |
| `ticket` | no | ticket key the sketch belongs to |
| `hints.ranks` | no | `[[id, ...], ...]` explicit layering |

## Visual semantics

- `service` — plain box, the default building block.
- `store` — double border, magenta. DBs, caches, logs, anything persistent.
- `queue` — single border, yellow. Topics, queues, streams.
- `external` — dim box. Entry points, browsers, third parties, terminal sinks.
- `decision` — renders `< label >`, yellow. A branch point.
- edge `sync` solid; `async` dashed; `event` dashed accent (yellow).
- note `info` is a dim footnote; `uncertain` is yellow and flagged with `?`, for open questions.

## Rules that bite

- **Edges flow downward only.** A node's rank must be strictly above its targets. A
  back-edge or a cycle is a hard error — set `hints.ranks` to control layering, or
  rethink direction. Back-edges are not supported yet.
- **Reference nodes by `id`**, never by label, in `from`, `to`, `on`, and `ranks`.
- **`hints.ranks`, when present, must place every node exactly once** — top-to-bottom
  rank order, left-to-right within a rank. Without hints, layers come from
  longest-path layering over the DAG.
- **Too wide for the viewport → a one-line `◆ sketch` placeholder**, never a truncated
  diagram. Node territories and relationship branches determine the required
  width, so keep authored ranks lean enough for the intended reading surface.

## Errors

All loud, all surfaced as `✗ sketch: <reason>`:

- duplicate node id
- an edge, note, or rank references an unknown id
- `hints.ranks` misses a node or places one twice
- an edge runs upward or sideways (not forward)
- the graph has a cycle and no ranks to break it
- routing ran out of room (widen the diagram)

## Source of truth

This doc is the authoring contract. The machine truth is the parser in
`src/diagram/doc.rs` (`deny_unknown_fields` enforces the field set above). Skills and
other surfaces link here rather than restating the schema.
