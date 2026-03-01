import { mkdirSync } from "node:fs";
import { homedir } from "node:os";
import { dirname, join } from "node:path";

import { Database } from "bun:sqlite";

export interface OpenOptions {
  AppName: string;
  Filename: string;
  Path?: string;
  Pragmas?: string[];
  Migrate?: (db: Database) => void;
}

export function DBPath(appName: string, filename: string): string {
  const home = homedir();
  if (home === "") {
    throw new Error("resolve home dir: home directory is empty");
  }
  return join(home, `.${appName}`, filename);
}

export function EnsureDirForFile(path: string): void {
  const dir = dirname(path);
  try {
    mkdirSync(dir, { recursive: true, mode: 0o755 });
  } catch (error) {
    throw new Error(`create directory ${dir}: ${errorMessage(error)}`);
  }
}

export function OpenSQLite(opts: OpenOptions): [Database, string] {
  if (!opts.AppName) {
    throw new Error("app name is required");
  }
  if (!opts.Filename) {
    throw new Error("filename is required");
  }

  const dbPath = opts.Path || DBPath(opts.AppName, opts.Filename);
  EnsureDirForFile(dbPath);

  let db: Database;
  try {
    db = new Database(dbPath);
  } catch (error) {
    throw new Error(`open sqlite ${dbPath}: ${errorMessage(error)}`);
  }

  try {
    ApplyPragmas(db, opts.Pragmas || []);
  } catch (error) {
    db.close();
    throw error;
  }

  if (opts.Migrate) {
    try {
      opts.Migrate(db);
    } catch (error) {
      db.close();
      throw new Error(`migrate ${dbPath}: ${errorMessage(error)}`);
    }
  }

  return [db, dbPath];
}

export function ApplyPragmas(db: Database, pragmas: string[]): void {
  for (const pragma of pragmas) {
    if (pragma === "") {
      continue;
    }

    try {
      db.exec(`PRAGMA ${pragma}`);
    } catch (error) {
      throw new Error(`apply pragma "${pragma}": ${errorMessage(error)}`);
    }
  }
}

function errorMessage(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}
