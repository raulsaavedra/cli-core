#[cfg(feature = "activity")]
pub mod activity;
#[cfg(feature = "ansi")]
pub mod ansi;
#[cfg(feature = "claude")]
pub mod claude;
#[cfg(feature = "markdown")]
pub mod markdown;
#[cfg(feature = "nvim")]
pub mod nvim;
#[cfg(feature = "output")]
pub mod output;
#[cfg(all(test, any(feature = "claude", feature = "worktrees")))]
mod scratch;
#[cfg(feature = "sqlite")]
pub mod sqlite;
pub mod stdio;
#[cfg(feature = "worktrees")]
pub mod worktrees;

#[cfg(feature = "ansi")]
pub use ansi::{parse_line, parse_lines};
#[cfg(feature = "nvim")]
pub use nvim::{
    editor_is_neovim, launch_handoff, quit_to_terminal_requested, write_handoff_file, NvimHandoff,
    NvimQuitCwd, NvimTarget, HANDOFF_ENV, QUIT_CWD_ENV,
};
#[cfg(feature = "output")]
pub use output::{errorf, json, success};
#[cfg(feature = "sqlite")]
pub use sqlite::{apply_pragmas, db_path, ensure_dir_for_file, open_sqlite, OpenOptions};
pub use stdio::read_stdin;
