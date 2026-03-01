# cli-core

Shared utilities for Raul’s CLI tools (Go).

## Local Development
```bash
go test ./...
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
- `Read(ReadOptions)`

### `pkg/output`
- `JSON(v)`
- `Success(format, ...)`
- `Errorf(format, ...)`
- `Fatalf(format, ...)`

### `pkg/skills`
- `ResolveSkillsDir(dest)`
- `Install(InstallOptions)`

### `pkg/sqliteutil`
- `DBPath(appName, filename)`
- `EnsureDirForFile(path)`
- `ApplyPragmas(db, pragmas)`
- `OpenSQLite(OpenOptions)`

### `pkg/stdio`
- `ReadStdin()`
