use std::collections::BTreeSet;
use std::fs;
use std::io::{self, BufRead, IsTerminal, Write};
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub enum TrustError {
    Io { path: PathBuf, source: io::Error },
}

impl std::fmt::Display for TrustError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(f, "trust store error for {}: {source}", path.display())
            }
        }
    }
}

impl std::error::Error for TrustError {}

pub trait RepoConfigPrompter {
    fn confirm_repo_local_config(&self, repo_root: &Path, overlay_path: &Path) -> bool;
}

pub struct StdioRepoConfigPrompter;

impl RepoConfigPrompter for StdioRepoConfigPrompter {
    fn confirm_repo_local_config(&self, repo_root: &Path, overlay_path: &Path) -> bool {
        if !io::stdin().is_terminal() || !io::stderr().is_terminal() {
            eprintln!(
                "warning: repo-local AGS config present at {}, but no interactive terminal is available to confirm trust; skipping it",
                overlay_path.display()
            );
            return false;
        }

        eprintln!(
            "Found repo-local AGS config: {}\nThis file comes from the current repository and can add mounts, tools, and secrets.\nTrust this repo-local AGS config for {}? [y/N] ",
            overlay_path.display(),
            repo_root.display()
        );

        let stdin = io::stdin();
        let mut reader = stdin.lock();
        loop {
            eprint!("> ");
            let _ = io::stderr().flush();

            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(0) => return false,
                Ok(_) => match line.trim().to_ascii_lowercase().as_str() {
                    "y" | "yes" => return true,
                    "" | "n" | "no" => return false,
                    _ => eprintln!("Please answer y or n."),
                },
                Err(err) => {
                    eprintln!("warning: failed to read trust prompt response: {err}");
                    return false;
                }
            }
        }
    }
}

pub fn default_trust_store_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("~/.config"))
        .join("ags/trusted-repo-overlays.txt")
}

pub fn resolve_repo_local_overlay(
    cwd: &Path,
    global_config_path: &Path,
    trust_store_path: &Path,
    prompter: &dyn RepoConfigPrompter,
) -> Result<Option<PathBuf>, TrustError> {
    let Some(repo_root) = crate::git::repo_root(cwd) else {
        return Ok(None);
    };

    let overlay_path = repo_root.join(".ags/config.toml");
    if !overlay_path.exists() || same_existing_path(&overlay_path, global_config_path) {
        return Ok(None);
    }

    let repo_root = canonical_or_original(&repo_root);
    if is_repo_trusted(trust_store_path, &repo_root)? {
        return Ok(Some(overlay_path));
    }

    if !prompter.confirm_repo_local_config(&repo_root, &overlay_path) {
        eprintln!(
            "warning: skipping untrusted repo-local AGS config: {}",
            overlay_path.display()
        );
        return Ok(None);
    }

    trust_repo(trust_store_path, &repo_root)?;
    eprintln!(
        "Trusted repo-local AGS config for {} (saved in {}).",
        repo_root.display(),
        trust_store_path.display()
    );
    Ok(Some(overlay_path))
}

fn is_repo_trusted(trust_store_path: &Path, repo_root: &Path) -> Result<bool, TrustError> {
    Ok(load_trusted_repo_roots(trust_store_path)?.contains(repo_root))
}

fn trust_repo(trust_store_path: &Path, repo_root: &Path) -> Result<(), TrustError> {
    let mut trusted = load_trusted_repo_roots(trust_store_path)?;
    trusted.insert(repo_root.to_path_buf());
    write_trusted_repo_roots(trust_store_path, &trusted)
}

fn load_trusted_repo_roots(trust_store_path: &Path) -> Result<BTreeSet<PathBuf>, TrustError> {
    let content = match fs::read_to_string(trust_store_path) {
        Ok(content) => content,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(BTreeSet::new()),
        Err(source) => {
            return Err(TrustError::Io {
                path: trust_store_path.to_path_buf(),
                source,
            });
        }
    };

    Ok(content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(PathBuf::from)
        .collect())
}

fn write_trusted_repo_roots(
    trust_store_path: &Path,
    trusted_roots: &BTreeSet<PathBuf>,
) -> Result<(), TrustError> {
    if let Some(parent) = trust_store_path.parent() {
        fs::create_dir_all(parent).map_err(|source| TrustError::Io {
            path: parent.to_path_buf(),
            source,
        })?;
    }

    let mut content = trusted_roots
        .iter()
        .map(|path| path.to_string_lossy())
        .collect::<Vec<_>>()
        .join("\n");
    if !content.is_empty() {
        content.push('\n');
    }

    fs::write(trust_store_path, content).map_err(|source| TrustError::Io {
        path: trust_store_path.to_path_buf(),
        source,
    })
}

fn canonical_or_original(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn same_existing_path(a: &Path, b: &Path) -> bool {
    let Ok(a) = a.canonicalize() else {
        return false;
    };
    let Ok(b) = b.canonicalize() else {
        return false;
    };
    a == b
}
