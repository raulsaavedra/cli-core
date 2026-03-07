package sqliteutil

import (
	"database/sql"
	"path/filepath"
	"testing"

	_ "modernc.org/sqlite"
)

func TestOpenSQLiteAppliesDefaultPragmas(t *testing.T) {
	t.Parallel()

	dbPath := filepath.Join(t.TempDir(), "default-pragmas.db")
	db, _, err := OpenSQLite(OpenOptions{
		AppName:  "sqliteutil-test",
		Filename: "default-pragmas.db",
		Path:     dbPath,
	})
	if err != nil {
		t.Fatalf("OpenSQLite: %v", err)
	}
	t.Cleanup(func() { _ = db.Close() })

	assertPragmaInt(t, db, "foreign_keys", 1)
	assertPragmaInt(t, db, "busy_timeout", 10000)
	assertPragmaText(t, db, "journal_mode", "wal")
	assertDBPoolUnlimited(t, db)
}

func TestOpenSQLiteAllowsPragmaOverrides(t *testing.T) {
	t.Parallel()

	dbPath := filepath.Join(t.TempDir(), "override-pragmas.db")
	db, _, err := OpenSQLite(OpenOptions{
		AppName:  "sqliteutil-test",
		Filename: "override-pragmas.db",
		Path:     dbPath,
		Pragmas: []string{
			"busy_timeout = 2500",
			"foreign_keys = OFF",
		},
	})
	if err != nil {
		t.Fatalf("OpenSQLite: %v", err)
	}
	t.Cleanup(func() { _ = db.Close() })

	assertPragmaInt(t, db, "busy_timeout", 2500)
	assertPragmaInt(t, db, "foreign_keys", 0)
	assertPragmaText(t, db, "journal_mode", "wal")
}

func assertPragmaInt(t *testing.T, db *sql.DB, pragma string, want int) {
	t.Helper()

	var got int
	if err := db.QueryRow("PRAGMA " + pragma).Scan(&got); err != nil {
		t.Fatalf("read PRAGMA %s: %v", pragma, err)
	}
	if got != want {
		t.Fatalf("PRAGMA %s = %d, want %d", pragma, got, want)
	}
}

func assertPragmaText(t *testing.T, db *sql.DB, pragma, want string) {
	t.Helper()

	var got string
	if err := db.QueryRow("PRAGMA " + pragma).Scan(&got); err != nil {
		t.Fatalf("read PRAGMA %s: %v", pragma, err)
	}
	if got != want {
		t.Fatalf("PRAGMA %s = %q, want %q", pragma, got, want)
	}
}

func assertDBPoolUnlimited(t *testing.T, db *sql.DB) {
	t.Helper()

	stats := db.Stats()
	if stats.MaxOpenConnections != 0 {
		t.Fatalf("MaxOpenConnections = %d, want 0 (unlimited)", stats.MaxOpenConnections)
	}
}
