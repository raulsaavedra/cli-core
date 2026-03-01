# cli-core

Shared utilities for Raul’s CLI tools (Bun/TypeScript).

## Local Development
```bash
bun install
bun run check
```

## Modules

### `pkg/args`
- `ParseIntOption(value, label)`
- `RequireExactlyOne(values, message)`

### `pkg/format`
- `Date(value)`
- `DateTime(value)`
- `Truncate(text, max)`

### `pkg/jsoninput`
- `Read(ReadOptions): string`
  - Returns validated raw JSON text.
  - Returns `"null"` when `AllowEmpty` is `true` and input is empty.

### `pkg/output`
- `JSON(v)`
- `Success(format, ...)`
- `Errorf(format, ...)`
- `Fatalf(format, ...): never` (throws `Error`)

### `pkg/skills`
- `ResolveSkillsDir(dest)`
- `Install(InstallOptions)`

### `pkg/sqliteutil`
- `DBPath(appName, filename)`
- `EnsureDirForFile(path)`
- `ApplyPragmas(db, pragmas)`
- `OpenSQLite(OpenOptions): [Database, string]`

### `pkg/stdio`
- `ReadStdin()`
