# cli-core

Purpose:
- Shared Rust library crate for local and agent-friendly CLIs.
- Keep common infrastructure in one place so downstream tools stay small.

Scope:
- App-agnostic helpers only.
- Modules should be reusable across multiple CLIs without carrying domain rules.

Current modules (under `src/`):
- `output`: terminal and JSON output helpers (`json`, `success`, `errorf`)
- `skills`: install skill directories into an agent skills root (`resolve_skills_dir`, `install`)
- `sqlite`: file-backed SQLite opening and bootstrap helpers via rusqlite (`open_sqlite`, `apply_pragmas`, `db_path`, `ensure_dir_for_file`)
- `stdio`: stdin helpers (`read_stdin`)
- `markdown`: terminal Markdown rendering and metadata extraction (`render`, `Heading`, `Link`, `RenderResult`)

Non-goals:
- No app-specific command definitions or business logic
- No schemas or migrations tied to one CLI
- No remote service clients unless they are broadly reusable and clearly in scope

API policy:
- Keep exports in `src/*.rs`, re-export via `src/lib.rs`
- Prefer small, composable helpers over large frameworks
- Preserve stable behavior for downstream CLIs when possible

Error policy:
- Shared modules should return `Result` (not call `process::exit`)
- Functions signal failure via `Result` or `io::Result`

Build:
- `cargo build` to compile
- `cargo test` to run tests (when added)

Dependency policy:
- Keep dependencies minimal
- Prefer Rust std library unless a dependency clearly earns its keep
- Key deps: rusqlite (bundled), serde/serde_json, crossterm, dirs, unicode-width
