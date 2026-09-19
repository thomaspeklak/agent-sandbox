//! Upstream version discovery for image-owned tools, from structured
//! metadata only. Requests are bounded in time and size; malformed or
//! unexpected metadata is an error, never "up to date".

use std::collections::BTreeMap;
use std::process::Command;

use base64::Engine;
use serde::{Deserialize, Serialize};

use super::error::ImageUpdateError;

const RUST_CHANNEL_URL: &str = "https://static.rust-lang.org/dist/channel-rust-stable.toml";
const RUSTUP_RELEASE_URL: &str = "https://static.rust-lang.org/rustup/release-stable.toml";
const PNPM_LATEST_URL: &str = "https://registry.npmjs.org/pnpm/latest";

/// Concrete compiler/component identity of the Rust stable channel.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RustRelease {
    /// `rustc -V` without the `rustc ` prefix, e.g. `1.90.0 (1159e78c4 2025-09-14)`.
    pub rustc: String,
    pub cargo: String,
    pub rustfmt: String,
    pub clippy: String,
}

/// Exact pnpm release selected by npm's `latest` dist-tag.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PnpmRelease {
    pub version: String,
    /// Subresource-integrity string from the registry (`sha512-...`).
    pub integrity: String,
    /// The same digest as lowercase hex, for `sha512sum -c` in the recipe.
    pub sha512: String,
}

pub fn resolve_rust(triple: &str) -> Result<RustRelease, ImageUpdateError> {
    let body = fetch(RUST_CHANNEL_URL, 16 * 1024 * 1024, None)
        .map_err(|message| metadata_error("Rust stable", message))?;
    parse_rust_channel(&body, triple).map_err(|message| metadata_error("Rust stable", message))
}

pub fn resolve_rustup() -> Result<String, ImageUpdateError> {
    let body = fetch(RUSTUP_RELEASE_URL, 64 * 1024, None)
        .map_err(|message| metadata_error("rustup", message))?;
    parse_rustup_release(&body).map_err(|message| metadata_error("rustup", message))
}

pub fn resolve_pnpm() -> Result<PnpmRelease, ImageUpdateError> {
    let body = fetch(PNPM_LATEST_URL, 1024 * 1024, Some("application/json"))
        .map_err(|message| metadata_error("pnpm", message))?;
    parse_pnpm_latest(&body).map_err(|message| metadata_error("pnpm", message))
}

fn metadata_error(component: &'static str, message: String) -> ImageUpdateError {
    ImageUpdateError::Metadata { component, message }
}

fn fetch(url: &str, max_bytes: u64, accept: Option<&str>) -> Result<String, String> {
    let mut command = Command::new("curl");
    command.args([
        "--proto",
        "=https",
        "--proto-redir",
        "=https",
        "--tlsv1.2",
        "-fsSL",
        "--connect-timeout",
        "10",
        "--max-time",
        "60",
        "--retry",
        "2",
        "--retry-delay",
        "1",
        "-H",
        "User-Agent: ags",
        "--max-filesize",
    ]);
    command.arg(max_bytes.to_string());
    if let Some(accept) = accept {
        command.args(["-H", &format!("Accept: {accept}")]);
    }
    let output = command
        .arg(url)
        .output()
        .map_err(|error| format!("could not run curl for {url}: {error}"))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(
            format!("{url}: curl exited with {} {stderr}", output.status)
                .trim_end()
                .to_owned(),
        );
    }
    String::from_utf8(output.stdout).map_err(|_| format!("{url}: response is not UTF-8"))
}

fn is_release_version(version: &str) -> bool {
    let parts: Vec<&str> = version.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
}

pub fn parse_rustup_release(body: &str) -> Result<String, String> {
    #[derive(Deserialize)]
    struct Release {
        #[serde(rename = "schema-version")]
        schema_version: String,
        version: String,
    }
    let release: Release =
        toml::from_str(body).map_err(|error| format!("malformed rustup release: {error}"))?;
    if release.schema_version != "1" {
        return Err(format!(
            "unsupported rustup release schema {}",
            release.schema_version
        ));
    }
    if !is_release_version(&release.version) {
        return Err(format!("unexpected rustup version '{}'", release.version));
    }
    Ok(release.version)
}

pub fn parse_rust_channel(body: &str, triple: &str) -> Result<RustRelease, String> {
    #[derive(Deserialize)]
    struct Channel {
        #[serde(rename = "manifest-version")]
        manifest_version: String,
        pkg: BTreeMap<String, Package>,
    }
    #[derive(Deserialize)]
    struct Package {
        version: String,
        #[serde(default)]
        target: BTreeMap<String, Target>,
    }
    #[derive(Deserialize)]
    struct Target {
        available: bool,
    }

    let channel: Channel =
        toml::from_str(body).map_err(|error| format!("malformed channel manifest: {error}"))?;
    if channel.manifest_version != "2" {
        return Err(format!(
            "unsupported channel manifest version {}",
            channel.manifest_version
        ));
    }
    let version = |name: &str| -> Result<String, String> {
        let package = channel
            .pkg
            .get(name)
            .ok_or_else(|| format!("channel manifest lacks {name}"))?;
        if !package
            .target
            .get(triple)
            .is_some_and(|target| target.available)
        {
            return Err(format!("{name} is not available for {triple}"));
        }
        // rustc reports a plain release; tools may add a channel suffix
        // (`1.8.0-stable`), which is still a release version.
        let release = package.version.split_whitespace().next().unwrap_or("");
        let release = match name {
            "rustc" => release,
            _ => release.strip_suffix("-stable").unwrap_or(release),
        };
        if !is_release_version(release) {
            return Err(format!("unexpected {name} version '{}'", package.version));
        }
        Ok(package.version.clone())
    };
    version("rust-std")?;
    Ok(RustRelease {
        rustc: version("rustc")?,
        cargo: version("cargo")?,
        rustfmt: version("rustfmt-preview")?,
        clippy: version("clippy-preview")?,
    })
}

pub fn parse_pnpm_latest(body: &str) -> Result<PnpmRelease, String> {
    #[derive(Deserialize)]
    struct Latest {
        name: String,
        version: String,
        dist: Dist,
    }
    #[derive(Deserialize)]
    struct Dist {
        integrity: String,
        tarball: String,
    }
    let latest: Latest =
        serde_json::from_str(body).map_err(|error| format!("malformed npm metadata: {error}"))?;
    if latest.name != "pnpm" {
        return Err(format!("npm metadata describes '{}'", latest.name));
    }
    if !is_release_version(&latest.version) {
        return Err(format!(
            "npm latest is '{}', not a stable release; refusing to change channel",
            latest.version
        ));
    }
    let expected_tarball = format!(
        "https://registry.npmjs.org/pnpm/-/pnpm-{}.tgz",
        latest.version
    );
    if latest.dist.tarball != expected_tarball {
        return Err(format!("unexpected tarball URL {}", latest.dist.tarball));
    }
    let digest = latest
        .dist
        .integrity
        .strip_prefix("sha512-")
        .and_then(|encoded| {
            base64::engine::general_purpose::STANDARD
                .decode(encoded)
                .ok()
        })
        .filter(|digest| digest.len() == 64)
        .ok_or_else(|| "integrity is not a sha512 digest".to_owned())?;
    Ok(PnpmRelease {
        version: latest.version,
        integrity: latest.dist.integrity,
        sha512: digest.iter().map(|byte| format!("{byte:02x}")).collect(),
    })
}

#[cfg(test)]
#[path = "metadata_tests.rs"]
mod tests;
