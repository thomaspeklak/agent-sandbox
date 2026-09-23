//! Immutable agent installations. Container mount sources identify users, including
//! stopped containers. Cleanup retains current/previous generations and launch leases.
use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

#[path = "agent_runtime_gc.rs"]
mod gc;
pub use gc::{CleanupReport, Lease, pin};
#[path = "agent_runtime_manifest.rs"]
mod manifest;
pub use manifest::{Publication, RuntimeManifest};

pub const ROOT: &str = "agent-runtimes";
pub const RUNTIME_DIRS: &[&str] = &[
    "pnpm-home",
    "codex-install",
    "opencode-install",
    "claude-install",
];

/// Read the selection for status reporting. Use `pin` when accessing runtime files
/// or building a launch: an unleased path can be collected after later updates.
/// Only an absent pointer permits legacy fallback; corrupt selections fail closed.
pub fn selected(cache: &Path) -> io::Result<PathBuf> {
    Ok(read_selection(&cache.join(ROOT), "current")?.unwrap_or_else(|| cache.to_owned()))
}

fn read_selection(root: &Path, selector: &str) -> io::Result<Option<PathBuf>> {
    let pointer = root.join(selector);
    match fs::symlink_metadata(&pointer) {
        Ok(metadata) if metadata.is_file() => {}
        Ok(_) => {
            return Err(io::Error::other(
                "agent runtime selection is not a regular file",
            ));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    }
    let name = fs::read_to_string(pointer)?;
    let name = name.trim();
    let mut components = Path::new(name).components();
    if !matches!(components.next(), Some(Component::Normal(_)))
        || components.next().is_some()
        || !name.starts_with("generation-")
    {
        return Err(io::Error::other(
            "invalid agent runtime generation selection",
        ));
    }
    let root = root.canonicalize()?;
    let generation = root.join(name).canonicalize()?;
    if generation.parent() != Some(root.as_path()) || !generation.is_dir() {
        return Err(io::Error::other("agent generation escapes runtime root"));
    }
    for suffix in RUNTIME_DIRS {
        if !generation.join(suffix).is_dir() {
            return Err(io::Error::other(format!("agent generation lacks {suffix}")));
        }
    }
    Ok(Some(generation))
}

pub fn mount_source(cache: &Path, selected: &Path, suffix: &str) -> PathBuf {
    if RUNTIME_DIRS.contains(&suffix) {
        selected.join(suffix)
    } else {
        cache.join(suffix)
    }
}

pub struct Update {
    // OS lock is released even if the updater crashes. Never unlink the lock file.
    _lock: File,
    // Shared with installer/verification containers through the .installing marker.
    _candidate_lease: File,
    cache: PathBuf,
    root: PathBuf,
    pub path: PathBuf,
}

impl Update {
    pub fn begin(cache: &Path) -> io::Result<Self> {
        let root = cache.join(ROOT);
        fs::create_dir_all(&root)?;
        let root = root.canonicalize()?;
        let lock = File::options()
            .create(true)
            .truncate(false)
            .write(true)
            .open(root.join("update.lock"))?;
        lock.try_lock().map_err(|error| {
            io::Error::other(format!(
                "cannot lock agent updates (another update may be running): {error}"
            ))
        })?;
        // Keep TempDir ownership until initialization completes so an ordinary
        // setup error cannot itself leave an incomplete candidate behind.
        let candidate = tempfile::Builder::new()
            .prefix("generation-")
            .tempdir_in(&root)?;
        let path = candidate.path();
        // Installer containers share-lock this marker. Cleanup only collects an
        // unlocked, unreferenced marker after the crash-startup grace period.
        let marker = path.join(".installing");
        fs::write(&marker, "")?;
        let candidate_lease = File::open(&marker)?;
        candidate_lease.lock_shared()?;
        for suffix in RUNTIME_DIRS.iter().copied().chain(["npm-global"]) {
            fs::create_dir(path.join(suffix))?;
        }
        let cache = cache.canonicalize()?;
        let path = candidate.keep();
        Ok(Self {
            _lock: lock,
            _candidate_lease: candidate_lease,
            cache,
            root,
            path,
        })
    }

    /// Call only after installation and smoke tests succeed.
    pub fn publish(&self) -> io::Result<()> {
        if let Some(previous) = read_selection(&self.root, "current")?
            && previous != self.path
        {
            self.write_selection("previous", &previous)?;
        }
        match fs::remove_file(self.path.join(".installing")) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        self.write_selection("current", &self.path)
    }

    fn write_selection(&self, selector: &str, path: &Path) -> io::Result<()> {
        let mut pointer = tempfile::NamedTempFile::new_in(&self.root)?;
        writeln!(pointer, "{}", path.file_name().unwrap().to_string_lossy())?;
        pointer.as_file().sync_all()?;
        pointer
            .persist(self.root.join(selector))
            .map_err(|error| error.error)?;
        File::open(&self.root)?.sync_all()
    }
}

impl Drop for Update {
    fn drop(&mut self) {
        // Explicit unlock avoids transient lock retention by unrelated forked
        // children between fork and exec in a multithreaded host process.
        let _ = self._lock.unlock();
    }
}

#[derive(Debug, Deserialize)]
pub struct ContainerUse {
    #[serde(rename = "Name")]
    pub name: String,
    #[serde(rename = "State")]
    pub state: ContainerState,
    #[serde(rename = "Mounts")]
    mounts: Vec<ContainerMount>,
}

#[derive(Debug, Deserialize)]
pub struct ContainerState {
    #[serde(rename = "Status")]
    pub status: String,
}

#[derive(Debug, Deserialize)]
struct ContainerMount {
    #[serde(rename = "Source", default)]
    source: PathBuf,
}

/// Query the same Podman context used to launch sandboxes. Include *all* containers:
/// stopped containers still reference their generation and can be restarted.
pub fn inspect_usage(cache: &Path) -> io::Result<Vec<(ContainerUse, BTreeSet<PathBuf>)>> {
    let ids = podman_output(&["ps", "--all", "--quiet", "--no-trunc"])?;
    let mut uses = Vec::new();
    for id in ids.split_whitespace() {
        let json = podman_output(&["container", "inspect", id])?;
        let containers: Vec<ContainerUse> = serde_json::from_str(&json)?;
        if containers.len() != 1 {
            return Err(io::Error::other(format!(
                "expected one inspected container for {id}"
            )));
        }
        for container in containers {
            let roots = referenced_roots(cache, &container);
            if !roots.is_empty() {
                uses.push((container, roots));
            }
        }
    }
    Ok(uses)
}

fn podman_output(args: &[&str]) -> io::Result<String> {
    let output = Command::new("podman").args(args).output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "podman {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    String::from_utf8(output.stdout).map_err(io::Error::other)
}

fn referenced_roots(cache: &Path, container: &ContainerUse) -> BTreeSet<PathBuf> {
    let cache = cache.canonicalize().unwrap_or_else(|_| cache.to_owned());
    let root = cache.join(ROOT);
    let root = root.canonicalize().unwrap_or(root);
    let mut roots = BTreeSet::new();
    for mount in &container.mounts {
        if !mount.source.is_absolute() {
            continue; // tmpfs and other non-bind mounts can lack a host source.
        }
        let source = mount
            .source
            .canonicalize()
            .unwrap_or_else(|_| mount.source.clone());
        if let Ok(relative) = source.strip_prefix(&root) {
            if let Some(Component::Normal(name)) = relative.components().next()
                && name.to_string_lossy().starts_with("generation-")
            {
                roots.insert(root.join(name));
            }
        } else if RUNTIME_DIRS
            .iter()
            .any(|suffix| source.starts_with(cache.join(suffix)))
        {
            roots.insert(cache.clone());
        }
    }
    roots
}

#[cfg(test)]
#[path = "agent_runtime_tests.rs"]
mod tests;
