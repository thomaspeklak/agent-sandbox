//! Safe Node.js runtime selection shared by the CLI and sandbox wrappers.
//!
//! Project files are data, never shell programs. Runtime command wrappers do
//! the same validation in the container before asking mise for an installed
//! version.

use std::fs;
use std::path::{Path, PathBuf};

/// Host-side persistent mise data directory, relative to `[sandbox].cache_dir`.
pub const NODE_STORE_SUFFIX: &str = "mise";
/// Container destination for the read-only managed Node installation store.
pub const NODE_STORE_CONTAINER: &str = "/opt/ags/mise";
/// The Node binary supplied by the sandbox image. This is intentionally not
/// selected by mise when a project contains an `.nvmrc`.
pub const NODE_BASELINE_VERSION: &str = "24";

/// Normalize a user or `.nvmrc` version to a safe mise Node version selector.
///
/// Only numeric major/minor/patch selectors are accepted. In particular this
/// rejects shell fragments, aliases such as `lts/*`, and whitespace. This is
/// deliberately narrower than all selectors accepted by mise because an
/// untrusted project file must never become executable input.
pub fn validate_node_version(raw: &str) -> Result<String, String> {
    let value = raw.strip_prefix("node@").unwrap_or(raw);
    let value = value
        .strip_suffix("\r\n")
        .or_else(|| value.strip_suffix('\n'))
        .unwrap_or(value);
    if value.chars().any(char::is_whitespace) {
        return Err(format_invalid_version(raw));
    }
    let value = value
        .strip_prefix('v')
        .or_else(|| value.strip_prefix('V'))
        .unwrap_or(value);
    let parts: Vec<&str> = value.split('.').collect();
    if parts.is_empty() || parts.len() > 3 || parts.iter().any(|part| part.is_empty()) {
        return Err(format_invalid_version(raw));
    }
    if parts
        .iter()
        .any(|part| !part.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return Err(format_invalid_version(raw));
    }
    // Avoid accepting values that mise or a shell could interpret in a
    // surprising way, and reject arbitrarily huge semver components.
    if parts.iter().any(|part| part.len() > 9) {
        return Err(format_invalid_version(raw));
    }
    Ok(parts.join("."))
}

fn format_invalid_version(raw: &str) -> String {
    format!(
        "invalid Node version {raw:?}; expected a numeric major, minor, or patch version such as 22 or 22.14.0"
    )
}

/// Find and validate the nearest regular `.nvmrc` from `start` up to and
/// including `boundary`. A symlink `.nvmrc` is rejected rather than followed.
pub fn nearest_nvmrc(start: &Path, boundary: &Path) -> Result<Option<(PathBuf, String)>, String> {
    let start = start.canonicalize().map_err(|error| {
        format!(
            "could not resolve Node workspace {}: {error}",
            start.display()
        )
    })?;
    let boundary = boundary.canonicalize().map_err(|error| {
        format!(
            "could not resolve Node workspace boundary {}: {error}",
            boundary.display()
        )
    })?;
    if !start.starts_with(&boundary) {
        return Ok(None);
    }

    let mut cursor = start;
    loop {
        let candidate = cursor.join(".nvmrc");
        match fs::symlink_metadata(&candidate) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(format!(
                    ".nvmrc is a symlink and is not allowed: {}",
                    candidate.display()
                ));
            }
            Ok(metadata) if metadata.is_file() => {
                let content = fs::read_to_string(&candidate)
                    .map_err(|error| format!("could not read {}: {error}", candidate.display()))?;
                let version = validate_node_version(&content)
                    .map_err(|error| format!("{}: {error}", candidate.display()))?;
                return Ok(Some((candidate, version)));
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(format!(
                    "could not inspect {}: {error}",
                    candidate.display()
                ));
            }
        }

        if cursor == boundary {
            break;
        }
        let Some(parent) = cursor.parent() else {
            break;
        };
        if parent == cursor || !parent.starts_with(&boundary) {
            break;
        }
        cursor = parent.to_owned();
    }
    Ok(None)
}

#[cfg(test)]
#[path = "node_runtime_tests.rs"]
mod tests;
