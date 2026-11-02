# cli-core

Shared Rust crate for building local and agent-friendly CLIs.

`cli-core` is intentionally small. It provides reusable building blocks that show up across multiple command-line tools:

- `output` — JSON and human-readable terminal output
- `sqlite` — open local SQLite databases with sensible defaults for CLI workloads
- `stdio` — read piped stdin content
- `markdown` — render Markdown to ratatui lines in a themed palette, with its headings, links, and file references
- `ansi` — parse ANSI-styled strings into ratatui `Span` objects for TUI rendering
- `nvim` — launch Neovim with structured handoff payloads and detect quit-to-terminal cwd handoff requests
- `worktrees` — discover git repositories and their worktrees under a directory
- `claude` — read Claude Code profiles, live sessions, transcript activity, and session titles
- `activity` — join worktrees with the agents working in them, most active first

The crate is best thought of as an opinionated utility library for local-first CLIs, not a full CLI framework.

## Usage

Add `cli-core` as a path dependency in your `Cargo.toml` and name the modules you use as features:

```toml
[dependencies]
cli-core = { path = "../packages/cli-core", features = ["markdown", "nvim"] }
```

Each module except `stdio` sits behind a Cargo feature of the same name, and no feature is on by default, so a CLI compiles only the modules and dependencies it names:

| Feature | Pulls in |
|---|---|
| `activity` | `claude` and `worktrees` |
| `ansi` | ratatui |
| `claude` | dirs, serde, serde_json |
| `markdown` | pulldown-cmark, ratatui, serde, serde_json, syntect, unicode-width |
| `nvim` | serde, serde_json |
| `output` | serde, serde_json |
| `sqlite` | dirs, rusqlite (bundled) |
| `worktrees` | nothing beyond std |

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

Terminal Markdown rendered straight to ratatui lines. mdv and cockpit's desk both draw through it, so they share one look.

- `render(source: &str, width: usize, theme: &Theme) -> Rendered`
- `Theme` holds one color per job: `prose`, `strong` (bold text), `heading` (`#` and `##` headings and list markers), `subheading` (`###` headings and table headers), `minor` (deeper headings), `code`, `link`, `quote`, `faint` (quote bars), and `border` (rules and table frames). `Theme::default()` is the desk's palette.
- `Rendered` holds `lines`, `headings` (level, text, and the line it renders on), `links` with `link_occurrences`, and `file_refs` with `file_ref_occurrences`. Each occurrence records the line and the start and end display columns where it renders. `plain()` returns the lines as text.

It draws `#` and `##` headings with a rule to the edge, wrapped paragraphs, nested, ordered, and task lists, quotes behind a faint bar, boxed tables whose cells wrap, and rules. Fenced code is highlighted with syntect and the bundled night-owl theme (`assets/night-owl.tmTheme`); a fence without a language, or with one syntect does not know, takes the code color. A line wider than the width ends in `…`.

Fences tagged `flow`, `map`, `states`, or `code` are drawn by `explain-render`, the command the explain mod in `~/src/config` ships and links into `~/.local/bin`. The renderer runs `explain-render <kind> <width>` with the block's source on stdin, reads back one array of `{text, tone}` pieces per line, maps each tone onto the `Theme`, and highlights a code excerpt's lines with syntect. Drawings are cached by kind, source, and width, and a document's blocks are drawn in parallel the first time it renders. When the command is missing, fails, runs past five seconds, or draws wider than the column, the fence shows as plain code. A code excerpt's header path is a file reference to the excerpt's first line.

A file reference is inline code holding a full path that starts with `/` or `~/` and ends in a file extension, optionally followed by `#symbol` or `:line`, such as `` `~/src/app/create.ts#createOrder` ``, or a link to a `file://` URL with an optional `#L12` or `#symbol` anchor. `FileRef::target_line()` returns the line to open, reading the file to find a symbol's definition with `definition_line`. A bare `http://` or `https://` URL counts as a link and keeps the prose color.

### `ansi`

Bridge between ANSI-styled strings, such as diff output, and ratatui's styled text model. Ratatui does not interpret raw ANSI escape codes; this module parses them into `Span` objects.

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

### `worktrees`

Git worktrees on disk. A repository is identified by its common git directory, so a linked worktree sitting next to its main checkout is not counted as a second repository. Bare entries and worktrees whose directory is gone are left out.

- `discover(root: &Path) -> io::Result<Vec<Worktree>>` — every worktree of every repository at `root` or one of its direct child directories
- `containing(paths: &[PathBuf]) -> io::Result<Vec<Worktree>>` — the worktrees whose checkouts contain the given paths
- `Worktree` — `repo`, `path`, `branch` (`None` when detached), `is_main`, `dirty`, `last_commit_at`

### `claude`

Claude Code state across account profiles, read from each profile's config directory.

- `profiles() -> Vec<Profile>` — rows of `profiles.tsv` in `CLAUDE_PROFILE_ROOT`, or `~/src/config/claude`
- `live_sessions(&[Profile]) -> io::Result<Vec<LiveSession>>` — `<config dir>/sessions/<pid>.json` records whose pid is alive and started at the recorded `procStart`, with busy or idle status, cwd, tmux pane, and transcript path
- `project_activity(&[Profile]) -> Vec<ProjectActivity>` — per profile and working directory, the newest transcript and its modification time, sorted by profile and directory
- `session_title(&Path) -> Option<String>` — the newest custom title, else the newest AI title, else the first typed prompt; for a subagent transcript, its spawn description, else the spawning session's title

Sessions write transcripts to `<config dir>/projects/<slug>/<session id>.jsonl`, where the slug is the working directory with every non-alphanumeric character turned into `-`. The slug cannot be reversed, so `project_activity` reads the directory from the transcripts. Subagents and workflow agents write theirs under `<slug>/<session id>/subagents/`, each with its own working directory, so a worktree only subagents touched still shows activity. A subagent's description comes from the `<agent>.meta.json` beside its transcript.

### `activity`

Worktrees joined with their agents. A session belongs to the deepest worktree containing its working directory.

- `discover(&Scope) -> io::Result<Vec<WorktreeEntry>>` — live sessions first, then by the later of last agent activity and last commit, then by repository and path
- `Scope::Root(PathBuf)` — every worktree that `worktrees::discover` finds at the root
- `Scope::RecentAgents(Duration)` — every worktree with a live session, or agent activity within the window, in any profile
- `WorktreeEntry { worktree, agent }`, where `Agent` holds `live` (`Working`, `Idle`, `None`), `profile`, `title`, and `last_active_at`

`cargo run --example activity -- ~/src` prints the top entries for a root, and `-- --recent 7` for the last week.

## Install scripts

`scripts/install-binary.sh` is sourced by each CLI's `install.sh`.

- `install_binary <src> <dest>` — copy a release binary into place, strip the quarantine attribute, and ad-hoc codesign it on macOS
- `install_cli_skills <repo-root>` — symlink every `skills/*/` directory containing a `SKILL.md` into `~/.agents/skills`, overriding the destination with `AGENTS_SKILLS_DIR`

Skills are symlinked rather than copied so the repository stays the only copy.

## Development

```bash
cargo build --all-features
cargo test --all-features
cargo clippy --all-features --all-targets
```

## Scope

`cli-core` should stay small and app-agnostic.

Included:

- reusable helpers for local CLI infrastructure
- local SQLite bootstrapping
- terminal rendering utilities
- shared install-script helpers for binaries and skills
- ANSI-to-ratatui conversion for TUI consumers
- the one terminal Markdown renderer for TUI consumers
- Neovim handoff helpers for local CLI/TUI workflows
- discovery of git worktrees and the local Claude Code agents working in them

Not included:

- app-specific business logic
- command wiring for a particular CLI
- network services or remote API clients
- database schemas tied to one application
