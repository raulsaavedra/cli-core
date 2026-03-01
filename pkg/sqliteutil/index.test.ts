import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";

import { describe, expect, test } from "bun:test";

import { DBPath, EnsureDirForFile, OpenSQLite } from "./index.ts";

describe("DBPath", () => {
  test("builds path under home directory", () => {
    expect(DBPath("myapp", "state.db")).toContain("/.myapp/state.db");
  });
});

describe("EnsureDirForFile", () => {
  test("creates missing directories", () => {
    const dir = mkdtempSync(join(tmpdir(), "cli-core-sqliteutil-"));
    try {
      const path = join(dir, "a", "b", "state.db");
      expect(() => EnsureDirForFile(path)).not.toThrow();
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });
});

describe("OpenSQLite", () => {
  test("opens sqlite, applies pragmas, and runs migration", () => {
    const dir = mkdtempSync(join(tmpdir(), "cli-core-sqlite-"));
    try {
      const dbFile = join(dir, "data", "state.db");
      const [db, path] = OpenSQLite({
        AppName: "ignored-with-explicit-path",
        Filename: "ignored.db",
        Path: dbFile,
        Pragmas: ["journal_mode=WAL"],
        Migrate: (client) => {
          client.exec(
            "CREATE TABLE IF NOT EXISTS items (id INTEGER PRIMARY KEY)",
          );
        },
      });

      expect(path).toBe(dbFile);
      const row = db
        .query(
          "SELECT name FROM sqlite_master WHERE type = 'table' AND name = 'items'",
        )
        .get() as { name: string } | null;
      expect(row?.name).toBe("items");

      db.close();
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });

  test("validates required options", () => {
    expect(() => OpenSQLite({ AppName: "", Filename: "state.db" })).toThrow(
      "app name is required",
    );
    expect(() => OpenSQLite({ AppName: "app", Filename: "" })).toThrow(
      "filename is required",
    );
  });
});
