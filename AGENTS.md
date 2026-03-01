# cli-core

Purpose:
- Shared, reusable building blocks for Raul’s local CLIs (Bun/TypeScript).
- Consolidate common CLI patterns (SQLite bootstrap, output, JSON input).

Scope:
- Generic helpers only (no domain rules, no CLI commands, no database schemas).
- Modules must stay app-agnostic and work across multiple CLIs.

Non-goals:
- No business logic, no project-specific formatting, no network calls.
- No direct CLI wiring or Cobra command definitions.

API policy:
- Keep packages in `pkg/*`.
- New exports must be documented in README.
- Prefer small, composable functions over large helpers.

Error policy:
- Helpers should throw `Error` and let callers decide how to render/exit.
- Avoid calling `os.Exit` in shared packages.

Dependencies:
- Keep dependencies minimal.
- Prefer Bun and Node stdlib where possible.

Compatibility:
- Works on Linux/macOS with Bun 1.2+.
- Keep APIs stable for downstream CLIs.
