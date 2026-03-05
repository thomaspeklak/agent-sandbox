use std::collections::BTreeSet;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Error type for git operations.
#[derive(Debug)]
pub enum GitError {
    Io(io::Error),
    GitConfigRead(String),
}

impl fmt::Display for GitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(err) => write!(f, "git I/O error: {err}"),
            Self::GitConfigRead(msg) => write!(f, "git config error: {msg}"),
        }
    }
}

impl From<io::Error> for GitError {
    fn from(err: io::Error) -> Self {
        Self::Io(err)
    }
}

/// Ensure the sandbox gitconfig exists at `gitconfig_path`.
///
/// If the file already exists, this is a no-op.
/// Otherwise, creates it with SSH signing config, pulling user.name and
/// user.email from the host's global git config (with fallback defaults).
pub fn ensure_gitconfig(gitconfig_path: &Path, sign_key_container: &str) -> Result<(), GitError> {
    if gitconfig_path.exists() {
        return Ok(());
    }

    if let Some(parent) = gitconfig_path.parent() {
        fs::create_dir_all(parent)?;
    }

    let name = git_global_config("user.name").unwrap_or_else(|| "Agent Sandbox Agent".to_owned());
    let email =
        git_global_config("user.email").unwrap_or_else(|| "agent-sandbox@example.invalid".to_owned());

    let content = format!(
        "[user]\n\
         \x20   name = {name}\n\
         \x20   email = {email}\n\
         \x20   signingkey = {sign_key_container}\n\
         [commit]\n\
         \x20   gpgsign = true\n\
         [gpg]\n\
         \x20   format = ssh\n"
    );

    fs::write(gitconfig_path, &content)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(gitconfig_path, fs::Permissions::from_mode(0o600))?;
    }

    Ok(())
}

/// Read a single value from the host's global git config.
fn git_global_config(key: &str) -> Option<String> {
    let output = Command::new("git")
        .args(["config", "--global", key])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let value = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if value.is_empty() { None } else { Some(value) }
}

/// Paths outside the workdir that need to be mounted for git operations.
///
/// This handles worktrees and submodules where `.git` metadata lives
/// outside the working directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExternalGitMounts {
    pub paths: Vec<PathBuf>,
}

/// Discover external git metadata paths that need container mounts.
///
/// For worktrees and submodules, git metadata may live outside the workdir.
/// This function detects those paths so they can be bind-mounted into the
/// container.
pub fn discover_external_git_mounts(workdir: &Path) -> ExternalGitMounts {
    let mut unique = BTreeSet::new();

    // Must be inside a git work tree
    if !is_inside_work_tree(workdir) {
        return ExternalGitMounts { paths: Vec::new() };
    }

    if let Some(git_dir) = resolve_absolute_git_dir(workdir) {
        try_add_mount(&mut unique, &git_dir, workdir);
    }

    if let Some(common_dir) = resolve_common_dir(workdir) {
        try_add_mount(&mut unique, &common_dir, workdir);
    }

    ExternalGitMounts {
        paths: unique.into_iter().collect(),
    }
}

/// If `workdir` is a linked git worktree, return the parent repository root.
///
/// For linked worktrees, `git rev-parse --git-common-dir` points to
/// `<main-repo>/.git`. This function returns `<main-repo>`.
///
/// Returns `None` for non-git paths, normal repos, or if resolution fails.
pub fn worktree_parent_repo_dir(workdir: &Path) -> Option<PathBuf> {
    if !is_inside_work_tree(workdir) {
        return None;
    }

    let git_dir = resolve_absolute_git_dir(workdir)?;
    if !has_git_worktrees_segment(&git_dir) {
        return None;
    }

    let common_dir = resolve_common_dir(workdir)?;
    common_dir.parent().map(Path::to_path_buf)
}

/// Parse a `.git` file (as used by worktrees) to extract the gitdir path.
///
/// A `.git` file contains a single line: `gitdir: /path/to/git/dir`
pub fn parse_dot_git_file(content: &str) -> Option<PathBuf> {
    let line = content.lines().next()?.trim();
    let path_str = line.strip_prefix("gitdir:")?;
    let path_str = path_str.trim();
    if path_str.is_empty() {
        return None;
    }
    Some(PathBuf::from(path_str))
}

fn is_inside_work_tree(workdir: &Path) -> bool {
    Command::new("git")
        .args([
            "-C",
            &workdir.to_string_lossy(),
            "rev-parse",
            "--is-inside-work-tree",
        ])
        .output()
        .ok()
        .is_some_and(|o| o.status.success())
}

fn resolve_absolute_git_dir(workdir: &Path) -> Option<PathBuf> {
    let output = Command::new("git")
        .args([
            "-C",
            &workdir.to_string_lossy(),
            "rev-parse",
            "--path-format=absolute",
            "--absolute-git-dir",
        ])
        .output()
        .ok()?;

    if !output.status.success() {
        // Fallback without --path-format (older git)
        let output = Command::new("git")
            .args([
                "-C",
                &workdir.to_string_lossy(),
                "rev-parse",
                "--absolute-git-dir",
            ])
            .output()
            .ok()?;

        if !output.status.success() {
            return None;
        }
        return parse_trimmed_path(&output.stdout);
    }

    parse_trimmed_path(&output.stdout)
}

fn resolve_common_dir(workdir: &Path) -> Option<PathBuf> {
    let output = Command::new("git")
        .args([
            "-C",
            &workdir.to_string_lossy(),
            "rev-parse",
            "--path-format=absolute",
            "--git-common-dir",
        ])
        .output()
        .ok()?;

    if output.status.success()
        && let Some(p) = parse_trimmed_path(&output.stdout)
        && p.is_absolute()
    {
        return Some(p);
    }

    // Fallback without --path-format
    let output = Command::new("git")
        .args([
            "-C",
            &workdir.to_string_lossy(),
            "rev-parse",
            "--git-common-dir",
        ])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let raw = parse_trimmed_path(&output.stdout)?;
    if raw.is_absolute() {
        return Some(raw);
    }

    // Relative common_dir: resolve against workdir
    let resolved = workdir.join(&raw);
    resolved.canonicalize().ok().or(Some(resolved))
}

fn parse_trimmed_path(bytes: &[u8]) -> Option<PathBuf> {
    let s = String::from_utf8_lossy(bytes).trim().to_owned();
    if s.is_empty() {
        None
    } else {
        Some(PathBuf::from(s))
    }
}

fn has_git_worktrees_segment(path: &Path) -> bool {
    let mut prev_dot_git = false;
    for component in path.components() {
        let std::path::Component::Normal(name) = component else {
            prev_dot_git = false;
            continue;
        };

        if prev_dot_git && name == "worktrees" {
            return true;
        }

        prev_dot_git = name == ".git";
    }
    false
}

/// Add a candidate path to the mount set if it's a real directory outside workdir.
fn try_add_mount(set: &mut BTreeSet<PathBuf>, candidate: &Path, workdir: &Path) {
    let candidate = match candidate.canonicalize() {
        Ok(p) => p,
        Err(_) => return,
    };

    if !candidate.is_dir() {
        return;
    }

    let workdir_resolved = workdir
        .canonicalize()
        .unwrap_or_else(|_| workdir.to_path_buf());
    if path_is_within(&candidate, &workdir_resolved) {
        return;
    }

    set.insert(candidate);
}

/// Check if `child` is the same as or a descendant of `parent`.
fn path_is_within(child: &Path, parent: &Path) -> bool {
    child == parent || child.starts_with(parent)
}
