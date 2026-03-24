use std::io::{self, Read};

/// Read all of stdin and return the trimmed content.
pub fn read_stdin() -> io::Result<String> {
    let mut buf = String::new();
    io::stdin().read_to_string(&mut buf)?;
    Ok(buf.trim().to_string())
}
