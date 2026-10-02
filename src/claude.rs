//! Claude Code state across account profiles.
//!
//! Each profile is a Claude Code config directory. A running Claude Code
//! process keeps `<config dir>/sessions/<pid>.json`, and every session appends
//! to `<config dir>/projects/<slug>/<session id>.jsonl`, where the slug is the
//! session's working directory with every non-alphanumeric character turned
//! into `-`.

use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
use std::fs;
use std::io::{self, BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const TITLE_PROMPT_CHARS: usize = 200;
const CWD_SCAN_LINES: usize = 100;

/// One Claude Code account profile and the `CLAUDE_CONFIG_DIR` it runs with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Profile {
    pub name: String,
    pub config_dir: PathBuf,
}

impl Profile {
    /// Live session records Claude Code keeps per running process.
    pub fn sessions_dir(&self) -> PathBuf {
        self.config_dir.join("sessions")
    }

    /// Transcript root: one directory per working directory, one JSONL per
    /// session.
    pub fn projects_dir(&self) -> PathBuf {
        self.config_dir.join("projects")
    }
}

/// Directory holding `profiles.tsv`: `CLAUDE_PROFILE_ROOT`, or
/// `~/src/config/claude`.
pub fn profile_root() -> PathBuf {
    std::env::var("CLAUDE_PROFILE_ROOT")
        .ok()
        .map(|raw| raw.trim().to_string())
        .filter(|raw| !raw.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home().join("src/config/claude"))
}

/// Profiles in table order.
pub fn profiles() -> Vec<Profile> {
    profiles_from(&profile_root().join("profiles.tsv"))
}

fn profiles_from(path: &Path) -> Vec<Profile> {
    let Ok(contents) = fs::read_to_string(path) else {
        return Vec::new();
    };
    contents
        .lines()
        .filter(|line| !line.trim().is_empty() && !line.trim().starts_with('#'))
        .filter_map(|line| {
            let mut fields = line.split('\t').map(str::trim);
            let name = fields.next().filter(|name| !name.is_empty())?;
            let dir = fields.next().filter(|dir| !dir.is_empty())?;
            Some(Profile {
                name: name.to_string(),
                config_dir: expand_tilde(dir),
            })
        })
        .collect()
}

fn home() -> PathBuf {
    dirs::home_dir().unwrap_or_default()
}

fn expand_tilde(value: &str) -> PathBuf {
    if value == "~" {
        home()
    } else if let Some(rest) = value.strip_prefix("~/") {
        home().join(rest)
    } else {
        PathBuf::from(value)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionStatus {
    /// A turn is running.
    Busy,
    Idle,
}

/// A running Claude Code session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveSession {
    pub pid: u32,
    pub session_id: String,
    pub profile: String,
    pub cwd: PathBuf,
    pub status: SessionStatus,
    /// When `status` last changed.
    pub status_changed_at: Option<SystemTime>,
    /// tmux pane id such as `%12`, when the session runs inside tmux.
    pub tmux_pane: Option<String>,
    /// Where the session's transcript lives once it has written one.
    pub transcript: PathBuf,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionRecord {
    pid: u32,
    session_id: String,
    cwd: Option<String>,
    proc_start: Option<String>,
    tmux: Option<String>,
    status: Option<String>,
    status_updated_at: Option<u64>,
}

/// Live sessions of every profile. A record counts only while its process is
/// the one that wrote it: the pid must be alive and started at the recorded
/// `procStart`, so a record left behind by a crash never claims a reused pid.
pub fn live_sessions(profiles: &[Profile]) -> io::Result<Vec<LiveSession>> {
    let records: Vec<(&Profile, SessionRecord)> = profiles
        .iter()
        .flat_map(|profile| {
            session_records(profile)
                .into_iter()
                .map(move |r| (profile, r))
        })
        .collect();
    if records.is_empty() {
        return Ok(Vec::new());
    }
    let started = process_starts()?;
    Ok(records
        .into_iter()
        .filter(|(_, record)| {
            let recorded = record.proc_start.as_deref().map(normalize_start);
            recorded.is_some() && started.get(&record.pid) == recorded.as_ref()
        })
        .map(|(profile, record)| {
            let cwd = PathBuf::from(record.cwd.unwrap_or_default());
            LiveSession {
                pid: record.pid,
                transcript: profile
                    .projects_dir()
                    .join(project_slug(&cwd))
                    .join(format!("{}.jsonl", record.session_id)),
                session_id: record.session_id,
                profile: profile.name.clone(),
                cwd,
                status: if record.status.as_deref() == Some("busy") {
                    SessionStatus::Busy
                } else {
                    SessionStatus::Idle
                },
                status_changed_at: record.status_updated_at.map(from_millis),
                tmux_pane: record.tmux.as_deref().and_then(pane_from_tmux_field),
            }
        })
        .collect())
}

fn session_records(profile: &Profile) -> Vec<SessionRecord> {
    let Ok(entries) = fs::read_dir(profile.sessions_dir()) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
        .filter_map(|path| serde_json::from_slice(&fs::read(path).ok()?).ok())
        .collect()
}

/// Start time of every running process, keyed by pid, in the
/// `ps -o lstart` shape Claude Code records as `procStart`: UTC, to the second.
fn process_starts() -> io::Result<HashMap<u32, String>> {
    let output = Command::new("ps")
        .args(["-A", "-o", "pid=,lstart="])
        .env("TZ", "UTC")
        .env("LC_ALL", "C")
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "ps exited with {}",
            output.status
        )));
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let (pid, start) = line.trim().split_once(char::is_whitespace)?;
            Some((pid.parse().ok()?, normalize_start(start)))
        })
        .collect())
}

/// `ps` pads single-digit days with a space; compare starts word by word.
fn normalize_start(value: &str) -> String {
    value.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// `session:@window.%pane` as Claude Code records it.
fn pane_from_tmux_field(value: &str) -> Option<String> {
    let pane = value.rsplit_once('.')?.1;
    (pane.len() > 1 && pane.starts_with('%')).then(|| pane.to_string())
}

fn from_millis(millis: u64) -> SystemTime {
    UNIX_EPOCH + Duration::from_millis(millis)
}

fn project_slug(cwd: &Path) -> String {
    cwd.to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect()
}

/// Agent activity in one working directory under one profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectActivity {
    pub profile: String,
    /// The directory the sessions ran in, as their transcripts record it.
    pub cwd: PathBuf,
    /// Modification time of the newest transcript.
    pub last_active_at: SystemTime,
    /// The newest transcript.
    pub transcript: PathBuf,
}

/// Activity of every transcript directory of every profile. The directory
/// comes from the transcripts rather than the slug, which cannot be reversed.
pub fn project_activity(profiles: &[Profile]) -> Vec<ProjectActivity> {
    profiles
        .iter()
        .flat_map(|profile| {
            let dirs = fs::read_dir(profile.projects_dir())
                .into_iter()
                .flatten()
                .flatten();
            dirs.filter_map(|dir| directory_activity(&profile.name, &dir.path()))
        })
        .collect()
}

fn directory_activity(profile: &str, dir: &Path) -> Option<ProjectActivity> {
    let mut transcripts: Vec<(SystemTime, PathBuf)> = fs::read_dir(dir)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "jsonl"))
        .filter_map(|path| Some((fs::metadata(&path).ok()?.modified().ok()?, path)))
        .collect();
    transcripts.sort_by_key(|(modified, _)| std::cmp::Reverse(*modified));
    let cwd = transcripts
        .iter()
        .find_map(|(_, path)| transcript_cwd(path))?;
    let (last_active_at, transcript) = transcripts.into_iter().next()?;
    Some(ProjectActivity {
        profile: profile.to_string(),
        cwd,
        last_active_at,
        transcript,
    })
}

fn transcript_cwd(path: &Path) -> Option<PathBuf> {
    let file = fs::File::open(path).ok()?;
    BufReader::new(file)
        .lines()
        .take(CWD_SCAN_LINES)
        .map_while(Result::ok)
        .filter(|line| line.contains("\"cwd\""))
        .find_map(|line| {
            let entry: Value = serde_json::from_str(&line).ok()?;
            let cwd = entry["cwd"].as_str().filter(|cwd| !cwd.is_empty())?;
            Some(PathBuf::from(cwd))
        })
}

/// The title Claude Code shows for a session: the newest title the user set,
/// else the newest one Claude generated, else the first prompt the user typed.
pub fn session_title(transcript: &Path) -> Option<String> {
    let file = fs::File::open(transcript).ok()?;
    let mut custom_title = None;
    let mut ai_title = None;
    let mut first_prompt = None;
    for line in BufReader::new(file).lines().map_while(Result::ok) {
        let wanted = line.contains("\"custom-title\"")
            || line.contains("\"ai-title\"")
            || (first_prompt.is_none() && line.contains("\"user\""));
        if !wanted {
            continue;
        }
        let Ok(entry) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        match entry["type"].as_str() {
            Some("custom-title") => {
                custom_title = non_empty(entry["customTitle"].as_str()).or(custom_title)
            }
            Some("ai-title") => ai_title = non_empty(entry["aiTitle"].as_str()).or(ai_title),
            Some("user") if first_prompt.is_none() => {
                first_prompt = prompt_text(&entry)
                    .filter(|prompt| !prompt.starts_with('<'))
                    .map(|prompt| prompt.chars().take(TITLE_PROMPT_CHARS).collect());
            }
            _ => {}
        }
    }
    custom_title.or(ai_title).or(first_prompt)
}

/// Text of a prompt that starts a turn. Tool results and injected meta
/// messages ride on user entries too but start nothing.
fn prompt_text(entry: &Value) -> Option<String> {
    if entry["isMeta"].as_bool() == Some(true) || !entry["toolUseResult"].is_null() {
        return None;
    }
    let content = &entry["message"]["content"];
    if let Some(text) = content.as_str() {
        return non_empty(Some(text));
    }
    let blocks = content.as_array()?;
    if blocks.iter().any(|block| block["type"] == "tool_result") {
        return None;
    }
    let text = blocks
        .iter()
        .filter(|block| block["type"] == "text")
        .filter_map(|block| block["text"].as_str())
        .collect::<Vec<_>>()
        .join("\n");
    non_empty(Some(&text))
}

fn non_empty(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
pub(crate) mod fixture {
    use super::Profile;
    use std::fs;
    use std::path::Path;
    use std::process::Command;
    use std::time::SystemTime;

    /// A profile rooted at `dir` with empty sessions and projects directories.
    pub fn profile(name: &str, dir: &Path) -> Profile {
        fs::create_dir_all(dir.join("sessions")).unwrap();
        fs::create_dir_all(dir.join("projects")).unwrap();
        Profile {
            name: name.to_string(),
            config_dir: dir.to_path_buf(),
        }
    }

    /// This test process's start, in the shape Claude Code records.
    pub fn own_proc_start() -> String {
        let output = Command::new("ps")
            .args(["-o", "lstart=", "-p", &std::process::id().to_string()])
            .env("TZ", "UTC")
            .env("LC_ALL", "C")
            .output()
            .unwrap();
        String::from_utf8(output.stdout).unwrap().trim().to_string()
    }

    pub fn session_record(profile: &Profile, pid: u32, proc_start: &str, cwd: &Path, status: &str) {
        let record = serde_json::json!({
            "pid": pid,
            "sessionId": format!("s-{pid}"),
            "cwd": cwd,
            "procStart": proc_start,
            "tmux": "main:@4.%7",
            "status": status,
            "statusUpdatedAt": 1_790_268_373_805_u64,
        });
        fs::write(
            profile.sessions_dir().join(format!("{pid}.json")),
            record.to_string(),
        )
        .unwrap();
    }

    /// A transcript for `session` in `cwd`, last modified at `modified`.
    pub fn transcript(
        profile: &Profile,
        cwd: &Path,
        session: &str,
        lines: &[&str],
        modified: SystemTime,
    ) {
        let dir = profile.projects_dir().join(super::project_slug(cwd));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(format!("{session}.jsonl"));
        fs::write(&path, format!("{}\n", lines.join("\n"))).unwrap();
        fs::File::options()
            .write(true)
            .open(&path)
            .unwrap()
            .set_modified(modified)
            .unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::fixture::{own_proc_start, profile, session_record, transcript};
    use super::*;
    use crate::worktrees::fixture::scratch;

    #[test]
    fn profile_table_expands_home_and_skips_comments() {
        let dir = scratch("claude-profiles");
        let table = dir.join("profiles.tsv");
        fs::write(
            &table,
            "# profile\tconfig-dir\nwork\t~/.claude\npersonal\t/opt/claude-personal\n\nbroken\n",
        )
        .unwrap();

        let profiles = profiles_from(&table);

        assert_eq!(
            profiles,
            vec![
                Profile {
                    name: "work".into(),
                    config_dir: home().join(".claude"),
                },
                Profile {
                    name: "personal".into(),
                    config_dir: PathBuf::from("/opt/claude-personal"),
                },
            ]
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn live_sessions_need_a_running_pid_started_when_recorded() {
        let dir = scratch("claude-live");
        let profile = profile("work", &dir);
        let pid = std::process::id();
        let cwd = Path::new("/work/my.repo");
        session_record(&profile, pid, &own_proc_start(), cwd, "busy");
        session_record(&profile, 1, "Thu Jan  1 00:00:00 1970", cwd, "idle");
        session_record(&profile, 4_194_305, "Thu Sep 24 16:41:55 2026", cwd, "busy");
        fs::write(profile.sessions_dir().join("9.abc.key"), "ignored").unwrap();

        let sessions = live_sessions(&[profile]).unwrap();

        assert_eq!(
            sessions,
            vec![LiveSession {
                pid,
                session_id: format!("s-{pid}"),
                profile: "work".into(),
                cwd: cwd.to_path_buf(),
                status: SessionStatus::Busy,
                status_changed_at: Some(UNIX_EPOCH + Duration::from_millis(1_790_268_373_805)),
                tmux_pane: Some("%7".into()),
                transcript: dir.join(format!("projects/-work-my-repo/s-{pid}.jsonl")),
            }]
        );
        fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn pane_comes_from_the_tail_of_the_tmux_field() {
        assert_eq!(
            pane_from_tmux_field("xepelin:@448.%508").as_deref(),
            Some("%508")
        );
        assert_eq!(pane_from_tmux_field("a.b:@1.%2").as_deref(), Some("%2"));
        assert_eq!(pane_from_tmux_field("xepelin:@448"), None);
        assert_eq!(pane_from_tmux_field("xepelin:@448.%"), None);
    }

    #[test]
    fn project_activity_reads_the_directory_from_transcripts() {
        let dir = scratch("claude-projects");
        let profile = profile("personal", &dir);
        let cwd = Path::new("/Users/r/Personal Projects/food");
        let older = UNIX_EPOCH + Duration::from_secs(1_790_000_000);
        let newer = older + Duration::from_secs(60);
        transcript(
            &profile,
            cwd,
            "a",
            &[
                r#"{"type":"user","cwd":"/Users/r/Personal Projects/food","message":{"content":"hi"}}"#,
            ],
            older,
        );
        transcript(
            &profile,
            cwd,
            "b",
            &[r#"{"type":"permission-mode"}"#],
            newer,
        );
        fs::create_dir_all(profile.projects_dir().join("-empty")).unwrap();

        let activity = project_activity(&[profile]);

        assert_eq!(
            activity,
            vec![ProjectActivity {
                profile: "personal".into(),
                cwd: cwd.to_path_buf(),
                last_active_at: newer,
                transcript: dir.join("projects/-Users-r-Personal-Projects-food/b.jsonl"),
            }]
        );
        fs::remove_dir_all(dir).unwrap();
    }

    fn title_of(lines: &[&str]) -> Option<String> {
        let dir = scratch("claude-title");
        let path = dir.join("t.jsonl");
        fs::write(&path, lines.join("\n")).unwrap();
        let title = session_title(&path);
        fs::remove_dir_all(dir).unwrap();
        title
    }

    #[test]
    fn title_prefers_the_newest_custom_then_ai_title_then_first_prompt() {
        let meta = r#"{"type":"user","isMeta":true,"message":{"content":"caveat"}}"#;
        let command =
            r#"{"type":"user","message":{"content":"<command-name>/clear</command-name>"}}"#;
        let tool = r#"{"type":"user","toolUseResult":{},"message":{"content":[{"type":"tool_result","content":"x"}]}}"#;
        let prompt = r#"{"type":"user","message":{"content":[{"type":"text","text":"fix the picker"},{"type":"image"},{"type":"text","text":"here"}]}}"#;
        let later = r#"{"type":"user","message":{"content":"second prompt"}}"#;
        let ai = |title: &str| format!(r#"{{"type":"ai-title","aiTitle":"{title}"}}"#);
        let custom = |title: &str| format!(r#"{{"type":"custom-title","customTitle":"{title}"}}"#);

        assert_eq!(
            title_of(&[meta, command, tool, prompt, later]).as_deref(),
            Some("fix the picker\nhere")
        );
        assert_eq!(
            title_of(&[prompt, &ai("Picker fix"), &ai("Picker rework")]).as_deref(),
            Some("Picker rework")
        );
        assert_eq!(
            title_of(&[
                prompt,
                &custom("picker"),
                &ai("Picker rework"),
                &custom("  ")
            ])
            .as_deref(),
            Some("picker")
        );
        let long = format!(
            r#"{{"type":"user","message":{{"content":"{}"}}}}"#,
            "x".repeat(300)
        );
        assert_eq!(title_of(&[&long]).map(|t| t.len()), Some(200));
        assert_eq!(title_of(&[command, "not json"]), None);
    }
}
