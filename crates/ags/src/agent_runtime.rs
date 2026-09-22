//! Immutable agent installations. Published generations are never updated or deleted.
//! Container mount sources (not process names/PIDs) identify users, including stopped
//! containers. Retention also protects launches whose plans have not reached Podman yet.
use std::collections::BTreeSet;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

pub const ROOT: &str = "agent-runtimes";
pub const RUNTIME_DIRS: &[&str] = &[
    "pnpm-home",
    "codex-install",
    "opencode-install",
    "claude-install",
];

/// Read the selection once per launch so every mount belongs to the same generation.
/// Only an absent pointer permits legacy fallback; corrupt selections fail closed.
pub fn selected(cache: &Path) -> io::Result<PathBuf> {
    let root = cache.join(ROOT);
    let pointer = root.join("current");
    match fs::symlink_metadata(&pointer) {
        Ok(metadata) if metadata.is_file() => {}
        Ok(_) => {
            return Err(io::Error::other(
                "agent runtime selection is not a regular file",
            ));
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(cache.to_owned()),
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
    Ok(generation)
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
        // Persist immediately: interruption must not remove files an installer might
        // still have open. Unpublished generations are harmless and never selected.
        let path = tempfile::Builder::new()
            .prefix("generation-")
            .tempdir_in(&root)?
            .keep();
        for suffix in RUNTIME_DIRS.iter().copied().chain(["npm-global"]) {
            fs::create_dir(path.join(suffix))?;
        }
        Ok(Self {
            _lock: lock,
            root,
            path,
        })
    }

    /// Call only after installation and smoke tests succeed.
    pub fn publish(&self) -> io::Result<()> {
        let mut pointer = tempfile::NamedTempFile::new_in(&self.root)?;
        writeln!(
            pointer,
            "{}",
            self.path.file_name().unwrap().to_string_lossy()
        )?;
        pointer.as_file().sync_all()?;
        pointer
            .persist(self.root.join("current"))
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
