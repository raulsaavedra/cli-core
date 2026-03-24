use std::io::{self, Write};

/// JSON-encode a value to stdout with 2-space indentation (matches Go json.Encoder).
pub fn json<T: serde::Serialize>(value: &T) {
    let buf = serde_json::to_string_pretty(value).expect("failed to serialize JSON");
    let stdout = io::stdout();
    let mut handle = stdout.lock();
    let _ = writeln!(handle, "{buf}");
}

/// Write a formatted message to stdout with a trailing newline.
pub fn success(msg: &str) {
    let stdout = io::stdout();
    let mut handle = stdout.lock();
    let _ = writeln!(handle, "{msg}");
}

/// Write a formatted error message to stderr with a trailing newline.
pub fn errorf(msg: &str) {
    let stderr = io::stderr();
    let mut handle = stderr.lock();
    let _ = writeln!(handle, "{msg}");
}
