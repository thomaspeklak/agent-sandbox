//! Verified runtime identities and post-install content sharing. Installers never
//! receive a directory containing links to a published generation.
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File};
use std::io::{self, Read};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Component, Path};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{RUNTIME_DIRS, Update, read_selection};
const MANIFEST: &str = "runtime-manifest.json";

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RuntimeManifest {
    schema: u32,
    image: String,
    request: serde_json::Value,
    entries: Vec<Entry>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct Entry {
    key: String,
    path: String,
    kind: Kind,
    mode: u32,
    size: u64,
    /// Semantic identity (physical pnpm installation IDs removed from shims/links).
    digest: String,
    /// Exact file SHA-256, or exact symlink target. Never normalize shared bytes.
    raw: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
enum Kind {
    File,
    Directory,
    Link,
}

#[derive(Debug, Default)]
pub struct SharedFiles {
    pub files: u64,
    pub bytes: u64,
}

#[derive(Debug)]
pub enum Publication {
    Unchanged,
    Published(SharedFiles),
}

impl RuntimeManifest {
    pub fn from_inventory(
        image: String,
        request: serde_json::Value,
        inventory: &[u8],
    ) -> io::Result<Self> {
        let mut manifest = Self {
            schema: 1,
            image,
            request,
            entries: serde_json::from_slice(inventory)?,
        };
        manifest.entries.sort_by(|a, b| a.key.cmp(&b.key));
        manifest.validate()?;
        Ok(manifest)
    }

    fn validate(&self) -> io::Result<()> {
        if self.schema != 1 {
            return Err(io::Error::other("unsupported runtime manifest schema"));
        }
        let mut keys = BTreeSet::new();
        for entry in &self.entries {
            let path = Path::new(&entry.path);
            let safe = path.components().all(|c| matches!(c, Component::Normal(_)))
                && RUNTIME_DIRS.iter().any(|root| path.starts_with(root))
                && path.components().count() > 1;
            if !safe || !keys.insert(&entry.key) || entry.mode > 0o7777 {
                return Err(io::Error::other("invalid runtime manifest entry"));
            }
            if entry.kind == Kind::File
                && [&entry.raw, &entry.digest]
                    .iter()
                    .any(|s| s.len() != 64 || !s.bytes().all(|c| c.is_ascii_hexdigit()))
            {
                return Err(io::Error::other("invalid runtime file digest"));
            }
        }
        Ok(())
    }

    fn equivalent(&self, other: &Self) -> bool {
        self.schema == other.schema
            && self.image == other.image
            && self.request == other.request
            && self.entries.len() == other.entries.len()
            && self.entries.iter().zip(&other.entries).all(|(a, b)| {
                a.key == b.key && a.kind == b.kind && a.mode == b.mode && a.digest == b.digest
            })
    }

    fn intact(&self, root: &Path) -> io::Result<bool> {
        for entry in &self.entries {
            let path = root.join(&entry.path);
            // Reject parent symlinks escaping the generation, including on sharing sources.
            if !path.parent().unwrap().canonicalize()?.starts_with(root) {
                return Ok(false);
            }
            let meta = fs::symlink_metadata(&path)?;
            if meta.permissions().mode() & 0o7777 != entry.mode {
                return Ok(false);
            }
            let matches = match entry.kind {
                Kind::Directory => meta.is_dir(),
                Kind::Link => {
                    meta.is_symlink() && fs::read_link(&path)?.to_str() == Some(&entry.raw)
                }
                Kind::File => {
                    meta.is_file() && meta.len() == entry.size && digest(&path)? == entry.raw
                }
            };
            if !matches {
                return Ok(false);
            }
        }
        Ok(true)
    }
}

impl Update {
    /// Only call after the installer and read-only verification containers exit.
    /// Identical candidates are discarded without rotating current/previous.
    pub fn finish(&self, manifest: RuntimeManifest) -> io::Result<Publication> {
        manifest.validate()?;
        if !manifest.intact(&self.path)? {
            return Err(io::Error::other(
                "verified candidate changed before publication",
            ));
        }
        let current = read_selection(&self.root, "current")?;
        let previous = current.as_ref().and_then(|root| {
            let bytes = fs::read(root.join(MANIFEST)).ok()?;
            let old: RuntimeManifest = serde_json::from_slice(&bytes).ok()?;
            old.validate().ok()?;
            old.intact(root).ok().filter(|intact| *intact)?;
            Some(old)
        });
        if previous
            .as_ref()
            .is_some_and(|old| manifest.equivalent(old))
        {
            fs::remove_dir_all(&self.path)?;
            return Ok(Publication::Unchanged);
        }
        let shared = match (current, previous) {
            (Some(root), Some(old)) => share_files(&root, &old, &self.path, &manifest)?,
            _ => SharedFiles::default(),
        };
        fs::write(
            self.path.join(MANIFEST),
            serde_json::to_vec_pretty(&manifest)?,
        )?;
        self.publish()?;
        Ok(Publication::Published(shared))
    }
}

fn digest(path: &Path) -> io::Result<String> {
    let mut file = File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        hash.update(&buffer[..count]);
    }
    Ok(format!("{:x}", hash.finalize()))
}

fn share_files(
    old_root: &Path,
    old: &RuntimeManifest,
    new_root: &Path,
    new: &RuntimeManifest,
) -> io::Result<SharedFiles> {
    let mut sources = BTreeMap::new();
    for entry in old.entries.iter().filter(|e| e.kind == Kind::File) {
        sources
            .entry((&entry.raw, entry.mode, entry.size))
            .or_insert_with(|| old_root.join(&entry.path));
    }
    let mut shared = SharedFiles::default();
    for entry in new.entries.iter().filter(|e| e.kind == Kind::File) {
        let Some(source) = sources.get(&(&entry.raw, entry.mode, entry.size)) else {
            continue;
        };
        let destination = new_root.join(&entry.path);
        let src_meta = fs::symlink_metadata(source)?;
        let dst_meta = fs::symlink_metadata(&destination)?;
        if src_meta.dev() == dst_meta.dev() && src_meta.ino() == dst_meta.ino() {
            continue;
        }
        if src_meta.uid() != dst_meta.uid() || src_meta.gid() != dst_meta.gid() {
            continue;
        }
        let Ok(temp) = tempfile::tempdir_in(destination.parent().unwrap()) else {
            continue; // A package may contain read-only directories; keep its copy.
        };
        let link = temp.path().join("shared");
        // Sharing is an optimization: cross-device/unsupported filesystems keep copies.
        if fs::hard_link(source, &link).is_err() {
            continue;
        }
        if fs::rename(&link, &destination).is_err() {
            continue;
        }
        shared.files += 1;
        shared.bytes += entry.size;
    }
    Ok(shared)
}

#[cfg(test)]
#[path = "agent_runtime_manifest_tests.rs"]
mod tests;
