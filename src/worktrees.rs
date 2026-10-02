//! Git worktrees on disk.
//!
//! A repository is identified by its common git directory, so a linked
//! worktree that sits next to its main checkout resolves to the same
//! repository instead of a second one. Bare entries and worktrees whose
//! directory is gone are left out.

use std::collections::{BTreeSet, HashSet};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// One checked-out worktree of a repository.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Worktree {
    /// Directory name of the repository's main worktree.
    pub repo: String,
    pub path: PathBuf,
    /// `None` while HEAD is detached.
    pub branch: Option<String>,
    pub is_main: bool,
    /// Uncommitted changes or untracked files.
    pub dirty: bool,
    /// Committer time of HEAD; `None` on an unborn branch.
    pub last_commit_at: Option<SystemTime>,
}

/// Every worktree of every repository found at `root` or in one of its
/// direct child directories, ordered by repository, main worktree first, then
/// path.
pub fn discover(root: &Path) -> io::Result<Vec<Worktree>> {
    let root = fs::canonicalize(root)?;
    let mut candidates = vec![root.clone()];
    for entry in fs::read_dir(&root)?.flatten() {
        let path = entry.path();
        if entry.file_type().is_ok_and(|kind| kind.is_dir()) && path.join(".git").exists() {
            candidates.push(path);
        }
    }
    let locations = try_par_map(&candidates, |path| locate(path))?;
    let repos: BTreeSet<PathBuf> = locations
        .into_iter()
        .flatten()
        .map(|l| l.common_dir)
        .collect();
    inspect_all(&repos, |_| true)
}

/// The worktrees whose checkouts contain any of `paths`, in the same order as
/// [`discover`]. Paths outside a git worktree are ignored.
pub fn containing(paths: &[PathBuf]) -> io::Result<Vec<Worktree>> {
    let locations: Vec<Location> = try_par_map(paths, |path| locate(path))?
        .into_iter()
        .flatten()
        .collect();
    let toplevels: HashSet<&Path> = locations.iter().map(|l| l.toplevel.as_path()).collect();
    let repos: BTreeSet<PathBuf> = locations.iter().map(|l| l.common_dir.clone()).collect();
    inspect_all(&repos, |path| toplevels.contains(path))
}

struct Location {
    common_dir: PathBuf,
    toplevel: PathBuf,
}

/// A worktree as `git worktree list` names it, before inspection.
#[derive(Debug, PartialEq, Eq)]
struct Listed {
    path: PathBuf,
    branch: Option<String>,
    is_main: bool,
}

fn locate(path: &Path) -> io::Result<Option<Location>> {
    let Some(output) = git(
        path,
        &[
            "rev-parse",
            "--path-format=absolute",
            "--git-common-dir",
            "--show-toplevel",
        ],
    )?
    else {
        return Ok(None);
    };
    let mut lines = output.lines();
    let (Some(common_dir), Some(toplevel)) = (lines.next(), lines.next()) else {
        return Ok(None);
    };
    Ok(Some(Location {
        common_dir: canonical(Path::new(common_dir)),
        toplevel: canonical(Path::new(toplevel)),
    }))
}

fn inspect_all(
    repos: &BTreeSet<PathBuf>,
    keep: impl Fn(&Path) -> bool,
) -> io::Result<Vec<Worktree>> {
    let repos: Vec<&PathBuf> = repos.iter().collect();
    let listed: Vec<(String, Listed)> = try_par_map(&repos, |common_dir| list(common_dir))?
        .into_iter()
        .flatten()
        .filter(|(_, listed)| keep(&listed.path))
        .collect();
    let mut worktrees = try_par_map(&listed, |(repo, listed)| inspect(repo, listed))?;
    worktrees.sort_by(|a, b| {
        a.repo
            .cmp(&b.repo)
            .then_with(|| b.is_main.cmp(&a.is_main))
            .then_with(|| a.path.cmp(&b.path))
    });
    Ok(worktrees)
}

fn list(common_dir: &Path) -> io::Result<Vec<(String, Listed)>> {
    let Some(output) = git(common_dir, &["worktree", "list", "--porcelain"])? else {
        return Ok(Vec::new());
    };
    let (repo, listed) = parse_worktree_list(&output);
    Ok(listed
        .into_iter()
        .filter_map(|listed| {
            let path = fs::canonicalize(&listed.path).ok()?;
            Some((repo.clone(), Listed { path, ..listed }))
        })
        .collect())
}

/// Repository name and the checked-out worktrees in `git worktree list
/// --porcelain` output. The first entry is always the main worktree.
fn parse_worktree_list(output: &str) -> (String, Vec<Listed>) {
    let mut repo = String::new();
    let mut listed = Vec::new();
    for (index, block) in output
        .split("\n\n")
        .filter(|b| !b.trim().is_empty())
        .enumerate()
    {
        let mut path = None;
        let mut branch = None;
        let mut checked_out = true;
        for line in block.lines() {
            if let Some(value) = line.strip_prefix("worktree ") {
                path = Some(PathBuf::from(value));
            } else if let Some(value) = line.strip_prefix("branch ") {
                branch = Some(
                    value
                        .strip_prefix("refs/heads/")
                        .unwrap_or(value)
                        .to_string(),
                );
            } else if line == "bare" || line == "prunable" || line.starts_with("prunable ") {
                checked_out = false;
            }
        }
        let Some(path) = path else { continue };
        if index == 0 {
            repo = repo_name(&path);
        }
        if checked_out {
            listed.push(Listed {
                path,
                branch,
                is_main: index == 0,
            });
        }
    }
    (repo, listed)
}

fn repo_name(main_path: &Path) -> String {
    let name = main_path
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| main_path.display().to_string());
    name.strip_suffix(".git")
        .map(str::to_string)
        .unwrap_or(name)
}

fn inspect(repo: &str, listed: &Listed) -> io::Result<Worktree> {
    let status = git(
        &listed.path,
        &["status", "--porcelain", "--untracked-files=normal"],
    )?;
    let committed = git(&listed.path, &["log", "-1", "--format=%ct"])?;
    Ok(Worktree {
        repo: repo.to_string(),
        path: listed.path.clone(),
        branch: listed.branch.clone(),
        is_main: listed.is_main,
        dirty: status.is_some_and(|out| !out.trim().is_empty()),
        last_commit_at: committed
            .and_then(|out| out.trim().parse::<u64>().ok())
            .map(|secs| UNIX_EPOCH + Duration::from_secs(secs)),
    })
}

/// Stdout of a git command run in `dir`, or `None` when git exits non-zero.
fn git(dir: &Path, args: &[&str]) -> io::Result<Option<String>> {
    let output = Command::new("git").arg("-C").arg(dir).args(args).output()?;
    Ok(output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).into_owned()))
}

fn canonical(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// `f` over `items` on a few threads, keeping order. Git spawns dominate the
/// cost of discovery, and they run independently.
fn try_par_map<T: Sync, R: Send>(
    items: &[T],
    f: impl Fn(&T) -> io::Result<R> + Sync,
) -> io::Result<Vec<R>> {
    if items.is_empty() {
        return Ok(Vec::new());
    }
    let workers = std::thread::available_parallelism().map_or(4, |n| n.get());
    let chunk = items.len().div_ceil(workers);
    std::thread::scope(|scope| {
        let handles: Vec<_> = items
            .chunks(chunk)
            .map(|part| scope.spawn(|| part.iter().map(&f).collect::<io::Result<Vec<R>>>()))
            .collect();
        let mut results = Vec::with_capacity(items.len());
        for handle in handles {
            results.extend(handle.join().expect("discovery worker panicked")?);
        }
        Ok(results)
    })
}

#[cfg(test)]
pub(crate) mod fixture {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::process::Command;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A fresh, canonical scratch directory for one test.
    pub fn scratch(label: &str) -> PathBuf {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "cli-core-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        fs::canonicalize(dir).unwrap()
    }

    /// Runs git in `dir` without the user's global or system config.
    pub fn git(dir: &Path, args: &[&str]) {
        let status = Command::new("git")
            .arg("-C")
            .arg(dir)
            .args([
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "-c",
                "init.defaultBranch=main",
            ])
            .args(args)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_COMMITTER_DATE", "@1790000000 +0000")
            .env("GIT_AUTHOR_DATE", "@1790000000 +0000")
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?} in {}", dir.display());
    }

    /// A repository at `path` with one commit on `main`.
    pub fn repo(path: &Path) {
        fs::create_dir_all(path).unwrap();
        git(path, &["init", "-q"]);
        fs::write(path.join("README"), "fixture\n").unwrap();
        git(path, &["add", "README"]);
        git(path, &["commit", "-q", "-m", "init"]);
    }
}

#[cfg(test)]
mod tests {
    use super::fixture::{git, repo, scratch};
    use super::*;

    fn summary(
        worktrees: &[Worktree],
        root: &Path,
    ) -> Vec<(String, String, Option<String>, bool, bool)> {
        worktrees
            .iter()
            .map(|w| {
                (
                    w.repo.clone(),
                    w.path.strip_prefix(root).unwrap().display().to_string(),
                    w.branch.clone(),
                    w.is_main,
                    w.dirty,
                )
            })
            .collect()
    }

    #[test]
    fn discovers_repos_at_the_root_and_its_children_once_each() {
        let root = scratch("worktrees");
        repo(&root.join("beta"));
        repo(&root.join("alpha"));
        git(
            &root.join("alpha"),
            &["worktree", "add", "-q", "-b", "feature", "../alpha-feature"],
        );
        git(
            &root.join("alpha"),
            &["worktree", "add", "-q", "--detach", ".worktrees/probe"],
        );
        git(
            &root.join("alpha"),
            &["worktree", "add", "-q", "-b", "gone", "../alpha-gone"],
        );
        fs::remove_dir_all(root.join("alpha-gone")).unwrap();
        fs::write(root.join("alpha-feature/notes"), "wip\n").unwrap();
        fs::create_dir_all(root.join("plain/nested")).unwrap();
        repo(&root.join("plain/nested/deep"));

        let worktrees = discover(&root).unwrap();

        assert_eq!(
            summary(&worktrees, &root),
            vec![
                (
                    "alpha".into(),
                    "alpha".into(),
                    Some("main".into()),
                    true,
                    false
                ),
                (
                    "alpha".into(),
                    "alpha/.worktrees/probe".into(),
                    None,
                    false,
                    false
                ),
                (
                    "alpha".into(),
                    "alpha-feature".into(),
                    Some("feature".into()),
                    false,
                    true
                ),
                (
                    "beta".into(),
                    "beta".into(),
                    Some("main".into()),
                    true,
                    false
                ),
            ]
        );
        assert!(worktrees
            .iter()
            .all(|w| w.last_commit_at == Some(UNIX_EPOCH + Duration::from_secs(1_790_000_000))));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn root_inside_a_repo_resolves_to_that_repo() {
        let root = scratch("worktrees-inside");
        repo(&root.join("gamma"));
        fs::create_dir_all(root.join("gamma/src")).unwrap();

        let worktrees = discover(&root.join("gamma/src")).unwrap();

        assert_eq!(
            summary(&worktrees, &root),
            vec![(
                "gamma".into(),
                "gamma".into(),
                Some("main".into()),
                true,
                false
            )]
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn containing_keeps_only_worktrees_holding_the_paths() {
        let root = scratch("worktrees-containing");
        repo(&root.join("alpha"));
        git(
            &root.join("alpha"),
            &["worktree", "add", "-q", "-b", "feature", "../alpha-feature"],
        );
        git(
            &root.join("alpha"),
            &["worktree", "add", "-q", "-b", "idle", "../alpha-idle"],
        );
        fs::create_dir_all(root.join("alpha-feature/src")).unwrap();

        let worktrees = containing(&[
            root.join("alpha-feature/src"),
            root.join("alpha-feature"),
            root.join("missing"),
            std::env::temp_dir(),
        ])
        .unwrap();

        assert_eq!(
            summary(&worktrees, &root),
            vec![(
                "alpha".into(),
                "alpha-feature".into(),
                Some("feature".into()),
                false,
                false
            )]
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn porcelain_skips_bare_and_prunable_entries() {
        let output = "worktree /srv/tools.git\nbare\n\n\
                      worktree /srv/tools-a\nHEAD abc\nbranch refs/heads/a\n\n\
                      worktree /srv/tools-b\nHEAD def\nbranch refs/heads/b\nprunable gitdir file points to non-existent location\n";

        let (repo, listed) = parse_worktree_list(output);

        assert_eq!(repo, "tools");
        assert_eq!(
            listed,
            vec![Listed {
                path: PathBuf::from("/srv/tools-a"),
                branch: Some("a".into()),
                is_main: false,
            }]
        );
    }
}
