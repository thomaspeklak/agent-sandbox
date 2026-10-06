//! Small, disposable manifest of reusable image components, one per output
//! image, platform, and Podman storage. Missing, corrupt, or unsupported state
//! is a recoverable cache miss: every recorded image is re-validated before use.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use super::inputs::sha256_hex;
use super::metadata::{PnpmRelease, RustRelease};
use super::platform::Platform;

pub const STATE_SCHEMA: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OutputIdentity {
    /// Fully qualified configured image name (`localhost/...:tag`).
    pub image: String,
    pub platform: Platform,
    /// Podman graph root, distinguishing separate image stores.
    pub storage: String,
}

impl OutputIdentity {
    /// Stable short identifier used for state files, locks, and private tags.
    pub fn id(&self) -> String {
        let canonical = serde_json::to_vec(self).expect("output identity serializes");
        sha256_hex(&canonical)[..16].to_owned()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BaseRecord {
    pub reference: String,
    pub digest: String,
    pub image_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FoundationRecord {
    pub key: String,
    pub image_id: String,
    /// Incremented by `--rebase` so the build foundation is refreshed.
    pub epoch: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OsRecord {
    pub baseline_key: String,
    pub baseline_id: String,
    pub checkpoint_id: String,
    /// SHA-256 of the sorted installed-RPM inventory of the checkpoint.
    pub inventory: String,
    pub packages: usize,
    /// Number of refresh layers on top of the baseline in this lineage.
    pub generation: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RustRecord {
    pub release: RustRelease,
    pub rustup: String,
    pub key: String,
    pub image_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PnpmRecord {
    pub release: PnpmRelease,
    pub key: String,
    pub image_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VendorRecord {
    pub id: String,
    pub version: String,
    pub install_as: String,
    pub key: String,
    pub image_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactRecord {
    pub key: String,
    pub image_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageState {
    pub schema: u32,
    pub output: OutputIdentity,
    pub base: BaseRecord,
    pub foundation: FoundationRecord,
    pub os: OsRecord,
    pub rust: RustRecord,
    pub pnpm: PnpmRecord,
    pub vendor: Vec<VendorRecord>,
    pub glimpse: ArtifactRecord,
    pub final_image: ArtifactRecord,
}

/// Publication in progress: written after verification and before the
/// configured tag moves, removed once `next` is committed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PendingPublication {
    pub schema: u32,
    pub previous_id: Option<String>,
    pub candidate_id: String,
    pub next: ImageState,
}

/// Files of one output below the canonical per-user image-state directory.
#[derive(Debug, Clone)]
pub struct StatePaths {
    pub state: PathBuf,
    pub pending: PathBuf,
    pub lock: PathBuf,
}

impl StatePaths {
    pub fn new(root: &Path, output: &OutputIdentity) -> Self {
        let id = output.id();
        Self {
            state: root.join(format!("{id}.json")),
            pending: root.join(format!("{id}.pending.json")),
            lock: root.join(format!("{id}.lock")),
        }
    }
}

/// Canonical per-user state directory. It deliberately ignores per-config
/// `sandbox.cache_dir`, so two configs building the same image share one state
/// and one lock. `XDG_CACHE_HOME` redirects it.
pub fn state_root() -> io::Result<PathBuf> {
    let root = crate::util::ags_cache_root()?.join("image-state");
    crate::util::ensure_private_dir(&root)?;
    Ok(root)
}

/// Outcome of reading a record; anything but `Found` is a cache miss.
#[derive(Debug)]
pub enum Loaded<T> {
    Missing,
    Unusable(String),
    Found(T),
}

pub fn load_state(path: &Path, output: &OutputIdentity) -> Loaded<ImageState> {
    match load::<ImageState>(path) {
        Loaded::Found(state) if state.schema != STATE_SCHEMA => {
            Loaded::Unusable(format!("unsupported schema {}", state.schema))
        }
        Loaded::Found(state) if state.output != *output => {
            Loaded::Unusable("recorded for a different output".to_owned())
        }
        other => other,
    }
}

pub fn load<T: DeserializeOwned>(path: &Path) -> Loaded<T> {
    match fs::read(path) {
        Err(error) if error.kind() == io::ErrorKind::NotFound => Loaded::Missing,
        Err(error) => Loaded::Unusable(error.to_string()),
        Ok(bytes) => match serde_json::from_slice(&bytes) {
            Ok(value) => Loaded::Found(value),
            Err(error) => Loaded::Unusable(error.to_string()),
        },
    }
}

/// Durably replace `path` with `value`: private temp file, fsync, rename, and
/// directory fsync.
pub fn save_atomic<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    let mut temp = tempfile::NamedTempFile::new_in(dir)?;
    serde_json::to_writer_pretty(&mut temp, value).map_err(io::Error::other)?;
    temp.write_all(b"\n")?;
    temp.as_file().sync_all()?;
    temp.persist(path).map_err(|error| error.error)?;
    fs::File::open(dir)?.sync_all()
}

pub fn remove_if_present(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Err(error) if error.kind() != io::ErrorKind::NotFound => Err(error),
        _ => Ok(()),
    }
}

#[cfg(test)]
#[path = "state_tests.rs"]
pub(crate) mod tests;
