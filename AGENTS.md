# cli-core

Purpose:
- Shared, reusable building blocks for Raul’s local CLIs (Bun/TS).
- Consolidate common CLI patterns (SQLite bootstrap, output, JSON input).

Scope:
- Generic helpers only (no domain rules, no CLI commands, no database schemas).
- Modules must stay app-agnostic and work across multiple CLIs.

Non-goals:
- No business logic, no project-specific formatting, no network calls.
- No direct CLI wiring or Commander command definitions.

API policy:
- ESM modules only; avoid default exports.
- New exports must be added to `src/index.ts` and documented in README.
- Prefer small, composable functions over large helpers.

Error policy:
- Helpers should throw `Error` and let callers decide how to render/exit.
- The only allowed process exit is `output.error()` (centralized CLI behavior).

Dependencies:
- Avoid runtime dependencies; use Node/Bun stdlib only.
- Types can use `@types/bun` and `@types/node`.

Compatibility:
- Works with Bun runtime and Bun/TS toolchain.
- Keep APIs stable; use semver for changes (patch for fixes, minor for new exports).
