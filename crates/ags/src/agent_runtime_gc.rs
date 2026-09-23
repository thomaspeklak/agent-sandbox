//! Cleanup coordination: a short root lock closes the selection/lease race;
//! per-generation shared leases protect delayed launches without blocking updates.
use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use super::{ROOT, RUNTIME_DIRS, Update, inspect_usage, read_selection, selected};

/// Covers the crash window between starting Podman and its container acquiring
/// the candidate lease. A later update collects abandoned candidates after this.
const INCOMPLETE_GRACE: Duration = Duration::from_secs(10 * 60);

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
    _lock: Lock,
}

pub fn pin(cache: &Path) -> io::Result<Arc<Lease>> {
    let root = cache.join(ROOT);
    // Register legacy selections too, including launches before the first update.
    fs::create_dir_all(&root)?;
    let gate = Lock::open(&root.join("cleanup.lock"))?;
    gate.0.lock_shared()?;
    let path = selected(cache)?;
    let lease_path = if path != cache {
        path.join(".lease")
    } else {
        root.join("legacy.lease")
    };
    let lock = Lock::open(&lease_path)?;
    lock.0.lock_shared()?;
    Ok(Arc::new(Lease { path, _lock: lock }))
}

#[derive(Debug, Default)]
pub struct CleanupReport {
    pub removed: Vec<PathBuf>,
    pub retained: Vec<PathBuf>,
}

impl Update {
    /// Call after publishing or discarding an identical candidate. Refresh Podman's
    /// references under the selection gate;
    /// the pre-install snapshot is not sufficient for safe deletion.
    pub fn cleanup(&self) -> io::Result<CleanupReport> {
        let gate = Lock::open(&self.root.join("cleanup.lock"))?;
        gate.0.lock()?;
        let uses = inspect_usage(&self.cache)?;
        let referenced = uses.into_iter().flat_map(|(_, roots)| roots).collect();
        self.cleanup_unlocked(&referenced)
    }

    fn cleanup_unlocked(&self, referenced: &BTreeSet<PathBuf>) -> io::Result<CleanupReport> {
        let current = read_selection(&self.root, "current")?.ok_or_else(|| {
            io::Error::other("cannot clean runtimes without a current generation")
        })?;
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
            if keep.contains(&path) {
                report.retained.push(path);
                continue;
            }
            if self.handle_incomplete(&path, &mut report)? {
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
        self.cleanup_legacy(referenced, &mut report)?;
        report.removed.sort();
        report.retained.sort();
        Ok(report)
    }

    /// Returns true when the path was an incomplete candidate (removed or kept).
    fn handle_incomplete(&self, path: &Path, report: &mut CleanupReport) -> io::Result<bool> {
        let marker = path.join(".installing");
        let metadata = match fs::symlink_metadata(&marker) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(error) => return Err(error),
        };
        if !metadata.is_file() || metadata.file_type().is_symlink() {
            report.retained.push(path.to_owned());
            return Ok(true);
        }
        let age = SystemTime::now()
            .duration_since(metadata.modified()?)
            .unwrap_or_default();
        if age < INCOMPLETE_GRACE {
            report.retained.push(path.to_owned());
            return Ok(true);
        }
        let lease = File::open(&marker)?;
        match lease.try_lock() {
            Ok(()) => {
                fs::remove_dir_all(path)?;
                report.removed.push(path.to_owned());
            }
            Err(std::fs::TryLockError::WouldBlock) => report.retained.push(path.to_owned()),
            Err(error) => return Err(io::Error::other(error)),
        }
        Ok(true)
    }

    fn cleanup_legacy(
        &self,
        referenced: &BTreeSet<PathBuf>,
        report: &mut CleanupReport,
    ) -> io::Result<()> {
        // Treat the old install layout as one generation. Never remove the cache
        // root, npm-global, authentication/settings, or unrelated user caches.
        let cache = &self.cache;
        let mut paths = Vec::new();
        for suffix in RUNTIME_DIRS {
            let path = cache.join(suffix);
            match fs::symlink_metadata(&path) {
                Ok(metadata) if metadata.is_dir() => paths.push(path),
                Ok(_) => {} // Do not follow symlinks or delete unexpected files.
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
        if paths.is_empty() {
            return Ok(());
        }
        if referenced.contains(cache) {
            report.retained.extend(paths);
            return Ok(());
        }
        let lease = Lock::open(&self.root.join("legacy.lease"))?;
        match lease.0.try_lock() {
            Ok(()) => {
                for path in paths {
                    fs::remove_dir_all(&path)?;
                    report.removed.push(path);
                }
            }
            Err(std::fs::TryLockError::WouldBlock) => report.retained.extend(paths),
            Err(error) => return Err(io::Error::other(error)),
        }
        Ok(())
    }
}

#[cfg(test)]
#[path = "agent_runtime_gc_tests.rs"]
mod tests;
