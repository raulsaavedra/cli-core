package sqliteutil

import (
	"database/sql"
	"fmt"
	"net/url"
	"os"
	"path/filepath"
	"strings"

	_ "modernc.org/sqlite"
)

type OpenOptions struct {
	AppName  string
	Filename string
	Path     string
	Pragmas  []string
	Migrate  func(*sql.DB) error
}

var defaultPragmas = []string{
	"busy_timeout = 10000",
	"foreign_keys = ON",
	"journal_mode = WAL",
}

func DBPath(appName, filename string) (string, error) {
	home, err := os.UserHomeDir()
	if err != nil {
		return "", fmt.Errorf("resolve home dir: %w", err)
	}
	return filepath.Join(home, "."+appName, filename), nil
}

func EnsureDirForFile(path string) error {
	dir := filepath.Dir(path)
	if err := os.MkdirAll(dir, 0o755); err != nil {
		return fmt.Errorf("create directory %s: %w", dir, err)
	}
	return nil
}

func OpenSQLite(opts OpenOptions) (*sql.DB, string, error) {
	if opts.AppName == "" {
		return nil, "", fmt.Errorf("app name is required")
	}
	if opts.Filename == "" {
		return nil, "", fmt.Errorf("filename is required")
	}

	dbPath := opts.Path
	if dbPath == "" {
		var err error
		dbPath, err = DBPath(opts.AppName, opts.Filename)
		if err != nil {
			return nil, "", err
		}
	}

	if err := EnsureDirForFile(dbPath); err != nil {
		return nil, "", err
	}

	db, err := sql.Open("sqlite", buildSQLiteDSN(dbPath, mergePragmas(defaultPragmas, opts.Pragmas)))
	if err != nil {
		return nil, "", fmt.Errorf("open sqlite %s: %w", dbPath, err)
	}
	db.SetMaxOpenConns(1)
	db.SetMaxIdleConns(1)

	if opts.Migrate != nil {
		if err := opts.Migrate(db); err != nil {
			_ = db.Close()
			return nil, "", fmt.Errorf("migrate %s: %w", dbPath, err)
		}
	}

	return db, dbPath, nil
}

func ApplyPragmas(db *sql.DB, pragmas []string) error {
	for _, pragma := range pragmas {
		if pragma == "" {
			continue
		}
		if _, err := db.Exec("PRAGMA " + pragma); err != nil {
			return fmt.Errorf("apply pragma %q: %w", pragma, err)
		}
	}
	return nil
}

func buildSQLiteDSN(dbPath string, pragmas []string) string {
	u := &url.URL{
		Scheme: "file",
		Path:   filepath.ToSlash(dbPath),
	}
	query := u.Query()
	for _, pragma := range pragmas {
		if pragma == "" {
			continue
		}
		query.Add("_pragma", pragma)
	}
	u.RawQuery = query.Encode()
	return u.String()
}

func mergePragmas(defaults, custom []string) []string {
	type pragmaEntry struct {
		key   string
		value string
	}

	order := []string{}
	entries := map[string]pragmaEntry{}
	appendEntry := func(pragma string) {
		trimmed := strings.TrimSpace(pragma)
		if trimmed == "" {
			return
		}
		key := pragmaKey(trimmed)
		if _, exists := entries[key]; !exists {
			order = append(order, key)
		}
		entries[key] = pragmaEntry{key: key, value: trimmed}
	}

	for _, pragma := range defaults {
		appendEntry(pragma)
	}
	for _, pragma := range custom {
		appendEntry(pragma)
	}

	merged := make([]string, 0, len(order))
	for _, key := range order {
		merged = append(merged, entries[key].value)
	}
	return merged
}

func pragmaKey(pragma string) string {
	normalized := strings.TrimSpace(strings.ToLower(pragma))
	for _, sep := range []string{"=", "("} {
		if idx := strings.Index(normalized, sep); idx >= 0 {
			return strings.TrimSpace(normalized[:idx])
		}
	}
	return normalized
}
