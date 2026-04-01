# cli-core

Shared Rust crate for building local and agent-friendly CLIs.

`cli-core` is intentionally small. It provides reusable building blocks that show up across multiple command-line tools:

- `output` — JSON and human-readable terminal output
- `skills` — install agent skill directories into the default locations or another destination
- `sqlite` — open local SQLite databases with sensible defaults for CLI workloads
- `stdio` — read piped stdin content
- `markdown` — render Markdown in the terminal and extract plain-text metadata
- `ansi` — parse ANSI-styled strings into ratatui `Span` objects for TUI rendering

The crate is best thought of as an opinionated utility library for local-first CLIs, not a full CLI framework.

## Usage

Add `cli-core` as a path dependency in your `Cargo.toml`:

```toml
[dependencies]
cli-core = { path = "../packages/cli-core" }
```

## Modules

### `output`

Small helpers for writing machine-readable and human-readable output.

- `json<T: Serialize>(value: &T)` — JSON-encode a value to stdout with 2-space indentation
- `success(msg: &str)` — write a message to stdout with a trailing newline
- `errorf(msg: &str)` — write a message to stderr with a trailing newline

### `skills`

Helpers for installing a skill directory into an agent skills folder.

- `resolve_default_skills_dirs() -> io::Result<Vec<PathBuf>>` — returns the default destination: `~/.agents/skills`
- `resolve_skills_dir(dest: Option<&str>) -> io::Result<PathBuf>` — returns the absolute path of `dest`, or `~/.agents/skills` when `None`
- `install(opts: &InstallOptions) -> io::Result<PathBuf>` — copy or symlink a skill directory to the destination; returns the installed path

`InstallOptions` fields: `src_dir`, `dest_dir`, `name` (optional override), `overwrite`, `link`.

### `sqlite`

Helpers for file-backed SQLite databases used by local CLIs.

- `db_path(app_name: &str, filename: &str) -> PathBuf` — returns `~/.{app_name}/{filename}`
- `ensure_dir_for_file(path: &Path)` — create the parent directory of `path` if it does not exist
- `apply_pragmas(db: &Connection, pragmas: &[String])` — execute a list of `PRAGMA` statements on an open connection
- `open_sqlite(opts: &OpenOptions) -> Result<(Connection, PathBuf), String>` — open or create a SQLite database with sensible CLI defaults

Default pragmas applied by `open_sqlite`:

- `busy_timeout = 10000`
- `foreign_keys = ON`
- `journal_mode = WAL`

Custom pragmas in `OpenOptions.pragmas` are merged on top of the defaults by key, so explicit overrides win.

`OpenOptions` fields: `app_name`, `filename`, `path` (explicit path override), `pragmas`, `migrate`.

Example:

```rust
use cli_core::sqlite::{open_sqlite, OpenOptions};
use rusqlite::Connection;

let (db, path) = open_sqlite(&OpenOptions {
    app_name: "myapp".into(),
    filename: "state.db".into(),
    path: None,
    pragmas: vec![],
    migrate: Some(|db: &Connection| {
        db.execute_batch(
            "CREATE TABLE IF NOT EXISTS items (
                id   INTEGER PRIMARY KEY,
                name TEXT NOT NULL
            )",
        )
        .map_err(|e| e.to_string())
    }),
})?;

println!("db path: {}", path.display());
```

### `stdio`

Simple stdin helper for CLIs that accept piped or redirected input.

- `read_stdin() -> io::Result<String>` — read all of stdin and return the trimmed content

### `markdown`

Terminal Markdown rendering with extracted metadata for downstream navigation and indexing.

- `render(content: &str, width: usize) -> RenderResult`

`RenderResult` fields:

- `rendered: String` — ANSI-styled output joined by newlines
- `lines: Vec<String>` — individual rendered lines (may contain ANSI codes)
- `plain: Vec<String>` — plain-text lines with ANSI stripped and trailing spaces trimmed
- `headings: Vec<Heading>` — heading metadata with `level`, `text`, and zero-based `line` index
- `links: Vec<Link>` — extracted links with `text` and `href`

### `ansi`

Bridge between the markdown renderer's ANSI output and ratatui's styled text model. Ratatui does not interpret raw ANSI escape codes; this module parses them into `Span` objects.

- `parse_line(s: &str) -> Line<'static>` — parse a single ANSI-styled string into a ratatui `Line`
- `parse_lines(lines: &[String]) -> Vec<Line<'static>>` — parse multiple lines at once

## Development

```bash
cargo build
cargo test
```

## Scope

`cli-core` should stay small and app-agnostic.

Included:

- reusable helpers for local CLI infrastructure
- local SQLite bootstrapping
- terminal rendering utilities
- skill-install helpers for agent workflows
- ANSI-to-ratatui conversion for TUI consumers

Not included:

- app-specific business logic
- command wiring for a particular CLI
- network services or remote API clients
- database schemas tied to one application
