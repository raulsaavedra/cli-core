# cli-core

Shared utilities for Raul’s CLI tools (Go).

## Local Development
```bash
go test ./...
```

## Modules

### `pkg/output`
- `JSON(v)`
- `Success(format, ...)`
- `Errorf(format, ...)`

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
