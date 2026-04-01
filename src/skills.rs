use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Options for installing a skill directory.
pub struct InstallOptions {
    /// Source directory to install from.
    pub src_dir: String,
    /// Destination root directory (the skill will be placed as a subdirectory).
    pub dest_dir: String,
    /// Name override; defaults to the basename of `src_dir`.
    pub name: Option<String>,
    /// Whether to overwrite an existing destination.
    pub overwrite: bool,
    /// If true, create a symlink instead of copying.
    pub link: bool,
}

/// Resolve the skills destination directory.
/// Returns the absolute path of `dest`, or `~/.agents/skills` when `None`.
pub fn resolve_skills_dir(dest: Option<&str>) -> io::Result<PathBuf> {
    if let Some(d) = dest {
        return Ok(fs::canonicalize(d).unwrap_or_else(|_| PathBuf::from(d)));
    }
    Ok(resolve_default_skills_dirs()?
        .into_iter()
        .next()
        .expect("default skills dirs should not be empty"))
}

/// Resolve the default skills destination directories.
/// Returns `~/.agents/skills`.
pub fn resolve_default_skills_dirs() -> io::Result<Vec<PathBuf>> {
    let home = dirs::home_dir()
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "home directory not found"))?;
    Ok(vec![home.join(".agents").join("skills")])
}

/// Install a skill directory to the destination.
/// Returns the absolute path of the installed skill.
pub fn install(opts: &InstallOptions) -> io::Result<PathBuf> {
    if opts.src_dir.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "source directory is required",
        ));
    }
    if opts.dest_dir.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "destination directory is required",
        ));
    }

    let name = opts.name.as_deref().unwrap_or_else(|| {
        Path::new(&opts.src_dir)
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("skill")
    });

    let src = fs::canonicalize(&opts.src_dir)?;
    let dest_root = PathBuf::from(&opts.dest_dir);
    let dest = dest_root.join(name);

    fs::create_dir_all(&dest_root)?;

    if dest.exists() {
        if !opts.overwrite {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                format!("destination exists: {}", dest.display()),
            ));
        }
        if dest.is_dir() {
            fs::remove_dir_all(&dest)?;
        } else {
            fs::remove_file(&dest)?;
        }
    }

    if opts.link {
        #[cfg(unix)]
        std::os::unix::fs::symlink(&src, &dest)?;
        #[cfg(windows)]
        std::os::windows::fs::symlink_dir(&src, &dest)?;
        return Ok(dest);
    }

    copy_dir(&src, &dest)?;
    Ok(dest)
}

/// Recursively copy a directory, preserving file permissions.
fn copy_dir(src: &Path, dest: &Path) -> io::Result<()> {
    fs::create_dir_all(dest)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dest_path = dest.join(entry.file_name());

        if src_path.is_dir() {
            copy_dir(&src_path, &dest_path)?;
        } else {
            copy_file_preserve_mode(&src_path, &dest_path)?;
        }
    }
    Ok(())
}

/// Copy a single file, preserving its permissions.
fn copy_file_preserve_mode(src: &Path, dest: &Path) -> io::Result<()> {
    let metadata = fs::metadata(src)?;
    fs::copy(src, dest)?;
    fs::set_permissions(dest, metadata.permissions())?;
    Ok(())
}
