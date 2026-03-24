use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use rusqlite::Connection;

/// Options for opening a SQLite database.
pub struct OpenOptions {
    /// Application name (used to derive the default path as `~/.{app_name}/`).
    pub app_name: String,
    /// Database filename.
    pub filename: String,
    /// Explicit path; when set, `app_name`/`filename` are only used for error messages.
    pub path: Option<String>,
    /// Extra pragmas — override defaults by key (e.g. "busy_timeout = 5000").
    pub pragmas: Vec<String>,
    /// Optional migration callback run after opening & pragma setup.
    pub migrate: Option<fn(&Connection) -> Result<(), String>>,
}

const DEFAULT_PRAGMAS: &[&str] = &[
    "busy_timeout = 10000",
    "foreign_keys = ON",
    "journal_mode = WAL",
];

/// Return the conventional path `~/.{app_name}/{filename}`.
pub fn db_path(app_name: &str, filename: &str) -> PathBuf {
    let home = dirs::home_dir().expect("home directory not found");
    home.join(format!(".{app_name}")).join(filename)
}

/// Ensure the parent directory of `path` exists.
pub fn ensure_dir_for_file(path: &Path) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).ok();
    }
}

/// Open (or create) a SQLite database with sensible CLI defaults.
///
/// Default pragmas: busy_timeout 10 s, foreign_keys ON, journal_mode WAL.
/// Custom pragmas in `opts.pragmas` override defaults by key.
pub fn open_sqlite(opts: &OpenOptions) -> Result<(Connection, PathBuf), String> {
    if opts.app_name.is_empty() {
        return Err("app name is required".into());
    }
    if opts.filename.is_empty() {
        return Err("filename is required".into());
    }

    let resolved_path = match &opts.path {
        Some(p) => PathBuf::from(p),
        None => db_path(&opts.app_name, &opts.filename),
    };

    ensure_dir_for_file(&resolved_path);

    let db = Connection::open(&resolved_path)
        .map_err(|e| format!("open {}: {e}", resolved_path.display()))?;

    let defaults: Vec<String> = DEFAULT_PRAGMAS.iter().map(|s| (*s).to_string()).collect();
    let pragmas = merge_pragmas(&defaults, &opts.pragmas);
    apply_pragmas(&db, &pragmas);

    if let Some(migrate) = opts.migrate {
        migrate(&db).map_err(|e| format!("migrate {}: {e}", resolved_path.display()))?;
    }

    Ok((db, resolved_path))
}

/// Execute a list of PRAGMA statements on an open database.
pub fn apply_pragmas(db: &Connection, pragmas: &[String]) {
    for pragma in pragmas {
        let trimmed = pragma.trim();
        if trimmed.is_empty() {
            continue;
        }
        let _ = db.execute_batch(&format!("PRAGMA {trimmed}"));
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Merge default pragmas with custom ones. Custom pragmas override by key.
fn merge_pragmas(defaults: &[String], custom: &[String]) -> Vec<String> {
    let mut order: Vec<String> = Vec::new();
    let mut entries: HashMap<String, String> = HashMap::new();

    let mut append = |pragma: &str| {
        let trimmed = pragma.trim();
        if trimmed.is_empty() {
            return;
        }
        let key = pragma_key(trimmed);
        if !entries.contains_key(&key) {
            order.push(key.clone());
        }
        entries.insert(key, trimmed.to_string());
    };

    for p in defaults {
        append(p);
    }
    for p in custom {
        append(p);
    }

    order
        .iter()
        .filter_map(|k| entries.get(k).cloned())
        .collect()
}

/// Extract the pragma key (everything before `=` or `(`), lowercased and trimmed.
fn pragma_key(pragma: &str) -> String {
    let normalized = pragma.trim().to_lowercase();
    for sep in ['=', '('] {
        if let Some(idx) = normalized.find(sep) {
            return normalized[..idx].trim().to_string();
        }
    }
    normalized
}
