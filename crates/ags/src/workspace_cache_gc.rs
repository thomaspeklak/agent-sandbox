//! Explicit, cron-friendly orphan collection. Launches never sweep caches.
use std::fs;
use std::io::{self, Read};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use super::{Identity, Lock, ROOT, delete, identity_hash, read_incarnation, usage};

#[derive(Debug, Clone)]
pub struct PruneOptions {
    pub dry_run: bool,
    pub grace: Duration,
    pub max_caches: usize,
    pub max_deletions: usize,
}

impl Default for PruneOptions {
    fn default() -> Self {
        Self {
            dry_run: false,
            grace: Duration::from_secs(7 * 24 * 60 * 60),
            max_caches: 2,
            max_deletions: 1000,
        }
    }
}

#[derive(Debug, Default)]
pub struct PruneReport {
    pub orphans: usize,
    pub eligible: Vec<PathBuf>,
    pub removed: Vec<PathBuf>,
    pub pending: Vec<PathBuf>,
    pub deletions: usize,
    pub busy: bool,
}

pub fn prune(cache: &Path, options: &PruneOptions) -> io::Result<PruneReport> {
    prune_with(cache, options, usage::inspect)
}

fn try_lock(lock: &Lock) -> io::Result<bool> {
    match lock.0.try_lock() {
        Ok(()) => Ok(true),
        Err(std::fs::TryLockError::WouldBlock) => Ok(false),
        Err(std::fs::TryLockError::Error(e)) => Err(e),
    }
}

fn prune_with(
    cache: &Path,
    options: &PruneOptions,
    inspect: impl FnOnce() -> io::Result<Vec<PathBuf>>,
) -> io::Result<PruneReport> {
    if options.max_caches == 0 || options.max_deletions < 3 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "cleanup budgets must be positive (at least three deletions)",
        ));
    }
    let root = cache.join(ROOT);
    let mut report = PruneReport::default();
    match fs::symlink_metadata(&root) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(report),
        Ok(m) if m.is_dir() => {}
        Ok(_) => return Err(io::Error::other("workspace cache root must be a directory")),
        Err(e) => return Err(e),
    }
    // Do not let successive cron invocations overlap, even during slow unlinking.
    let run_lock = Lock::open(&root.join("prune.lock"))?;
    if !try_lock(&run_lock)? {
        report.busy = true;
        return Ok(report);
    }
    let now = SystemTime::now();
    let mut candidates = Vec::new();
    // Only read shallow host-owned identities. Never walk package contents here,
    // run Git per checkout, or compute cache sizes.
    for entry in fs::read_dir(&root)? {
        let entry = entry?;
        if !entry.file_type()?.is_dir() {
            continue;
        }
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        let (id, trash) = match name.strip_prefix(".gc-") {
            Some(id) => (id, true),
            None => (name, false),
        };
        if id.len() != 64
            || !id
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
        {
            continue;
        }
        let path = entry.path();
        let Ok(identity) = read_identity(&path) else {
            // A crash between unlinking identity.json and rmdir may leave only
            // the lease or an empty trash directory; no other data is eligible.
            if trash && empty_trash(&path)? {
                candidates.push((path, true));
            }
            continue;
        };
        if !identity.worktree.starts_with('/')
            || identity_hash(
                Path::new(&identity.worktree),
                identity.anchor_device,
                identity.anchor_inode,
                identity.checkout_incarnation.as_deref(),
            ) != id
        {
            continue;
        }
        if trash {
            candidates.push((path, true));
            continue;
        }
        match orphaned(&identity) {
            Ok(false) => {
                if !options.dry_run {
                    remove_marker(&path.join(".orphaned"))?;
                }
            }
            Ok(true) => {
                report.orphans += 1;
                let marker = path.join(".orphaned");
                if orphan_age(&path, now) >= options.grace {
                    candidates.push((path, false));
                } else if !options.dry_run && !marker.try_exists()? {
                    fs::File::options()
                        .write(true)
                        .create_new(true)
                        .open(marker)?;
                }
            }
            Err(_) => {} // Inaccessible/ambiguous checkouts are not proof of disuse.
        }
    }
    if candidates.is_empty() {
        return Ok(report); // No Podman process on the usual no-op path.
    }
    let gate = Lock::open(&root.join("cleanup.lock"))?;
    if !try_lock(&gate)? {
        report.busy = true;
        return Ok(report);
    }
    // Refresh under the launch gate: a snapshot from before the gate has a
    // detached-launch race. Any inspection failure means NO deletion.
    let sources = inspect()?;
    let mut selected = Vec::new();
    for (path, trash) in candidates {
        if selected.len() >= options.max_caches {
            break;
        }
        if usage::referenced(&path, &sources) {
            continue;
        }
        // A checkout may have been restored or launched during the shallow
        // scan. Revalidate identity under the gate before acquiring its lease.
        if !trash
            && (!read_identity(&path)
                .and_then(|identity| orphaned(&identity))
                .unwrap_or(false)
                || orphan_age(&path, SystemTime::now()) < options.grace)
        {
            continue;
        }
        let lease = Lock::open(&path.join(".lease"))?;
        if !try_lock(&lease)? {
            continue;
        }
        report.eligible.push(path.clone());
        if options.dry_run {
            selected.push((path, lease));
            continue;
        }
        let target = if trash {
            path.clone()
        } else {
            root.join(format!(
                ".gc-{}",
                path.file_name().unwrap().to_string_lossy()
            ))
        };
        // Never overwrite a partially collected earlier incarnation.
        if target != path {
            if target.try_exists()? {
                continue;
            }
            fs::rename(&path, &target)?;
        }
        selected.push((target, lease));
    }
    // New launches can proceed while we unlink; their original cache paths no
    // longer resolve to the quarantined tree. Exclusive leases stay pinned.
    drop(gate);
    if !options.dry_run {
        let mut budget = options.max_deletions;
        for (path, _lease) in selected {
            let device = fs::symlink_metadata(&path)?.dev();
            if delete::remove(&path, &mut budget, device)? {
                report.removed.push(path);
            } else {
                report.pending.push(path);
            }
        }
        report.deletions = options.max_deletions - budget;
    }
    Ok(report)
}

fn orphan_age(path: &Path, now: SystemTime) -> Duration {
    fs::symlink_metadata(path.join(".orphaned"))
        .ok()
        .filter(|m| m.is_file())
        .and_then(|m| m.modified().ok())
        .and_then(|t| now.duration_since(t).ok())
        .unwrap_or_default()
}

fn remove_marker(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

fn empty_trash(path: &Path) -> io::Result<bool> {
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        if entry.file_name() != ".lease" || !entry.file_type()?.is_file() {
            return Ok(false);
        }
    }
    Ok(true)
}

fn read_identity(path: &Path) -> io::Result<Identity> {
    let mut bytes = Vec::new();
    let identity = path.join("identity.json");
    if !fs::symlink_metadata(&identity)?.is_file() {
        return Err(io::Error::other(
            "workspace identity must be a regular file",
        ));
    }
    fs::File::open(identity)?
        .take(16 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 16 * 1024 {
        return Err(io::Error::other("oversized workspace identity"));
    }
    serde_json::from_slice(&bytes).map_err(io::Error::other)
}

/// Resolve .git directly, including linked worktrees; no Git subprocesses and
/// no writing/creating checkout metadata during collection. Old identities are
/// supported, but unusual external-GIT_DIR layouts are conservatively retained.
fn orphaned(identity: &Identity) -> io::Result<bool> {
    let worktree = Path::new(&identity.worktree);
    let current = match fs::canonicalize(worktree) {
        Ok(path) => path,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(true),
        Err(e) => return Err(e),
    };
    if current != worktree {
        return Ok(true);
    }
    let dot_git = worktree.join(".git");
    let anchor = match fs::metadata(&dot_git) {
        Ok(m) if m.is_dir() => dot_git,
        Ok(m) if m.is_file() => {
            let mut content = String::new();
            fs::File::open(dot_git)?
                .take(16 * 1024 + 1)
                .read_to_string(&mut content)?;
            if content.len() > 16 * 1024 {
                return Err(io::Error::other("oversized .git file"));
            }
            let path = crate::git::parse_dot_git_file(&content)
                .ok_or_else(|| io::Error::other("unrecognized .git file"))?;
            worktree.join(path)
        }
        Ok(_) => return Err(io::Error::other("unrecognized Git anchor")),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            if identity
                .git_anchor
                .as_ref()
                .is_some_and(|anchor| anchor != &dot_git)
            {
                // External Git dirs may have no .git entry. Do not infer deletion.
                return Err(io::Error::other("Git anchor cannot be resolved"));
            }
            if identity.checkout_incarnation.is_some() {
                return Ok(true);
            }
            worktree.to_owned()
        }
        Err(e) => return Err(e),
    };
    let mut metadata = match fs::symlink_metadata(&anchor) {
        Ok(m) => m,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(true),
        Err(e) => return Err(e),
    };
    // Git may canonicalize a symlinked .git before recording the anchor. Match
    // either the link itself or its target, depending on the stored identity.
    if metadata.is_symlink()
        && (metadata.dev() != identity.anchor_device || metadata.ino() != identity.anchor_inode)
    {
        metadata = fs::metadata(&anchor)?;
    }
    if metadata.dev() != identity.anchor_device || metadata.ino() != identity.anchor_inode {
        return Ok(true);
    }
    if let Some(expected) = &identity.checkout_incarnation {
        match read_incarnation(&anchor.join(super::INCARNATION_FILE)) {
            Ok(current) => return Ok(&current != expected),
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(true),
            Err(e) => return Err(e),
        }
    }
    Ok(false)
}

#[cfg(test)]
#[path = "workspace_cache_race_tests.rs"]
mod race_tests;
#[cfg(test)]
#[path = "workspace_cache_gc_tests.rs"]
mod tests;
