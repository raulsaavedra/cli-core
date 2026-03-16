# cli-core

Shared Go packages for building local and agent-friendly CLIs.

`cli-core` is intentionally small. It provides a few reusable building blocks that showed up across multiple command-line tools:

- `pkg/output` for JSON and human-readable terminal output
- `pkg/skills` for installing agent skill directories into `~/.claude/skills` or another destination
- `pkg/sqliteutil` for opening local SQLite databases with sensible defaults for CLI workloads
- `pkg/stdio` for reading piped stdin content
- `pkg/termmd` for rendering Markdown in the terminal while preserving plain-text metadata

The repo is best thought of as an opinionated utility module for local-first CLIs, not a full CLI framework.

## Install

```bash
go get github.com/raulsaavedra/cli-core@latest
```

## Packages

### `pkg/output`

Small helpers for writing machine-readable and human-readable output.

- `JSON(v any) error`
- `Success(format string, args ...any)`
- `Errorf(format string, args ...any)`

### `pkg/skills`

Helpers for installing a skill directory into an agent skills folder.

- `ResolveSkillsDir(dest string) (string, error)`
- `Install(InstallOptions) (string, error)`

If `dest` is empty, `ResolveSkillsDir` defaults to `~/.claude/skills`.

### `pkg/sqliteutil`

Helpers for file-backed SQLite databases used by local CLIs.

- `DBPath(appName, filename string) (string, error)`
- `EnsureDirForFile(path string) error`
- `ApplyPragmas(db *sql.DB, pragmas []string) error`
- `OpenSQLite(opts OpenOptions) (*sql.DB, string, error)`

`OpenSQLite` applies the following defaults for file-backed databases:

- `busy_timeout = 10000`
- `foreign_keys = ON`
- `journal_mode = WAL`
- `MaxOpenConns = 1`
- `MaxIdleConns = 1`

That combination is tuned for local CLI processes that may open the same SQLite database concurrently and should wait instead of failing fast with `SQLITE_BUSY`.

Caller-provided `OpenOptions.Pragmas` are merged on top of the defaults by pragma name, so explicit overrides still win.

Example:

```go
db, path, err := sqliteutil.OpenSQLite(sqliteutil.OpenOptions{
	AppName:  "myapp",
	Filename: "state.db",
	Migrate: func(db *sql.DB) error {
		_, err := db.Exec(`create table if not exists items (id integer primary key, name text not null)`)
		return err
	},
})
if err != nil {
	return err
}
defer db.Close()

fmt.Println("db path:", path)
```

### `pkg/stdio`

Simple stdin helper for CLIs that accept piped or redirected input.

- `ReadStdin() (string, error)`

### `pkg/termmd`

Terminal Markdown rendering with extracted metadata for downstream navigation and indexing.

- `Render(content string, width int) Result`

`Result` includes:

- rendered terminal output
- normalized rendered lines
- plain-text lines with ANSI stripped
- heading metadata with line positions
- extracted links

Example:

```go
result := termmd.Render("# Title\n\nSee [docs](https://example.com).", 80)
fmt.Println(result.Rendered)
fmt.Println(result.Headings[0].Text)
fmt.Println(result.Links[0].Href)
```

## Development

```bash
go test ./...
```

## Scope

`cli-core` should stay small and app-agnostic.

Included:

- reusable helpers for local CLI infrastructure
- local SQLite bootstrapping
- terminal rendering utilities
- skill-install helpers for agent workflows

Not included:

- app-specific business logic
- command wiring for a particular CLI
- network services or remote API clients
- database schemas tied to one application
