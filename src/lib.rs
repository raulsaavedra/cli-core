pub mod ansi;
pub mod markdown;
mod mermaid;
pub mod nvim;
pub mod output;
pub mod skills;
pub mod sqlite;
pub mod stdio;

pub use ansi::{parse_line, parse_lines};
pub use markdown::{render, render_with_viewport, Heading, Link, RenderResult};
pub use nvim::{
    editor_is_neovim, launch_handoff, quit_to_terminal_requested, write_handoff_file, NvimHandoff,
    NvimQuitCwd, NvimTarget, HANDOFF_ENV, QUIT_CWD_ENV,
};
pub use output::{errorf, json, success};
pub use skills::{install, resolve_default_skills_dirs, resolve_skills_dir, InstallOptions};
pub use sqlite::{apply_pragmas, db_path, ensure_dir_for_file, open_sqlite, OpenOptions};
pub use stdio::read_stdin;
