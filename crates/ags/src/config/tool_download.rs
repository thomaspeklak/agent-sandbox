use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path};

use regex::Regex;
use serde::{Deserialize, Serialize};
use url::Url;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
pub enum ToolArchiveFormat {
    #[serde(rename = "zip")]
    Zip,
    #[serde(rename = "tar.gz")]
    TarGz,
    #[serde(rename = "tar.xz")]
    TarXz,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize)]
pub enum ArchiveMemberMatch {
    #[default]
    #[serde(rename = "exact")]
    Exact,
    #[serde(rename = "unique_basename")]
    UniqueBasename,
}

impl ArchiveMemberMatch {
    fn is_exact(&self) -> bool {
        *self == Self::Exact
    }
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
    #[serde(default, skip_serializing_if = "ArchiveMemberMatch::is_exact")]
    pub member_match: ArchiveMemberMatch,
    pub install_as: String,
    pub artifacts: BTreeMap<String, ToolDownloadArtifact>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LockedToolDownload {
    pub id: String,
    pub download: ToolDownloadSource,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GitHubReleaseSource {
    pub repository: String,
    pub release: GitHubReleaseSelection,
    pub archive: ToolArchiveFormat,
    pub member: String,
    #[serde(default, skip_serializing_if = "ArchiveMemberMatch::is_exact")]
    pub member_match: ArchiveMemberMatch,
    pub install_as: String,
    pub assets: GitHubReleaseAssetSelectors,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum GitHubReleaseSelection {
    Latest,
    Version {
        version: String,
        #[serde(default = "default_tag_template")]
        tag_template: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GitHubReleaseAssetSelectors {
    pub x86_64: GitHubReleaseAssetSelector,
    pub aarch64: GitHubReleaseAssetSelector,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GitHubReleaseAssetSelector {
    pub archive: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checksum: Option<String>,
}

fn default_tag_template() -> String {
    "v{version}".to_owned()
}

pub(crate) fn validate_tool_download_source(
    source: &ToolDownloadSource,
    context: &str,
) -> Result<(), String> {
    if source.version.trim().is_empty() {
        return Err(format!("{context}.version must be a non-empty string"));
    }
    validate_archive_member(
        &source.member,
        source.member_match,
        &format!("{context}.member"),
    )?;
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

pub(crate) fn validate_github_release_source(source: &GitHubReleaseSource) -> Result<(), String> {
    let context = "github_release";
    validate_repository(&source.repository, &format!("{context}.repository"))?;
    match &source.release {
        GitHubReleaseSelection::Latest => {}
        GitHubReleaseSelection::Version {
            version,
            tag_template,
        } => {
            if version.trim().is_empty() || version.chars().any(char::is_whitespace) {
                return Err(format!(
                    "{context}.release.version must be a non-empty string without whitespace"
                ));
            }
            if tag_template.matches("{version}").count() != 1
                || tag_template.chars().any(char::is_control)
            {
                return Err(format!(
                    "{context}.release.tag_template must contain exactly one '{{version}}' substitution and no control characters"
                ));
            }
        }
    }
    validate_archive_member(
        &source.member,
        source.member_match,
        &format!("{context}.member"),
    )?;
    if !valid_command_name(&source.install_as) {
        return Err(format!(
            "{context}.install_as must be a command name containing only lowercase letters, digits, '.', '_', and '-'"
        ));
    }
    for (arch, selector) in [
        ("x86_64", &source.assets.x86_64),
        ("aarch64", &source.assets.aarch64),
    ] {
        validate_asset_pattern(
            &selector.archive,
            &format!("{context}.assets.{arch}.archive"),
        )?;
        if let Some(checksum) = &selector.checksum {
            validate_asset_pattern(checksum, &format!("{context}.assets.{arch}.checksum"))?;
        }
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

fn validate_repository(value: &str, context: &str) -> Result<(), String> {
    let mut segments = value.split('/');
    let valid_segment = |segment: Option<&str>| {
        segment.is_some_and(|segment| {
            !segment.is_empty()
                && !matches!(segment, "." | "..")
                && segment
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        })
    };
    if !valid_segment(segments.next())
        || !valid_segment(segments.next())
        || segments.next().is_some()
    {
        return Err(format!(
            "{context} must contain exactly one GitHub owner/repository pair"
        ));
    }
    Ok(())
}

fn validate_asset_pattern(pattern: &str, context: &str) -> Result<(), String> {
    if pattern.chars().any(char::is_control) {
        return Err(format!("{context} must not contain control characters"));
    }
    let trailing_backslashes = pattern
        .strip_suffix('$')
        .map(|prefix| {
            prefix
                .bytes()
                .rev()
                .take_while(|byte| *byte == b'\\')
                .count()
        })
        .unwrap_or(0);
    if !pattern.starts_with('^')
        || !pattern.ends_with('$')
        || !trailing_backslashes.is_multiple_of(2)
    {
        return Err(format!("{context} must be anchored with '^' and '$'"));
    }
    let expanded = pattern
        .replace("{version}", &regex::escape("1.2.3"))
        .replace("{tag}", &regex::escape("v1.2.3"));
    Regex::new(&expanded).map_err(|error| format!("{context} is not a valid regex: {error}"))?;
    Ok(())
}

fn validate_archive_member(
    value: &str,
    member_match: ArchiveMemberMatch,
    context: &str,
) -> Result<(), String> {
    validate_safe_relative_path(value, context)?;
    if member_match == ArchiveMemberMatch::UniqueBasename
        && (value.contains('/') || value.contains('\\'))
    {
        return Err(format!(
            "{context} must be a basename when member_match is 'unique_basename'"
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
        || value.chars().any(char::is_control)
        || value
            .bytes()
            .any(|byte| matches!(byte, b'*' | b'?' | b'[' | b']'))
    {
        return Err(format!(
            "{context} must not begin with '-' or contain archive glob characters or control characters"
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
