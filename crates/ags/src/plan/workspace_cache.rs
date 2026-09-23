use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use serde::Serialize;
use sha2::{Digest, Sha256};

use super::types::PlanError;

pub(super) const STORE_CONTAINER: &str = "/var/cache/ags/pnpm/store";
pub(super) const CACHE_CONTAINER: &str = "/var/cache/ags/pnpm/cache";

pub(super) struct WorkspacePnpmCache {
    pub store: PathBuf,
    pub cache: PathBuf,
}

#[derive(Serialize)]
struct Identity {
    worktree: String,
    anchor_device: u64,
    anchor_inode: u64,
}

/// Scope writable package-manager state to the canonical Git worktree. The
/// metadata inode distinguishes a checkout recreated at the same path.
pub(super) fn prepare(cache_root: &Path, workdir: &Path) -> Result<WorkspacePnpmCache, PlanError> {
    let worktree = crate::git::repo_root(workdir).unwrap_or_else(|| workdir.to_owned());
    let worktree = fs::canonicalize(&worktree).map_err(|source| PlanError::DirCreate {
        path: worktree.clone(),
        source,
    })?;
    let git_anchor = worktree.join(".git");
    let anchor = if git_anchor.exists() {
        &git_anchor
    } else {
        &worktree
    };
    let metadata = fs::symlink_metadata(anchor).map_err(|source| PlanError::DirCreate {
        path: anchor.to_owned(),
        source,
    })?;
    let mut hasher = Sha256::new();
    hasher.update(worktree.as_os_str().as_bytes());
    hasher.update([0]);
    hasher.update(metadata.dev().to_le_bytes());
    hasher.update(metadata.ino().to_le_bytes());
    let id = format!("{:x}", hasher.finalize());
    let root = cache_root.join("workspace-caches").join(id);
    let store = root.join("pnpm-store");
    let cache = root.join("pnpm-cache");
    for path in [&store, &cache] {
        fs::create_dir_all(path).map_err(|source| PlanError::DirCreate {
            path: path.clone(),
            source,
        })?;
    }
    let identity = Identity {
        worktree: worktree.to_string_lossy().into_owned(),
        anchor_device: metadata.dev(),
        anchor_inode: metadata.ino(),
    };
    let bytes = serde_json::to_vec_pretty(&identity).expect("workspace identity is serializable");
    fs::write(root.join("identity.json"), bytes).map_err(|source| PlanError::DirCreate {
        path: root.join("identity.json"),
        source,
    })?;
    Ok(WorkspacePnpmCache { store, cache })
}

#[cfg(test)]
#[path = "workspace_cache_tests.rs"]
mod tests;
