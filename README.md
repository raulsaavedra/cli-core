# cli-core

Shared Rust crate for building local and agent-friendly CLIs.

`cli-core` is intentionally small. It provides reusable building blocks that show up across multiple command-line tools:

- `output` — JSON and human-readable terminal output
- `sqlite` — open local SQLite databases with sensible defaults for CLI workloads
- `stdio` — read piped stdin content
- `markdown` — render Markdown in the terminal and extract plain-text metadata
- `diagram` — render architecture and flow diagrams from `sketch` JSON fenced blocks
- `ansi` — parse ANSI-styled strings into ratatui `Span` objects for TUI rendering
- `nvim` — launch Neovim with structured handoff payloads and detect quit-to-terminal cwd handoff requests

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

### `diagram`

Terminal-native architecture diagrams. A fenced `sketch` JSON block describes typed nodes, edges, and notes; the engine lays them out on a character grid and paints once. The `markdown` module renders these fences inline, so any cli-core markdown consumer draws them for free.

- `render_json(src: &str) -> Result<Rendered, DiagramError>` — render a sketch document to ANSI lines at its natural width
- `render_json_in(src: &str, viewport: usize) -> Result<Rendered, DiagramError>` — render a sketch in the available viewport, using topology-shaped branches and wrapped edge captions

See [`docs/sketch.md`](docs/sketch.md) for the authoring format. The parser in `src/diagram/doc.rs` is the canonical schema.

### `ansi`

Bridge between the markdown renderer's ANSI output and ratatui's styled text model. Ratatui does not interpret raw ANSI escape codes; this module parses them into `Span` objects.

- `parse_line(s: &str) -> Line<'static>` — parse a single ANSI-styled string into a ratatui `Line`
- `parse_lines(lines: &[String]) -> Vec<Line<'static>>` — parse multiple lines at once

### `nvim`

Structured Neovim handoff helpers for CLIs that temporarily leave a TUI, open Neovim, and then decide whether to restore the TUI or return to the shell.

- `NvimHandoff` — JSON-serializable payload with source, action, cwd, targets, and context
- `NvimTarget` — file or directory target metadata for the handoff payload
- `write_handoff_file(&NvimHandoff) -> Result<PathBuf, String>` — write a handoff payload to a temp JSON file
- `launch_handoff(&NvimHandoff) -> Result<ExitStatus, String>` — launch `nvim` with `NVIM_HANDOFF` pointing at the handoff file
- `NvimQuitCwd::ensure() -> Result<NvimQuitCwd, String>` — ensure child processes have a `NVIM_QUIT_CWD_FILE` signal file
- `quit_to_terminal_requested() -> bool` — return true when the current quit-cwd signal file exists and has content
- `editor_is_neovim(editor: &str) -> bool` — detect whether an editor command resolves to `nvim`

Environment contracts:

- `NVIM_HANDOFF` points Neovim at the structured JSON handoff file.
- `NVIM_QUIT_CWD_FILE` points Neovim at a writable file. A Neovim quit-to-terminal action writes the target cwd into that file; the parent CLI can then exit instead of restoring its TUI, and a shell wrapper can `cd` to the written directory.

## Install scripts

`scripts/install-binary.sh` is sourced by each CLI's `install.sh`.

- `install_binary <src> <dest>` — copy a release binary into place, strip the quarantine attribute, and ad-hoc codesign it on macOS
- `install_cli_skills <repo-root>` — symlink every `skills/*/` directory containing a `SKILL.md` into `~/.agents/skills`, overriding the destination with `AGENTS_SKILLS_DIR`

Skills are symlinked rather than copied so the repository stays the only copy.

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
- shared install-script helpers for binaries and skills
- ANSI-to-ratatui conversion for TUI consumers
- Neovim handoff helpers for local CLI/TUI workflows

Not included:

- app-specific business logic
- command wiring for a particular CLI
- network services or remote API clients
- database schemas tied to one application
