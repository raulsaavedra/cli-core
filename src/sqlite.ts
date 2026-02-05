import type { Database } from "bun:sqlite";
import { existsSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { homedir } from "node:os";
import { Database as BunDatabase } from "bun:sqlite";

export interface SqliteOpenOptions {
  appName: string;
  filename: string;
  path?: string;
  migrate?: (db: Database) => void;
  pragmas?: string[];
}

export function dbPath(appName: string, filename: string): string {
  return join(homedir(), `.${appName}`, filename);
}

export function ensureDirForFile(path: string): void {
  const dir = dirname(path);
  if (!existsSync(dir)) {
    mkdirSync(dir, { recursive: true });
  }
}

export function applyPragmas(db: Database, pragmas: string[]): void {
  for (const pragma of pragmas) {
    db.exec(`PRAGMA ${pragma}`);
  }
}

export function openSqliteDb(options: SqliteOpenOptions): Database {
  const path = options.path ?? dbPath(options.appName, options.filename);
  ensureDirForFile(path);

  const db = new BunDatabase(path);
  if (options.pragmas && options.pragmas.length > 0) {
    applyPragmas(db, options.pragmas);
  }
  if (options.migrate) {
    options.migrate(db);
  }

  return db;
}
