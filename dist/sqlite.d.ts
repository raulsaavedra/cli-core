import type { Database } from "bun:sqlite";
export interface SqliteOpenOptions {
    appName: string;
    filename: string;
    path?: string;
    migrate?: (db: Database) => void;
    pragmas?: string[];
}
export declare function dbPath(appName: string, filename: string): string;
export declare function ensureDirForFile(path: string): void;
export declare function applyPragmas(db: Database, pragmas: string[]): void;
export declare function openSqliteDb(options: SqliteOpenOptions): Database;
