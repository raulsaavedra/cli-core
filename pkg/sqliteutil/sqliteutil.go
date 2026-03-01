package sqliteutil

import (
	"database/sql"
	"fmt"
	"os"
	"path/filepath"

	_ "modernc.org/sqlite"
)

type OpenOptions struct {
	AppName  string
	Filename string
	Path     string
	Pragmas  []string
	Migrate  func(*sql.DB) error
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

	db, err := sql.Open("sqlite", dbPath)
	if err != nil {
		return nil, "", fmt.Errorf("open sqlite %s: %w", dbPath, err)
	}

	if err := ApplyPragmas(db, opts.Pragmas); err != nil {
		_ = db.Close()
		return nil, "", err
	}

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
