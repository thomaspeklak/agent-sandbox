use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Repository {
    pub id: String,
    pub common: PathBuf,
    pub main: PathBuf,
    pub worktrees: Vec<PathBuf>,
    pub device: u64,
    pub inode: u64,
}

fn git(path: &Path, args: &[&str]) -> io::Result<Vec<u8>> {
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(args)
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "cannot resolve T3 repository: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(output.stdout)
}

impl Repository {
    pub fn resolve(path: &Path) -> io::Result<Self> {
        let root = crate::git::repo_root(path)
            .ok_or_else(|| io::Error::other("T3 requires a Git checkout"))?;
        let common = git(
            &root,
            &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        )?;
        let common = PathBuf::from(String::from_utf8(common).map_err(io::Error::other)?.trim())
            .canonicalize()?;
        let listing = git(&root, &["worktree", "list", "--porcelain", "-z"])?;
        let mut worktrees = Vec::new();
        for record in listing.split(|byte| *byte == 0) {
            if let Some(path) = record.strip_prefix(b"worktree ") {
                let path =
                    PathBuf::from(String::from_utf8(path.to_vec()).map_err(io::Error::other)?);
                // Prunable/missing worktrees cannot become mount sources.
                if path.is_dir() {
                    worktrees.push(path.canonicalize()?);
                }
            }
        }
        let main = worktrees
            .first()
            .cloned()
            .ok_or_else(|| io::Error::other("repository has no accessible main checkout"))?;
        let metadata = common.metadata()?;
        let id = format!(
            "{:x}",
            Sha256::digest(common.as_os_str().as_encoded_bytes())
        );
        Ok(Self {
            id,
            common,
            main,
            worktrees,
            device: metadata.dev(),
            inode: metadata.ino(),
        })
    }

    pub fn validate(&self) -> io::Result<Self> {
        let current = Self::resolve(&self.main)?;
        if self.id != current.id || self.device != current.device || self.inode != current.inode {
            return Err(io::Error::other(
                "registered repository was replaced or moved; register it again with `ags --agent t3`",
            ));
        }
        Ok(current)
    }
}
