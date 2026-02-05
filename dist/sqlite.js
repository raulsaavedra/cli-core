import { existsSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { homedir } from "node:os";
import { Database as BunDatabase } from "bun:sqlite";
export function dbPath(appName, filename) {
    return join(homedir(), `.${appName}`, filename);
}
export function ensureDirForFile(path) {
    const dir = dirname(path);
    if (!existsSync(dir)) {
        mkdirSync(dir, { recursive: true });
    }
}
export function applyPragmas(db, pragmas) {
    for (const pragma of pragmas) {
        db.exec(`PRAGMA ${pragma}`);
    }
}
export function openSqliteDb(options) {
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
