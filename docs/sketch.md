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

- Ranks establish vertical order. Nodes within a rank keep their declared order
  and align horizontally from their parent and child relationships.
- Edges of the same kind that share an endpoint also share its physical port and
  trunk. Each declared edge then receives its own connector row, including an
  edge that belongs to both a fan-out bundle and a fan-in bundle.
- Labels sit inside their edge's horizontal connector when space permits. A
  longer label receives a dedicated row immediately above that connector, and
  its text width participates in layout before anything is drawn.
- An edge that skips an authored rank travels through a margin gutter instead
  of crossing through the intervening nodes.
- Fixed node order can make some routes cross. A `╪` marks two independent
  routes passing over one another; junction glyphs are reserved for shared
  endpoint bundles.
- Connected ranks align around the median center of their neighboring nodes.
  For an even sibling group, the midpoint between the two middle nodes keeps
  the parent centered over the complete group.
- Sibling ranks expand into available viewport space up to a readable gap. Narrow
  viewports reduce that spacing before the graph is considered too wide.

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
  diagram. The renderer first uses compact spacing for narrow viewports. Keep
  ranks lean when the node labels themselves exceed the available width.

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
