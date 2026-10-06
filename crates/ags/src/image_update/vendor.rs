//! Pinned vendor tools as independent artifacts. Each selected tool is keyed
//! by its checksum and extraction semantics, so one changed lock entry
//! downloads and extracts only that tool.

use std::collections::BTreeSet;
use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::Command;

use sha2::{Digest, Sha256};

use super::components::Foundation;
use super::context::{BuildContext, BuildStep, find_component};
use super::error::ImageUpdateError;
use super::inputs::{self, VendorInputs};
use super::platform::Platform;
use super::state::VendorRecord;
use crate::config::LockedToolDownload;
use crate::podman::LayerCache;

/// Destinations in `/usr/local/bin` owned by the image itself.
const RESERVED_COMMANDS: &[&str] = &["pnpm"];

#[derive(Debug, Clone)]
pub struct VendorSelection<'a> {
    pub tool: &'a LockedToolDownload,
    pub url: &'a str,
    pub inputs: VendorInputs<'a>,
    pub key: String,
}

/// Validate the selected tools for `platform` and compute their keys.
/// Destination conflicts are rejected instead of letting copy order win.
pub fn select<'a>(
    platform: &Platform,
    tools: &'a [LockedToolDownload],
) -> Result<Vec<VendorSelection<'a>>, ImageUpdateError> {
    let arch = platform.vendor_arch();
    let mut destinations = BTreeSet::new();
    let mut selected = Vec::with_capacity(tools.len());
    for tool in tools {
        let source = &tool.download;
        let conflict = |message: String| ImageUpdateError::Download {
            tool: tool.id.clone(),
            message,
        };
        if RESERVED_COMMANDS.contains(&source.install_as.as_str()) {
            return Err(conflict(format!(
                "/usr/local/bin/{} is owned by the sandbox image",
                source.install_as
            )));
        }
        if !destinations.insert(source.install_as.as_str()) {
            return Err(conflict(format!(
                "another selected tool also installs /usr/local/bin/{}",
                source.install_as
            )));
        }
        let artifact = source
            .artifacts
            .get(arch)
            .ok_or_else(|| conflict(format!("no {arch} artifact in the tool lock")))?;
        let inputs = VendorInputs {
            arch,
            sha256: artifact.sha256.to_ascii_lowercase(),
            archive: source.archive,
            member: &source.member,
            member_match: source.member_match,
            install_as: &source.install_as,
        };
        selected.push(VendorSelection {
            tool,
            url: &artifact.url,
            key: inputs::vendor_key(platform, &inputs),
            inputs,
        });
    }
    selected.sort_by(|a, b| a.inputs.install_as.cmp(b.inputs.install_as));
    Ok(selected)
}

pub struct VendorResult {
    pub records: Vec<VendorRecord>,
    pub reused: usize,
    pub built: usize,
    pub removed: usize,
}

pub fn artifacts(
    ctx: &BuildContext,
    foundation: &mut Foundation,
    previous: &[VendorRecord],
    selections: &[VendorSelection<'_>],
) -> Result<VendorResult, ImageUpdateError> {
    let mut result = VendorResult {
        records: Vec::with_capacity(selections.len()),
        reused: 0,
        built: 0,
        removed: previous
            .iter()
            .filter(|old| {
                !selections
                    .iter()
                    .any(|new| new.inputs.install_as == old.install_as)
            })
            .count(),
    };
    for selection in selections {
        let component = format!("vendor-{}", selection.inputs.install_as);
        let recorded = previous
            .iter()
            .find(|old| old.key == selection.key)
            .map(|old| old.image_id.as_str());
        let id = match find_component("vendor", &selection.key, &ctx.platform, recorded)? {
            Some(id) => {
                ctx.hold(&component, &id)?;
                result.reused += 1;
                id
            }
            None => {
                result.built += 1;
                build(ctx, foundation, &component, selection)?
            }
        };
        result.records.push(VendorRecord {
            id: selection.tool.id.clone(),
            version: selection.tool.download.version.clone(),
            install_as: selection.inputs.install_as.to_owned(),
            key: selection.key.clone(),
            image_id: id,
        });
    }
    Ok(result)
}

fn build(
    ctx: &BuildContext,
    foundation: &mut Foundation,
    component: &str,
    selection: &VendorSelection<'_>,
) -> Result<String, ImageUpdateError> {
    let tool = &selection.tool.id;
    let download = |message: String| ImageUpdateError::Download {
        tool: tool.clone(),
        message,
    };
    let store = download_store().map_err(|error| download(error.to_string()))?;
    println!("Fetching {tool} {}", selection.tool.download.version);
    let archive =
        fetch_verified(&store, selection.url, &selection.inputs.sha256).map_err(download)?;
    let context_dir = ctx.root().join(component);
    fs::create_dir_all(&context_dir).map_err(|error| download(error.to_string()))?;
    let staged = context_dir.join("archive");
    if fs::hard_link(&archive, &staged).is_err() {
        fs::copy(&archive, &staged).map_err(|error| download(error.to_string()))?;
    }
    let source = &selection.tool.download;
    let foundation_id = foundation.ensure(ctx)?;
    ctx.build(BuildStep {
        component,
        label_component: "vendor",
        key: &selection.key,
        containerfile: &ctx.recipe("vendor-tool.Containerfile"),
        context_dir: &context_dir,
        build_args: &[
            ("FOUNDATION_IMAGE", foundation_id),
            ("TOOL_ID", tool.clone()),
            ("TOOL_ARCHIVE", archive_name(source.archive).to_owned()),
            ("TOOL_MEMBER", source.member.clone()),
            (
                "TOOL_MEMBER_MATCH",
                member_match_name(source.member_match).to_owned(),
            ),
            ("TOOL_INSTALL_AS", source.install_as.clone()),
            ("TOOL_SHA256", selection.inputs.sha256.clone()),
        ],
        cache: LayerCache::Reuse,
    })
}

fn archive_name(format: crate::config::ToolArchiveFormat) -> &'static str {
    use crate::config::ToolArchiveFormat::*;
    match format {
        Zip => "zip",
        TarGz => "tar.gz",
        TarXz => "tar.xz",
    }
}

fn member_match_name(member_match: crate::config::ArchiveMemberMatch) -> &'static str {
    match member_match {
        crate::config::ArchiveMemberMatch::Exact => "exact",
        crate::config::ArchiveMemberMatch::UniqueBasename => "unique_basename",
    }
}

/// Private store of verified archives, addressed by SHA-256.
pub fn download_store() -> io::Result<PathBuf> {
    let store = crate::util::ags_cache_root()?.join("image-build/downloads");
    crate::util::ensure_private_dir(&store)?;
    Ok(store)
}

pub fn file_sha256(path: &Path) -> io::Result<String> {
    let mut file = fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect())
}

/// Return the stored archive for `sha256` only if its content still matches;
/// a corrupt or poisoned entry is removed so it is fetched again.
pub fn cached_archive(store: &Path, sha256: &str) -> Option<PathBuf> {
    let path = store.join(format!("sha256-{sha256}"));
    match file_sha256(&path) {
        Ok(actual) if actual == sha256 => Some(path),
        Ok(_) => {
            let _ = fs::remove_file(&path);
            None
        }
        Err(_) => None,
    }
}

/// Download into a temporary file, verify, then atomically publish it.
pub fn fetch_verified(store: &Path, url: &str, sha256: &str) -> Result<PathBuf, String> {
    if let Some(path) = cached_archive(store, sha256) {
        return Ok(path);
    }
    let temp = tempfile::NamedTempFile::new_in(store).map_err(|error| error.to_string())?;
    let output = Command::new("curl")
        .args([
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--tlsv1.2",
            "-fsSL",
            "--connect-timeout",
            "10",
            "--max-time",
            "300",
            "--retry",
            "2",
            "--retry-delay",
            "1",
            "-o",
        ])
        .arg(temp.path())
        .arg(url)
        .output()
        .map_err(|error| format!("could not run curl: {error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(format!(
            "{url}: curl exited with {} {stderr}",
            output.status
        ));
    }
    let actual = file_sha256(temp.path()).map_err(|error| error.to_string())?;
    if actual != sha256 {
        return Err(format!(
            "{url}: SHA-256 {actual} does not match the locked {sha256}"
        ));
    }
    let target = store.join(format!("sha256-{sha256}"));
    temp.persist(&target)
        .map_err(|error| error.error.to_string())?;
    Ok(target)
}

#[cfg(test)]
#[path = "vendor_tests.rs"]
mod tests;
