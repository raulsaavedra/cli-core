//! Worktrees joined with the Claude Code agents working in them.
//!
//! A session or transcript directory belongs to the deepest worktree that
//! contains its working directory, so a session in a linked worktree nested
//! inside the main checkout counts for the linked one.

use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use crate::claude::{self, LiveSession, ProjectActivity, SessionStatus};
use crate::worktrees::{self, Worktree};

/// Which worktrees to report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    /// Every worktree of every repository at this directory or one of its
    /// direct children.
    Root(PathBuf),
    /// Every worktree, anywhere, with a live agent session or agent activity
    /// within this long, in any profile.
    RecentAgents(Duration),
}

/// Whether an agent session is running in the worktree right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Live {
    /// A session is running a turn.
    Working,
    /// A session is open and waiting.
    Idle,
    None,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Agent {
    pub live: Live,
    /// Profile of the live session, else of the most recent activity.
    pub profile: Option<String>,
    /// Title of the live session, else of the most recent transcript.
    pub title: Option<String>,
    /// Newest transcript write in the worktree, across profiles.
    pub last_active_at: Option<SystemTime>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeEntry {
    pub worktree: Worktree,
    pub agent: Agent,
}

/// Worktrees in `scope` with their agent state: live sessions first, then
/// the most recently touched by an agent or a commit, then by repository and
/// path.
pub fn discover(scope: &Scope) -> io::Result<Vec<WorktreeEntry>> {
    let profiles = claude::profiles();
    let live = claude::live_sessions(&profiles)?;
    let projects = claude::project_activity(&profiles);
    let worktrees = match scope {
        Scope::Root(root) => worktrees::discover(root)?,
        Scope::RecentAgents(window) => {
            let since = SystemTime::now()
                .checked_sub(*window)
                .unwrap_or(SystemTime::UNIX_EPOCH);
            worktrees::containing(&active_dirs(&live, &projects, since))?
        }
    };
    Ok(join(worktrees, &live, &projects))
}

fn active_dirs(
    live: &[LiveSession],
    projects: &[ProjectActivity],
    since: SystemTime,
) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = live
        .iter()
        .map(|session| session.cwd.clone())
        .chain(
            projects
                .iter()
                .filter(|project| project.last_active_at >= since)
                .map(|project| project.cwd.clone()),
        )
        .filter(|dir| dir.is_dir())
        .collect();
    dirs.sort();
    dirs.dedup();
    dirs
}

fn join(
    worktrees: Vec<Worktree>,
    live: &[LiveSession],
    projects: &[ProjectActivity],
) -> Vec<WorktreeEntry> {
    let mut sessions_by_worktree: Vec<Vec<&LiveSession>> = vec![Vec::new(); worktrees.len()];
    for session in live {
        if let Some(index) = owner(&worktrees, &session.cwd) {
            sessions_by_worktree[index].push(session);
        }
    }
    let mut projects_by_worktree: Vec<Vec<&ProjectActivity>> = vec![Vec::new(); worktrees.len()];
    for project in projects {
        if let Some(index) = owner(&worktrees, &project.cwd) {
            projects_by_worktree[index].push(project);
        }
    }
    let mut entries: Vec<WorktreeEntry> = worktrees
        .into_iter()
        .zip(sessions_by_worktree.iter().zip(&projects_by_worktree))
        .map(|(worktree, (sessions, projects))| WorktreeEntry {
            worktree,
            agent: agent(sessions, projects),
        })
        .collect();
    entries.sort_by(|a, b| {
        let live = |entry: &WorktreeEntry| entry.agent.live != Live::None;
        live(b)
            .cmp(&live(a))
            .then_with(|| recency(b).cmp(&recency(a)))
            .then_with(|| a.worktree.repo.cmp(&b.worktree.repo))
            .then_with(|| a.worktree.path.cmp(&b.worktree.path))
    });
    entries
}

/// Index of the deepest worktree containing `dir`.
fn owner(worktrees: &[Worktree], dir: &Path) -> Option<usize> {
    worktrees
        .iter()
        .enumerate()
        .filter(|(_, worktree)| dir.starts_with(&worktree.path))
        .max_by_key(|(_, worktree)| worktree.path.components().count())
        .map(|(index, _)| index)
}

fn agent(sessions: &[&LiveSession], projects: &[&ProjectActivity]) -> Agent {
    let session = sessions.iter().max_by_key(|session| {
        (
            session.status == SessionStatus::Busy,
            session.status_changed_at,
        )
    });
    let latest = projects.iter().max_by_key(|project| project.last_active_at);
    match session {
        Some(session) => Agent {
            live: match session.status {
                SessionStatus::Busy => Live::Working,
                SessionStatus::Idle => Live::Idle,
            },
            profile: Some(session.profile.clone()),
            title: claude::session_title(&session.transcript),
            last_active_at: latest.map(|project| project.last_active_at),
        },
        None => Agent {
            live: Live::None,
            profile: latest.map(|project| project.profile.clone()),
            title: latest.and_then(|project| claude::session_title(&project.transcript)),
            last_active_at: latest.map(|project| project.last_active_at),
        },
    }
}

fn recency(entry: &WorktreeEntry) -> Option<SystemTime> {
    entry
        .agent
        .last_active_at
        .max(entry.worktree.last_commit_at)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::claude::fixture::{own_proc_start, profile, session_record, transcript};
    use crate::worktrees::fixture::{git, repo, scratch};
    use std::fs;
    use std::time::UNIX_EPOCH;

    fn at(secs: u64) -> SystemTime {
        UNIX_EPOCH + Duration::from_secs(secs)
    }

    fn worktree(repo: &str, path: &str, committed: u64) -> Worktree {
        Worktree {
            repo: repo.into(),
            path: PathBuf::from(path),
            branch: Some("main".into()),
            is_main: !path.contains("/.worktrees/"),
            dirty: false,
            last_commit_at: Some(at(committed)),
        }
    }

    fn session(cwd: &str, status: SessionStatus, profile: &str) -> LiveSession {
        LiveSession {
            pid: 1,
            session_id: "s".into(),
            profile: profile.into(),
            cwd: PathBuf::from(cwd),
            status,
            status_changed_at: None,
            tmux_pane: None,
            transcript: PathBuf::from("/missing.jsonl"),
        }
    }

    fn project(cwd: &str, profile: &str, active: u64) -> ProjectActivity {
        ProjectActivity {
            profile: profile.into(),
            cwd: PathBuf::from(cwd),
            last_active_at: at(active),
            transcript: PathBuf::from("/missing.jsonl"),
        }
    }

    fn rows(entries: &[WorktreeEntry]) -> Vec<(String, Live, Option<String>, Option<SystemTime>)> {
        entries
            .iter()
            .map(|e| {
                (
                    e.worktree.path.display().to_string(),
                    e.agent.live,
                    e.agent.profile.clone(),
                    e.agent.last_active_at,
                )
            })
            .collect()
    }

    #[test]
    fn live_first_then_latest_of_agent_and_commit_then_alphabetical() {
        let worktrees = vec![
            worktree("app", "/src/app", 100),
            worktree("app", "/src/app/.worktrees/fix", 100),
            worktree("lib", "/src/lib", 300),
            worktree("old", "/src/old", 50),
            worktree("cli", "/src/cli", 50),
            worktree("zed", "/src/zed", 10),
        ];
        let live = [
            session("/src/zed/src", SessionStatus::Idle, "work"),
            session("/src/app/.worktrees/fix", SessionStatus::Idle, "personal"),
            session("/src/app/.worktrees/fix", SessionStatus::Busy, "work"),
            session("/elsewhere", SessionStatus::Busy, "work"),
        ];
        let projects = [
            project("/src/app", "personal", 400),
            project("/src/app", "work", 200),
            project("/src/app/.worktrees/fix/src", "personal", 20),
        ];

        let entries = join(worktrees, &live, &projects);

        assert_eq!(
            rows(&entries),
            vec![
                (
                    "/src/app/.worktrees/fix".into(),
                    Live::Working,
                    Some("work".into()),
                    Some(at(20))
                ),
                ("/src/zed".into(), Live::Idle, Some("work".into()), None),
                (
                    "/src/app".into(),
                    Live::None,
                    Some("personal".into()),
                    Some(at(400))
                ),
                ("/src/lib".into(), Live::None, None, None),
                ("/src/cli".into(), Live::None, None, None),
                ("/src/old".into(), Live::None, None, None),
            ]
        );
    }

    #[test]
    fn joins_real_repositories_with_profile_state() {
        let root = scratch("activity");
        repo(&root.join("app"));
        git(
            &root.join("app"),
            &["worktree", "add", "-q", "-b", "fix", ".worktrees/fix"],
        );
        repo(&root.join("lib"));
        let work = profile("work", &root.join("profiles/work"));
        let personal = profile("personal", &root.join("profiles/personal"));
        let fix = root.join("app/.worktrees/fix");
        let pid = std::process::id();
        session_record(&work, pid, &own_proc_start(), &fix, "busy");
        transcript(
            &work,
            &fix,
            &format!("s-{pid}"),
            &[
                &format!(
                    r#"{{"type":"user","cwd":"{}","message":{{"content":"fix the picker"}}}}"#,
                    fix.display()
                ),
                r#"{"type":"ai-title","aiTitle":"Picker fix"}"#,
            ],
            at(1_790_000_100),
        );
        transcript(
            &personal,
            &root.join("lib"),
            "old",
            &[&format!(
                r#"{{"type":"user","cwd":"{}","message":{{"content":"tidy lib"}}}}"#,
                root.join("lib").display()
            )],
            at(1_790_000_500),
        );

        let profiles = [work, personal];
        let live = claude::live_sessions(&profiles).unwrap();
        let projects = claude::project_activity(&profiles);
        let entries = join(worktrees::discover(&root).unwrap(), &live, &projects);

        let summary: Vec<_> = entries
            .iter()
            .map(|e| {
                (
                    e.worktree
                        .path
                        .strip_prefix(&root)
                        .unwrap()
                        .display()
                        .to_string(),
                    e.worktree.branch.clone(),
                    e.agent.clone(),
                )
            })
            .collect();
        let agent = |live, profile: Option<&str>, title: Option<&str>, active| Agent {
            live,
            profile: profile.map(str::to_string),
            title: title.map(str::to_string),
            last_active_at: active,
        };
        assert_eq!(
            summary,
            vec![
                (
                    "app/.worktrees/fix".into(),
                    Some("fix".into()),
                    agent(
                        Live::Working,
                        Some("work"),
                        Some("Picker fix"),
                        Some(at(1_790_000_100))
                    )
                ),
                (
                    "lib".into(),
                    Some("main".into()),
                    agent(
                        Live::None,
                        Some("personal"),
                        Some("tidy lib"),
                        Some(at(1_790_000_500))
                    )
                ),
                (
                    "app".into(),
                    Some("main".into()),
                    agent(Live::None, None, None, None)
                ),
            ]
        );

        let recent = active_dirs(&live, &projects, at(1_790_000_200));
        assert_eq!(recent, vec![fix.clone(), root.join("lib")]);
        fs::remove_dir_all(root).unwrap();
    }
}
