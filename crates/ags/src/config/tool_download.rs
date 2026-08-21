use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path};

use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub enum ToolArchiveFormat {
    #[serde(rename = "zip")]
    Zip,
    #[serde(rename = "tar.gz")]
    TarGz,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolDownloadArtifact {
    pub url: String,
    pub sha256: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolDownloadSource {
    pub version: String,
    pub archive: ToolArchiveFormat,
    pub member: String,
    pub install_as: String,
    pub artifacts: BTreeMap<String, ToolDownloadArtifact>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LockedToolDownload {
    pub id: String,
    pub download: ToolDownloadSource,
}

pub(crate) fn validate_tool_download_source(
    source: &ToolDownloadSource,
    context: &str,
) -> Result<(), String> {
    if source.version.trim().is_empty() {
        return Err(format!("{context}.version must be a non-empty string"));
    }
    validate_safe_relative_path(&source.member, &format!("{context}.member"))?;
    if !valid_command_name(&source.install_as) {
        return Err(format!(
            "{context}.install_as must be a command name containing only lowercase letters, digits, '.', '_', and '-'"
        ));
    }

    let expected_arches = BTreeSet::from(["aarch64", "x86_64"]);
    let actual_arches = source
        .artifacts
        .keys()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    if actual_arches != expected_arches {
        return Err(format!(
            "{context}.artifacts must define exactly 'x86_64' and 'aarch64'"
        ));
    }
    for (arch, artifact) in &source.artifacts {
        validate_artifact(artifact, &format!("{context}.artifacts.{arch}"))?;
    }
    Ok(())
}

pub(crate) fn validate_locked_tool_downloads(
    downloads: &[LockedToolDownload],
    context: &str,
) -> Result<(), String> {
    let mut ids = BTreeSet::new();
    let mut commands = BTreeSet::new();
    for (index, tool) in downloads.iter().enumerate() {
        let item_context = format!("{context}[{index}]");
        if !valid_tool_id(&tool.id) {
            return Err(format!("{item_context}.id must use lower-kebab-case"));
        }
        if !ids.insert(tool.id.as_str()) {
            return Err(format!("{context} repeats tool id '{}'", tool.id));
        }
        validate_tool_download_source(&tool.download, &format!("{item_context}.download"))?;
        if !commands.insert(tool.download.install_as.as_str()) {
            return Err(format!(
                "{context} assigns command '{}' more than once",
                tool.download.install_as
            ));
        }
    }
    Ok(())
}

pub(crate) fn valid_tool_id(id: &str) -> bool {
    let mut chars = id.chars();
    chars.next().is_some_and(|ch| ch.is_ascii_lowercase())
        && chars.all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-')
        && !id.ends_with('-')
        && !id.contains("--")
}

fn validate_artifact(artifact: &ToolDownloadArtifact, context: &str) -> Result<(), String> {
    let url = Url::parse(&artifact.url).map_err(|error| format!("{context}.url: {error}"))?;
    if url.scheme() != "https"
        || !url.has_host()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err(format!(
            "{context}.url must be an HTTPS URL without credentials or a fragment"
        ));
    }
    if artifact.sha256.len() != 64 || !artifact.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(format!(
            "{context}.sha256 must contain 64 hexadecimal digits"
        ));
    }
    Ok(())
}

fn validate_safe_relative_path(value: &str, context: &str) -> Result<(), String> {
    let path = Path::new(value);
    if value.is_empty()
        || path.is_absolute()
        || !path
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
    {
        return Err(format!(
            "{context} must be a non-empty relative path without '.' or '..'"
        ));
    }
    if value.starts_with('-')
        || value
            .bytes()
            .any(|byte| matches!(byte, b'*' | b'?' | b'[' | b']'))
    {
        return Err(format!(
            "{context} must not begin with '-' or contain archive glob characters"
        ));
    }
    Ok(())
}

fn valid_command_name(value: &str) -> bool {
    let mut chars = value.chars();
    chars.next().is_some_and(|ch| ch.is_ascii_lowercase())
        && chars.all(|ch| {
            ch.is_ascii_lowercase() || ch.is_ascii_digit() || matches!(ch, '.' | '_' | '-')
        })
}
