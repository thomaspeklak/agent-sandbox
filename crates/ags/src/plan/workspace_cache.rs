use std::fs;
use std::io::{self, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};

use serde::Serialize;
use sha2::{Digest, Sha256};

use super::types::PlanError;

pub(super) const STORE_CONTAINER: &str = "/var/cache/ags/pnpm/store";
pub(super) const CACHE_CONTAINER: &str = "/var/cache/ags/pnpm/cache";
const INCARNATION_FILE: &str = "ags-workspace-id";

pub(super) struct WorkspacePnpmCache {
    pub store: PathBuf,
    pub cache: PathBuf,
}

#[derive(Serialize)]
struct Identity {
    worktree: String,
    anchor_device: u64,
    anchor_inode: u64,
    checkout_incarnation: Option<String>,
}

/// Scope writable package-manager state to one checkout incarnation. The ID is
/// stored in per-worktree Git metadata, so it survives ordinary Git activity but
/// disappears when the checkout metadata is removed and recreated.
pub(super) fn prepare(cache_root: &Path, workdir: &Path) -> Result<WorkspacePnpmCache, PlanError> {
    let worktree = crate::git::repo_root(workdir).unwrap_or_else(|| workdir.to_owned());
    let worktree = fs::canonicalize(&worktree).map_err(|source| PlanError::DirCreate {
        path: worktree.clone(),
        source,
    })?;
    let git_dir = crate::git::worktree_git_dir(&worktree);
    let anchor = git_dir.as_deref().unwrap_or(&worktree);
    let metadata = fs::symlink_metadata(anchor).map_err(|source| PlanError::DirCreate {
        path: anchor.to_owned(),
        source,
    })?;
    let incarnation = git_dir
        .as_deref()
        .map(checkout_incarnation)
        .transpose()
        .map_err(|source| PlanError::DirCreate {
            path: anchor.join(INCARNATION_FILE),
            source,
        })?;
    let id = identity_hash(
        &worktree,
        metadata.dev(),
        metadata.ino(),
        incarnation.as_deref(),
    );
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
        checkout_incarnation: incarnation,
    };
    let bytes = serde_json::to_vec_pretty(&identity).expect("workspace identity is serializable");
    fs::write(root.join("identity.json"), bytes).map_err(|source| PlanError::DirCreate {
        path: root.join("identity.json"),
        source,
    })?;
    Ok(WorkspacePnpmCache { store, cache })
}

fn checkout_incarnation(git_dir: &Path) -> io::Result<String> {
    let identity_path = git_dir.join(INCARNATION_FILE);
    if let Ok(identity) = read_incarnation(&identity_path) {
        return Ok(identity);
    }

    let mut candidate = tempfile::Builder::new()
        .prefix(".ags-workspace-id-")
        .tempfile_in(git_dir)?;
    let identity = candidate
        .path()
        .file_name()
        .expect("temporary identity has a file name")
        .to_string_lossy()
        .into_owned();
    candidate.as_file_mut().write_all(identity.as_bytes())?;
    candidate.as_file().sync_all()?;
    match fs::hard_link(candidate.path(), &identity_path) {
        Ok(()) => {}
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(error),
    }
    read_incarnation(&identity_path)
}

fn read_incarnation(path: &Path) -> io::Result<String> {
    let identity = fs::read_to_string(path)?;
    let identity = identity.trim();
    if identity.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "workspace identity is empty",
        ));
    }
    Ok(identity.to_owned())
}

fn identity_hash(worktree: &Path, device: u64, inode: u64, incarnation: Option<&str>) -> String {
    let mut hasher = Sha256::new();
    hasher.update(worktree.as_os_str().as_bytes());
    hasher.update([0]);
    hasher.update(device.to_le_bytes());
    hasher.update(inode.to_le_bytes());
    if let Some(incarnation) = incarnation {
        hasher.update([0]);
        hasher.update(incarnation.as_bytes());
    }
    format!("{:x}", hasher.finalize())
}

#[cfg(test)]
#[path = "workspace_cache_tests.rs"]
mod tests;
