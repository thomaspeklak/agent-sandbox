//! Cleanup coordination: a short root lock closes the selection/lease race;
//! per-generation shared leases protect delayed launches without blocking updates.
use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::{ROOT, Update, inspect_usage, read_selection, selected};

#[derive(Debug)]
struct Lock(File);

impl Lock {
    fn open(path: &Path) -> io::Result<Self> {
        File::options()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(path)
            .map(Self)
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}

/// Held until the last launch-plan clone is dropped (normally when Podman exits).
#[derive(Debug)]
pub struct Lease {
    pub path: PathBuf,
    _lock: Option<Lock>,
}

pub fn pin(cache: &Path) -> io::Result<Arc<Lease>> {
    let root = cache.join(ROOT);
    if !root.exists() {
        // Legacy directories are never garbage collected.
        return Ok(Arc::new(Lease {
            path: cache.to_owned(),
            _lock: None,
        }));
    }
    let gate = Lock::open(&root.join("cleanup.lock"))?;
    gate.0.lock_shared()?;
    let path = selected(cache)?;
    let lock = if path != cache {
        let lock = Lock::open(&path.join(".lease"))?;
        lock.0.lock_shared()?;
        Some(lock)
    } else {
        None
    };
    Ok(Arc::new(Lease { path, _lock: lock }))
}

#[derive(Debug, Default)]
pub struct CleanupReport {
    pub removed: Vec<PathBuf>,
    pub retained: Vec<PathBuf>,
}

impl Update {
    /// Call after publishing. Refresh Podman's references under the selection gate;
    /// the pre-install snapshot is not sufficient for safe deletion.
    pub fn cleanup(&self, cache: &Path) -> io::Result<CleanupReport> {
        let gate = Lock::open(&self.root.join("cleanup.lock"))?;
        gate.0.lock()?;
        let uses = inspect_usage(cache)?;
        let referenced = uses.into_iter().flat_map(|(_, roots)| roots).collect();
        self.cleanup_unlocked(&referenced)
    }

    fn cleanup_unlocked(&self, referenced: &BTreeSet<PathBuf>) -> io::Result<CleanupReport> {
        let current = read_selection(&self.root, "current")?.ok_or_else(|| {
            io::Error::other("cannot clean runtimes without a current generation")
        })?;
        if current != self.path {
            return Err(io::Error::other(
                "cleanup requires the successfully published generation",
            ));
        }
        let previous = read_selection(&self.root, "previous")?;
        let mut keep = referenced.clone();
        keep.insert(current);
        keep.extend(previous);
        let mut report = CleanupReport::default();
        for entry in fs::read_dir(&self.root)? {
            let entry = entry?;
            // Do not follow symlinks or remove unrelated/legacy cache directories.
            if !entry.file_type()?.is_dir()
                || !entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("generation-")
            {
                continue;
            }
            let path = entry.path();
            if keep.contains(&path) || path.join(".installing").try_exists()? {
                report.retained.push(path);
                continue;
            }
            let lease = Lock::open(&path.join(".lease"))?;
            match lease.0.try_lock() {
                Ok(()) => {
                    fs::remove_dir_all(&path)?;
                    report.removed.push(path);
                }
                Err(std::fs::TryLockError::WouldBlock) => report.retained.push(path),
                Err(error) => return Err(io::Error::other(error)),
            }
        }
        report.removed.sort();
        report.retained.sort();
        Ok(report)
    }
}

#[cfg(test)]
#[path = "agent_runtime_gc_tests.rs"]
mod tests;
