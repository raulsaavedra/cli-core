//! Prints the top worktrees with their agent state.
//!
//! cargo run --example activity -- [ROOT | --recent DAYS] [LIMIT]

use cli_core::activity::{self, Live, Scope};
use std::path::PathBuf;
use std::time::{Duration, SystemTime};

fn main() -> std::io::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (scope, rest) = match args.first().map(String::as_str) {
        Some("--recent") => {
            let days: u64 = args.get(1).and_then(|d| d.parse().ok()).unwrap_or(7);
            (
                Scope::RecentAgents(Duration::from_secs(days * 86_400)),
                &args[2.min(args.len())..],
            )
        }
        Some(root) => (Scope::Root(PathBuf::from(root)), &args[1..]),
        None => (
            Scope::Root(dirs::home_dir().unwrap_or_default().join("src")),
            &args[..],
        ),
    };
    let limit: usize = rest.first().and_then(|n| n.parse().ok()).unwrap_or(15);

    let started = std::time::Instant::now();
    let entries = activity::discover(&scope)?;
    println!(
        "{scope:?}: {} worktrees in {:?}",
        entries.len(),
        started.elapsed()
    );
    for entry in entries.iter().take(limit) {
        let worktree = &entry.worktree;
        let agent = &entry.agent;
        println!(
            "{:<8} {:<9} {:>5} {:>5} {:<16} {:<28} {}{}  {}",
            match agent.live {
                Live::Working => "working",
                Live::Idle => "idle",
                Live::None => "-",
            },
            agent.profile.as_deref().unwrap_or("-"),
            ago(agent.last_active_at),
            ago(worktree.last_commit_at),
            worktree.repo,
            worktree.branch.as_deref().unwrap_or("(detached)"),
            if worktree.dirty { "*" } else { "" },
            worktree.path.display(),
            agent
                .title
                .as_deref()
                .unwrap_or("")
                .lines()
                .next()
                .unwrap_or(""),
        );
    }
    Ok(())
}

fn ago(at: Option<SystemTime>) -> String {
    let Some(elapsed) = at.and_then(|at| at.elapsed().ok()) else {
        return "-".into();
    };
    match elapsed.as_secs() {
        secs if secs < 3_600 => format!("{}m", secs / 60),
        secs if secs < 86_400 => format!("{}h", secs / 3_600),
        secs => format!("{}d", secs / 86_400),
    }
}
