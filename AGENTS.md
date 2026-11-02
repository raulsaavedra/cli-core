# cli-core

Purpose:
- Shared Rust library crate for local and agent-friendly CLIs.
- Keep common infrastructure in one place so downstream tools stay small.

Scope:
- App-agnostic helpers only.
- Modules should be reusable across multiple CLIs without carrying domain rules.

Current modules (under `src/`), each behind a Cargo feature of the same name except `stdio`:
- `output`: terminal and JSON output helpers (`json`, `success`, `errorf`)
- `sqlite`: file-backed SQLite opening and bootstrap helpers via rusqlite (`open_sqlite`, `apply_pragmas`, `db_path`, `ensure_dir_for_file`)
- `stdio`: stdin helpers (`read_stdin`), always compiled
- `ansi`: ANSI-styled strings parsed into ratatui lines (`parse_line`, `parse_lines`)
- `markdown`: Markdown rendered to ratatui lines in a `Theme`, with headings, links, and file references (`render`, `Theme`, `Rendered`, `FileRef`, `definition_line`); split into `layout` (wrapping and fitting), `code` (syntect highlighting with the bundled night-owl theme), `refs` (file references), and `explain` (`flow`, `map`, `states`, and `code` fences drawn by the `explain-render` command)
- `nvim`: structured Neovim handoff helpers (`NvimHandoff`, `NvimTarget`, `launch_handoff`, `NvimQuitCwd`)
- `worktrees`: git worktrees under a root or containing given paths (`discover`, `containing`, `Worktree`)
- `claude`: Claude Code profiles, live sessions, transcript activity, and titles (`profiles`, `live_sessions`, `project_activity`, `session_title`)
- `activity`: worktrees joined with their agents and ranked (`discover`, `Scope`, `WorktreeEntry`, `Agent`, `Live`); its feature turns on `claude` and `worktrees`

Features:
- No feature is on by default. Each consumer names the modules it uses in its `Cargo.toml`, so it compiles only those modules and their dependencies.
- A new module gets a feature named after it that turns on exactly the optional dependencies it needs, gated in `src/lib.rs`.
- Test fixtures shared across modules live in `src/scratch.rs`, compiled only for tests, so each feature's tests build alone.

Non-goals:
- No app-specific command definitions or business logic
- No schemas or migrations tied to one CLI
- No remote service clients unless they are broadly reusable and clearly in scope

Markdown rendering:
- `cli_core::markdown` is the one terminal Markdown renderer: mdv and cockpit's desk both call `render`, and the look is the desk's. A consumer changes colors only by passing a `Theme`.
- Fences the explain mod draws go through `explain-render` on `PATH`; any failure, or a drawing wider than the column, falls back to the plain fence, so rendering never depends on the command being installed. Tests drive the writer through `render_with` and a fake drawer.
- Metadata comes from the same pass that lays out the lines: every link and file reference piece carries a mark, and its occurrence is read from where the mark lands after wrapping, so columns stay exact in tables and wrapped lines.

Neovim handoff contract:
- `NVIM_HANDOFF` points Neovim at a JSON handoff payload.
- `NVIM_QUIT_CWD_FILE` points Neovim at a writable cwd signal file.
- Keep these names protocol-scoped; do not encode personal, app-specific, or library-ownership prefixes into the cross-process contract.

API policy:
- Keep exports in `src/*.rs`, re-export via `src/lib.rs`
- Prefer small, composable helpers over large frameworks
- Preserve stable behavior for downstream CLIs when possible

Error policy:
- Shared modules should return `Result` (not call `process::exit`)
- Functions signal failure via `Result` or `io::Result`

Build:
- `cargo build --all-features` to compile every module
- `cargo test --all-features` to run tests; `cargo test --features <name>` checks one module alone
- `cargo clippy --all-features --all-targets` for lints

Dependency policy:
- Keep dependencies minimal
- Prefer Rust std library unless a dependency clearly earns its keep
- Every dependency is optional and belongs to the features that need it: rusqlite (bundled), serde/serde_json, dirs, pulldown-cmark, ratatui, syntect, unicode-width
