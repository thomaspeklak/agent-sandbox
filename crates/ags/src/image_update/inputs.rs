//! Canonical component identities. Each key hashes a typed structure of
//! exactly the inputs that may change that component (see the invalidation
//! table in docs/COMMANDS.md); nothing hashes the whole AGS binary,
//! repository, or a monolithic Containerfile.

use serde::Serialize;
use sha2::{Digest, Sha256};

use super::metadata::{PnpmRelease, RustRelease};
use super::platform::Platform;
use crate::assets;
use crate::config::{ArchiveMemberMatch, ToolArchiveFormat};

/// Bump when the meaning of a component key changes.
const KEY_SCHEMA: u32 = 1;

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn key<T: Serialize>(kind: &str, inputs: &T) -> String {
    #[derive(Serialize)]
    struct Envelope<'a, T> {
        schema: u32,
        kind: &'a str,
        inputs: &'a T,
    }
    let bytes = serde_json::to_vec(&Envelope {
        schema: KEY_SCHEMA,
        kind,
        inputs,
    })
    .expect("component inputs serialize");
    format!("{kind}-{}", sha256_hex(&bytes))
}

/// Hash of the named embedded recipes, standing for "installation recipe".
fn recipe(names: &[&str]) -> String {
    let parts: Vec<(&str, &str)> = names
        .iter()
        .map(|name| (*name, assets::image_recipe(name)))
        .collect();
    sha256_hex(&serde_json::to_vec(&parts).expect("recipes serialize"))
}

/// Sorted, de-duplicated package selection: equivalent configurations share one
/// baseline and one set of build arguments.
pub fn canonical_packages(packages: &[String]) -> Vec<String> {
    let mut canonical: Vec<String> = packages
        .iter()
        .map(|package| package.trim().to_owned())
        .filter(|package| !package.is_empty())
        .collect();
    canonical.sort();
    canonical.dedup();
    canonical
}

pub fn foundation_key(platform: &Platform, base_image: &str, epoch: u32) -> String {
    #[derive(Serialize)]
    struct Inputs<'a> {
        platform: &'a Platform,
        base_image: &'a str,
        epoch: u32,
        recipe: String,
    }
    key(
        "foundation",
        &Inputs {
            platform,
            base_image,
            epoch,
            recipe: recipe(&["build-foundation.Containerfile"]),
        },
    )
}

pub fn baseline_key(platform: &Platform, base_image: &str, packages: &[String]) -> String {
    #[derive(Serialize)]
    struct Inputs<'a> {
        platform: &'a Platform,
        base_image: &'a str,
        base_packages: &'a [&'a str],
        extra_packages: Vec<String>,
        recipe: String,
    }
    key(
        "os-baseline",
        &Inputs {
            platform,
            base_image,
            base_packages: crate::config::BASE_DNF_PACKAGES,
            extra_packages: canonical_packages(packages),
            recipe: recipe(&["os-baseline.Containerfile"]),
        },
    )
}

pub fn rust_key(platform: &Platform, release: &RustRelease, rustup: &str) -> String {
    #[derive(Serialize)]
    struct Inputs<'a> {
        platform: &'a Platform,
        triple: &'a str,
        release: &'a RustRelease,
        rustup: &'a str,
        recipe: String,
    }
    key(
        "rust",
        &Inputs {
            platform,
            triple: platform.rust_triple(),
            release,
            rustup,
            recipe: recipe(&["rust.Containerfile", "rust-install.sh"]),
        },
    )
}

pub fn pnpm_key(platform: &Platform, release: &PnpmRelease) -> String {
    #[derive(Serialize)]
    struct Inputs<'a> {
        platform: &'a Platform,
        version: &'a str,
        integrity: &'a str,
        recipe: String,
    }
    key(
        "pnpm",
        &Inputs {
            platform,
            version: &release.version,
            integrity: &release.integrity,
            recipe: recipe(&["pnpm.Containerfile"]),
        },
    )
}

/// Extraction semantics of one vendor executable. Tool IDs, versions,
/// download URLs, and catalog text are deliberately absent: the checksum
/// identifies the payload, and the rest decides what is installed where.
#[derive(Debug, Clone, Serialize)]
pub struct VendorInputs<'a> {
    pub arch: &'a str,
    pub sha256: String,
    pub archive: ToolArchiveFormat,
    pub member: &'a str,
    pub member_match: ArchiveMemberMatch,
    pub install_as: &'a str,
}

pub fn vendor_key(platform: &Platform, inputs: &VendorInputs<'_>) -> String {
    #[derive(Serialize)]
    struct Inputs<'a> {
        platform: &'a Platform,
        artifact: &'a VendorInputs<'a>,
        recipe: String,
    }
    key(
        "vendor",
        &Inputs {
            platform,
            artifact: inputs,
            recipe: recipe(&["vendor-tool.Containerfile"]),
        },
    )
}

/// Glimpse depends on its sources, lockfile, compiler identity, and build
/// foundation, but not on the rustup manager version, pnpm, or vendor tools.
pub fn glimpse_key(platform: &Platform, release: &RustRelease, foundation_key: &str) -> String {
    #[derive(Serialize)]
    struct Inputs<'a> {
        platform: &'a Platform,
        sources: [&'a str; 5],
        compiler: &'a RustRelease,
        foundation: &'a str,
        recipe: String,
    }
    key(
        "glimpse",
        &Inputs {
            platform,
            sources: [
                assets::GLIMPSE_SHIM_CARGO_TOML,
                assets::GLIMPSE_SHIM_CARGO_LOCK,
                assets::GLIMPSE_SHIM_MAIN,
                assets::GLIMPSE_SHIM_SOCKET,
                assets::GLIMPSE_SHIM_BRIDGE,
            ],
            compiler: release,
            foundation: foundation_key,
            recipe: recipe(&["glimpse.Containerfile"]),
        },
    )
}

/// Concrete component image IDs selected for final assembly.
#[derive(Debug, Clone, Serialize)]
pub struct FinalInputs {
    pub os_checkpoint: String,
    pub rust: String,
    pub pnpm: String,
    pub glimpse: String,
    /// `(install_as, image ID)`, sorted by destination.
    pub vendor: Vec<(String, String)>,
}

pub fn final_key(inputs: &FinalInputs) -> String {
    #[derive(Serialize)]
    struct Inputs<'a> {
        components: &'a FinalInputs,
        assembly: [&'a str; 3],
        verification: &'a str,
    }
    key(
        "final",
        &Inputs {
            components: inputs,
            assembly: [assets::CONTAINERFILE, assets::UV_TOML, assets::TMUX_CONF],
            verification: assets::image_recipe("verify-image.sh"),
        },
    )
}

#[cfg(test)]
#[path = "inputs_tests.rs"]
mod tests;
