use std::fs;
use std::io::{self, Read, Write};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};

#[path = "workspace_cache_delete.rs"]
mod delete;
#[path = "workspace_cache_gc.rs"]
mod gc;
#[path = "workspace_cache_usage.rs"]
mod usage;
pub use gc::{PruneOptions, PruneReport, prune};
use sha2::{Digest, Sha256};

use crate::plan::PlanError;

pub(crate) const STORE_CONTAINER: &str = "/var/cache/ags/pnpm/store";
pub(crate) const CACHE_CONTAINER: &str = "/var/cache/ags/pnpm/cache";
const INCARNATION_FILE: &str = "ags-workspace-id";

pub(crate) struct WorkspacePnpmCache {
    pub store: PathBuf,
    pub cache: PathBuf,
    pub lease: Arc<Lease>,
}

/// Shared until the last launch-plan owner exits; never mounted into a sandbox.
#[derive(Debug)]
pub struct Lease {
    _lock: Lock,
}

#[derive(Debug)]
struct Lock(fs::File);

impl Lock {
    fn open(path: &Path) -> io::Result<Self> {
        fs::File::options()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(path)
            .map(Self)
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}

const ROOT: &str = "workspace-caches";

#[derive(Deserialize, Serialize)]
struct Identity {
    worktree: String,
    anchor_device: u64,
    anchor_inode: u64,
    checkout_incarnation: Option<String>,
    #[serde(default)]
    git_anchor: Option<PathBuf>,
}

/// Scope writable package-manager state to one checkout incarnation. The ID is
/// stored in per-worktree Git metadata, so it survives ordinary Git activity but
/// disappears when the checkout metadata is removed and recreated.
pub(crate) fn prepare(cache_root: &Path, workdir: &Path) -> Result<WorkspacePnpmCache, PlanError> {
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
    let parent = cache_root.join(ROOT);
    let coordinated = || -> io::Result<(Lock, Lock)> {
        fs::create_dir_all(&parent)?;
        let gate = Lock::open(&parent.join("cleanup.lock"))?;
        gate.0.lock_shared()?;
        let root = parent.join(&id);
        fs::create_dir_all(&root)?;
        let lease = Lock::open(&root.join(".lease"))?;
        lease.0.lock_shared()?;
        Ok((gate, lease))
    };
    let (_gate, lease) = coordinated().map_err(|source| PlanError::DirCreate {
        path: parent.clone(),
        source,
    })?;
    let root = parent.join(id);
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
        git_anchor: git_dir,
    };
    let bytes = serde_json::to_vec_pretty(&identity).expect("workspace identity is serializable");
    let publish = || -> io::Result<()> {
        let mut file = tempfile::NamedTempFile::new_in(&root)?;
        file.write_all(&bytes)?;
        file.persist(root.join("identity.json"))
            .map_err(|e| e.error)?;
        match fs::remove_file(root.join(".orphaned")) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e),
        }
    };
    publish().map_err(|source| PlanError::DirCreate {
        path: root.join("identity.json"),
        source,
    })?;
    Ok(WorkspacePnpmCache {
        store,
        cache,
        lease: Arc::new(Lease { _lock: lease }),
    })
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
    let mut identity = String::new();
    fs::File::open(path)?
        .take(4097)
        .read_to_string(&mut identity)?;
    if identity.len() > 4096 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "workspace identity is oversized",
        ));
    }
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
