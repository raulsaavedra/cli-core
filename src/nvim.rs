use serde::Serialize;
use serde_json::{Map, Value};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitStatus, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

pub const HANDOFF_ENV: &str = "NVIM_HANDOFF";
pub const QUIT_CWD_ENV: &str = "NVIM_QUIT_CWD_FILE";

#[derive(Debug)]
pub struct NvimQuitCwd {
    path: PathBuf,
    owned: bool,
    previous: Option<OsString>,
}

#[derive(Debug, Clone, Serialize)]
pub struct NvimHandoff {
    pub version: u8,
    pub source: String,
    pub action: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub targets: Vec<NvimTarget>,
    #[serde(default, skip_serializing_if = "Map::is_empty")]
    pub context: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize)]
pub struct NvimTarget {
    pub path: String,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

impl NvimHandoff {
    pub fn new(source: impl Into<String>, action: impl Into<String>) -> Self {
        Self {
            version: 1,
            source: source.into(),
            action: action.into(),
            cwd: None,
            targets: Vec::new(),
            context: Map::new(),
        }
    }

    pub fn cwd(mut self, cwd: impl AsRef<Path>) -> Self {
        self.cwd = Some(cwd.as_ref().to_string_lossy().into_owned());
        self
    }

    pub fn target(mut self, target: NvimTarget) -> Self {
        self.targets.push(target);
        self
    }

    pub fn context(mut self, key: impl Into<String>, value: impl Serialize) -> Self {
        self.context.insert(
            key.into(),
            serde_json::to_value(value).expect("failed to serialize Neovim handoff context"),
        );
        self
    }
}

impl NvimTarget {
    pub fn file(path: impl AsRef<Path>) -> Self {
        Self {
            path: path.as_ref().to_string_lossy().into_owned(),
            kind: "file".to_string(),
            line: None,
            label: None,
        }
    }

    pub fn directory(path: impl AsRef<Path>) -> Self {
        Self {
            path: path.as_ref().to_string_lossy().into_owned(),
            kind: "directory".to_string(),
            line: None,
            label: None,
        }
    }

    pub fn line(mut self, line: usize) -> Self {
        if line > 0 {
            self.line = Some(line);
        }
        self
    }

    pub fn label(mut self, label: impl Into<String>) -> Self {
        self.label = Some(label.into());
        self
    }
}

pub fn write_handoff_file(handoff: &NvimHandoff) -> Result<PathBuf, String> {
    let path = handoff_file_path();
    let body = serde_json::to_string_pretty(handoff)
        .map_err(|error| format!("serialize Neovim handoff: {error}"))?;
    fs::write(&path, body).map_err(|error| format!("write {}: {error}", path.display()))?;
    Ok(path)
}

pub fn launch_handoff(handoff: &NvimHandoff) -> Result<ExitStatus, String> {
    let handoff_path = write_handoff_file(handoff)?;
    let mut command = Command::new("nvim");
    command.env(HANDOFF_ENV, &handoff_path);
    // Open the first file target on nvim's command line so it boots straight
    // into the buffer instead of painting the empty start screen for a frame
    // before the VimEnter handler edits it. Directory/explore handoffs keep the
    // bare launch so the custom explorer owns the first screen.
    if let Some(target) = handoff_argv_target(handoff) {
        let path = absolutize(&target.path, handoff.cwd.as_deref());
        if let Some(line) = target.line {
            command.arg(format!("+{line}"));
        }
        command.arg(path);
    }
    let status = command
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|error| format!("open nvim: {error}"));
    let _ = fs::remove_file(&handoff_path);
    status
}

/// First target eligible to be opened directly on nvim's command line: a file
/// target, unless the handoff is an explorer action (which owns its own screen).
fn handoff_argv_target(handoff: &NvimHandoff) -> Option<&NvimTarget> {
    if handoff.action == "explore" {
        return None;
    }
    handoff
        .targets
        .first()
        .filter(|target| target.kind == "file")
}

/// Resolve a possibly-relative target path against the handoff cwd so nvim
/// opens it regardless of the terminal's working directory.
fn absolutize(path: &str, cwd: Option<&str>) -> PathBuf {
    let path = Path::new(path);
    if path.is_absolute() {
        return path.to_path_buf();
    }
    match cwd {
        Some(base) if !base.is_empty() => Path::new(base).join(path),
        _ => path.to_path_buf(),
    }
}

impl NvimQuitCwd {
    pub fn ensure() -> Result<Self, String> {
        let previous = std::env::var_os(QUIT_CWD_ENV);
        if let Some(path) = previous.as_ref().and_then(non_empty_os_path) {
            return Ok(Self {
                path,
                owned: false,
                previous: None,
            });
        }

        let path = quit_cwd_file_path();
        fs::File::create(&path).map_err(|error| {
            format!(
                "create Neovim quit cwd handoff file {}: {error}",
                path.display()
            )
        })?;
        std::env::set_var(QUIT_CWD_ENV, &path);
        Ok(Self {
            path,
            owned: true,
            previous,
        })
    }

    pub fn requested(&self) -> bool {
        quit_to_terminal_requested_at(&self.path)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for NvimQuitCwd {
    fn drop(&mut self) {
        if self.owned {
            if let Some(previous) = self.previous.take() {
                std::env::set_var(QUIT_CWD_ENV, previous);
            } else {
                std::env::remove_var(QUIT_CWD_ENV);
            }
            let _ = fs::remove_file(&self.path);
        }
    }
}

pub fn quit_to_terminal_requested() -> bool {
    std::env::var(QUIT_CWD_ENV)
        .ok()
        .filter(|path| !path.trim().is_empty())
        .map(|path| quit_to_terminal_requested_at(Path::new(&path)))
        .unwrap_or(false)
}

pub fn editor_is_neovim(editor: &str) -> bool {
    editor
        .split_whitespace()
        .next()
        .and_then(|part| Path::new(part).file_name())
        .map(|name| name == "nvim")
        .unwrap_or(false)
}

fn handoff_file_path() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    std::env::temp_dir().join(format!("nvim-handoff-{}-{nanos}.json", std::process::id()))
}

fn quit_cwd_file_path() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    std::env::temp_dir().join(format!("nvim-quit-cwd-{}-{nanos}", std::process::id()))
}

fn quit_to_terminal_requested_at(path: &Path) -> bool {
    fs::metadata(path)
        .map(|metadata| metadata.len() > 0)
        .unwrap_or(false)
}

fn non_empty_os_path(value: &OsString) -> Option<PathBuf> {
    if value.is_empty() {
        return None;
    }
    Some(PathBuf::from(value))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    static ENV_LOCK: Mutex<()> = Mutex::new(());

    struct EnvGuard {
        key: &'static str,
        old: Option<String>,
    }

    impl EnvGuard {
        fn set_path(key: &'static str, path: &Path) -> Self {
            let old = std::env::var(key).ok();
            std::env::set_var(key, path);
            Self { key, old }
        }

        fn remove(key: &'static str) -> Self {
            let old = std::env::var(key).ok();
            std::env::remove_var(key);
            Self { key, old }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            if let Some(value) = self.old.take() {
                std::env::set_var(self.key, value);
            } else {
                std::env::remove_var(self.key);
            }
        }
    }

    #[test]
    fn handoff_serializes_source_action_cwd_targets_and_context() {
        let handoff = NvimHandoff::new("diff", "edit")
            .cwd("/repo")
            .target(NvimTarget::file("/repo/src/main.rs").line(42).label("main"))
            .context("issue_token", "AUTH-12");

        let json = serde_json::to_value(&handoff).unwrap();

        assert_eq!(json["version"], 1);
        assert_eq!(json["source"], "diff");
        assert_eq!(json["action"], "edit");
        assert_eq!(json["cwd"], "/repo");
        assert_eq!(json["targets"][0]["path"], "/repo/src/main.rs");
        assert_eq!(json["targets"][0]["line"], 42);
        assert_eq!(json["context"]["issue_token"], "AUTH-12");
    }

    #[test]
    fn target_line_omits_zero() {
        let target = serde_json::to_value(NvimTarget::file("src/main.rs").line(0)).unwrap();

        assert!(target.get("line").is_none());
    }

    #[test]
    fn editor_detection_accepts_path_to_nvim() {
        assert!(editor_is_neovim("nvim"));
        assert!(editor_is_neovim("/opt/homebrew/bin/nvim -O"));
        assert!(!editor_is_neovim("vim"));
    }

    #[test]
    fn quit_to_terminal_requested_requires_non_empty_handoff_file() {
        let _guard = ENV_LOCK.lock().unwrap();
        let path = handoff_file_path();
        let _ = fs::remove_file(&path);
        let _env = EnvGuard::set_path(QUIT_CWD_ENV, &path);

        assert!(!quit_to_terminal_requested());

        fs::write(&path, "").unwrap();
        assert!(!quit_to_terminal_requested());

        fs::write(&path, "/repo").unwrap();
        assert!(quit_to_terminal_requested());

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn quit_cwd_guard_creates_signal_file_when_env_is_absent() {
        let _guard = ENV_LOCK.lock().unwrap();
        let _env = EnvGuard::remove(QUIT_CWD_ENV);
        let path;

        {
            let quit_cwd = NvimQuitCwd::ensure().unwrap();
            path = quit_cwd.path().to_path_buf();

            assert_eq!(
                std::env::var(QUIT_CWD_ENV).unwrap(),
                path.display().to_string()
            );
            assert!(!quit_cwd.requested());

            fs::write(&path, "/repo").unwrap();
            assert!(quit_cwd.requested());
        }

        assert!(std::env::var(QUIT_CWD_ENV).is_err());
        assert!(!path.exists());
    }

    #[test]
    fn quit_cwd_guard_reuses_existing_signal_file() {
        let _guard = ENV_LOCK.lock().unwrap();
        let path = quit_cwd_file_path();
        fs::write(&path, "").unwrap();
        let _env = EnvGuard::set_path(QUIT_CWD_ENV, &path);

        {
            let quit_cwd = NvimQuitCwd::ensure().unwrap();

            assert_eq!(quit_cwd.path(), path.as_path());
            assert!(!quit_cwd.requested());

            fs::write(&path, "/repo").unwrap();
            assert!(quit_cwd.requested());
        }

        assert_eq!(
            std::env::var(QUIT_CWD_ENV).unwrap(),
            path.display().to_string()
        );
        assert!(path.exists());
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn argv_target_is_first_file_for_edit_action() {
        let handoff = NvimHandoff::new("diff", "edit")
            .cwd("/repo")
            .target(NvimTarget::file("src/main.rs").line(7));
        let target = handoff_argv_target(&handoff).expect("file target");
        assert_eq!(target.path, "src/main.rs");
        assert_eq!(target.line, Some(7));
    }

    #[test]
    fn argv_target_skips_explore_action() {
        let handoff = NvimHandoff::new("diff", "explore")
            .target(NvimTarget::directory("/repo"))
            .target(NvimTarget::file("/repo/src/main.rs"));
        assert!(handoff_argv_target(&handoff).is_none());
    }

    #[test]
    fn argv_target_skips_directory_first_target() {
        let handoff =
            NvimHandoff::new("tickets", "worktree").target(NvimTarget::directory("/repo"));
        assert!(handoff_argv_target(&handoff).is_none());
    }

    #[test]
    fn absolutize_joins_relative_against_cwd() {
        assert_eq!(
            absolutize("src/main.rs", Some("/repo")),
            PathBuf::from("/repo/src/main.rs")
        );
    }

    #[test]
    fn absolutize_keeps_absolute_and_handles_missing_cwd() {
        assert_eq!(
            absolutize("/abs/main.rs", Some("/repo")),
            PathBuf::from("/abs/main.rs")
        );
        assert_eq!(
            absolutize("src/main.rs", None),
            PathBuf::from("src/main.rs")
        );
        assert_eq!(
            absolutize("src/main.rs", Some("")),
            PathBuf::from("src/main.rs")
        );
    }
}
