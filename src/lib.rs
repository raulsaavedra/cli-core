pub mod ansi;
pub mod markdown;
mod mermaid;
pub mod output;
pub mod skills;
pub mod sqlite;
pub mod stdio;

pub use ansi::{parse_line, parse_lines};
pub use markdown::{render, render_with_viewport, Heading, Link, RenderResult};
pub use output::{errorf, json, success};
pub use skills::{install, resolve_default_skills_dirs, resolve_skills_dir, InstallOptions};
pub use sqlite::{apply_pragmas, db_path, ensure_dir_for_file, open_sqlite, OpenOptions};
pub use stdio::read_stdin;
