# cli-core

Purpose:
- Shared Go packages for local and agent-friendly CLIs.
- Keep common infrastructure in one place so downstream tools stay small.

Scope:
- App-agnostic helpers only.
- Packages should be reusable across multiple CLIs without carrying domain rules.

Current packages:
- `pkg/output`: terminal and JSON output helpers
- `pkg/skills`: install skill directories into an agent skills root
- `pkg/sqliteutil`: file-backed SQLite opening and bootstrap helpers
- `pkg/stdio`: stdin helpers
- `pkg/termmd`: terminal Markdown rendering and metadata extraction

Non-goals:
- No app-specific command definitions or business logic
- No schemas or migrations tied to one CLI
- No remote service clients unless they are broadly reusable and clearly in scope

API policy:
- Keep exports in `pkg/*`
- Document new public APIs in `README.md`
- Prefer small, composable helpers over large frameworks
- Preserve stable behavior for downstream CLIs when possible

Error policy:
- Shared packages should return `error`
- Avoid calling `os.Exit` in library code

Testing policy:
- Add focused unit tests for exported behavior
- Run `go test ./...` before committing

Dependency policy:
- Keep dependencies minimal
- Prefer the standard library unless a dependency clearly earns its keep
