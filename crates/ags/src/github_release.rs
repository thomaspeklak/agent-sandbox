use std::collections::BTreeMap;
use std::fmt;

use regex::Regex;
use serde::Deserialize;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use url::Url;

use crate::config::{
    GitHubReleaseAssetSelector, GitHubReleaseSelection, GitHubReleaseSource, ToolDownloadArtifact,
    ToolDownloadSource, validate_github_release_source, validate_tool_download_source,
};
use crate::github_release_http::fetch_url;

const RELEASES_PER_PAGE: usize = 100;
const MAX_REJECTION_DIAGNOSTICS: usize = 12;

#[derive(Debug)]
pub enum GitHubReleaseError {
    InvalidSource(String),
    Fetch {
        repo: String,
        message: String,
    },
    Parse {
        repo: String,
        message: String,
    },
    NoCompatibleRelease {
        repo: String,
        minimum_release_age: u32,
        rejections: Vec<String>,
    },
    IncompatibleVersion {
        repo: String,
        tag: String,
        message: String,
    },
}

impl fmt::Display for GitHubReleaseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSource(message) => write!(f, "invalid GitHub release source: {message}"),
            Self::Fetch { repo, message } => {
                write!(f, "{repo}: could not fetch GitHub release data: {message}")
            }
            Self::Parse { repo, message } => {
                write!(f, "{repo}: could not parse GitHub release data: {message}")
            }
            Self::NoCompatibleRelease {
                repo,
                minimum_release_age,
                rejections,
            } => {
                write!(
                    f,
                    "{repo}: no compatible stable release published at least {minimum_release_age} minutes ago"
                )?;
                if !rejections.is_empty() {
                    write!(f, "; rejected {}", rejections.join("; "))?;
                }
                Ok(())
            }
            Self::IncompatibleVersion { repo, tag, message } => {
                write!(
                    f,
                    "{repo}: requested release {tag} is incompatible: {message}"
                )
            }
        }
    }
}

impl std::error::Error for GitHubReleaseError {}

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    published_at: Option<String>,
    assets: Vec<GitHubReleaseAsset>,
}

#[derive(Debug, Deserialize)]
struct GitHubReleaseAsset {
    name: String,
    state: String,
    size: u64,
    #[serde(default)]
    browser_download_url: String,
    #[serde(default)]
    digest: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum FetchRequest {
    ReleasesPage(usize),
    ReleaseByTag(String),
    Asset(String),
}

enum ReleaseResolutionError {
    Incompatible(String),
    Fetch(GitHubReleaseError),
}

impl From<String> for ReleaseResolutionError {
    fn from(message: String) -> Self {
        Self::Incompatible(message)
    }
}

/// Resolve a catalog GitHub source into an immutable, verified download lock entry.
pub fn resolve_github_release_source(
    source: &GitHubReleaseSource,
    minimum_release_age: u32,
) -> Result<ToolDownloadSource, GitHubReleaseError> {
    resolve_github_release_source_with(
        source,
        minimum_release_age,
        OffsetDateTime::now_utc(),
        |request| fetch_request(&source.repository, request),
    )
}

fn resolve_github_release_source_with<F>(
    source: &GitHubReleaseSource,
    minimum_release_age: u32,
    now: OffsetDateTime,
    mut fetch: F,
) -> Result<ToolDownloadSource, GitHubReleaseError>
where
    F: FnMut(FetchRequest) -> Result<Vec<u8>, GitHubReleaseError>,
{
    validate_github_release_source(source).map_err(GitHubReleaseError::InvalidSource)?;
    match &source.release {
        GitHubReleaseSelection::Latest => {
            resolve_latest_source(source, minimum_release_age, now, &mut fetch)
        }
        GitHubReleaseSelection::Version {
            version,
            tag_template,
        } => {
            let tag = tag_template.replace("{version}", version);
            let body = fetch(FetchRequest::ReleaseByTag(tag.clone()))?;
            let release = parse_release(&source.repository, &body)?;
            if release.tag_name != tag {
                return Err(GitHubReleaseError::IncompatibleVersion {
                    repo: source.repository.clone(),
                    tag,
                    message: format!("GitHub returned tag '{}'", release.tag_name),
                });
            }
            if release.draft {
                return Err(GitHubReleaseError::IncompatibleVersion {
                    repo: source.repository.clone(),
                    tag,
                    message: "release is a draft".to_owned(),
                });
            }
            match materialize_release(source, &release, version, &mut fetch) {
                Ok(download) => Ok(download),
                Err(ReleaseResolutionError::Incompatible(message)) => {
                    Err(GitHubReleaseError::IncompatibleVersion {
                        repo: source.repository.clone(),
                        tag,
                        message,
                    })
                }
                Err(ReleaseResolutionError::Fetch(error)) => Err(error),
            }
        }
    }
}

fn resolve_latest_source<F>(
    source: &GitHubReleaseSource,
    minimum_release_age: u32,
    now: OffsetDateTime,
    fetch: &mut F,
) -> Result<ToolDownloadSource, GitHubReleaseError>
where
    F: FnMut(FetchRequest) -> Result<Vec<u8>, GitHubReleaseError>,
{
    let mut rejections = Vec::new();
    for release in stable_releases_newest_first(source, fetch)? {
        let version = version_from_stable_tag(&release.tag_name)
            .expect("stable releases carry a parsed version tag");
        if !is_mature(&release, minimum_release_age, now) {
            push_rejection(&mut rejections, &release.tag_name, "release is immature");
            continue;
        }
        match materialize_release(source, &release, version, fetch) {
            Ok(download) => return Ok(download),
            Err(ReleaseResolutionError::Incompatible(message)) => {
                push_rejection(&mut rejections, &release.tag_name, &message)
            }
            Err(ReleaseResolutionError::Fetch(error)) => return Err(error),
        }
    }
    Err(GitHubReleaseError::NoCompatibleRelease {
        repo: source.repository.clone(),
        minimum_release_age,
        rejections,
    })
}

/// Lists every stable `vX.Y.Z` release ordered by version, highest first, because
/// GitHub orders by creation time and a late backport patch must not win.
fn stable_releases_newest_first<F>(
    source: &GitHubReleaseSource,
    fetch: &mut F,
) -> Result<Vec<GitHubRelease>, GitHubReleaseError>
where
    F: FnMut(FetchRequest) -> Result<Vec<u8>, GitHubReleaseError>,
{
    let mut stable = Vec::new();
    let mut page = 1;
    loop {
        let body = fetch(FetchRequest::ReleasesPage(page))?;
        let releases = parse_release_page(&source.repository, &body)?;
        let page_len = releases.len();
        stable.extend(releases.into_iter().filter(|release| {
            !release.draft
                && !release.prerelease
                && version_from_stable_tag(&release.tag_name).is_some()
        }));
        if page_len < RELEASES_PER_PAGE {
            break;
        }
        page += 1;
    }
    stable.sort_by_cached_key(|release| {
        std::cmp::Reverse(
            stable_version_key(&release.tag_name).expect("filtered to stable version tags"),
        )
    });
    Ok(stable)
}

fn stable_version_key(tag: &str) -> Option<[u64; 3]> {
    let mut parts = version_from_stable_tag(tag)?
        .split('.')
        .map(|part| part.parse::<u64>().ok());
    Some([parts.next()??, parts.next()??, parts.next()??])
}

fn materialize_release<F>(
    source: &GitHubReleaseSource,
    release: &GitHubRelease,
    version: &str,
    fetch: &mut F,
) -> Result<ToolDownloadSource, ReleaseResolutionError>
where
    F: FnMut(FetchRequest) -> Result<Vec<u8>, GitHubReleaseError>,
{
    let mut checksum_cache = BTreeMap::new();
    let mut artifacts = BTreeMap::new();
    for (arch, selector) in [
        ("x86_64", &source.assets.x86_64),
        ("aarch64", &source.assets.aarch64),
    ] {
        let artifact =
            resolve_artifact(release, selector, version, arch, fetch, &mut checksum_cache)?;
        artifacts.insert(arch.to_owned(), artifact);
    }
    let download = ToolDownloadSource {
        version: version.to_owned(),
        archive: source.archive,
        member: source.member.clone(),
        member_match: source.member_match,
        install_as: source.install_as.clone(),
        artifacts,
    };
    validate_tool_download_source(&download, "resolved github release")?;
    Ok(download)
}

fn resolve_artifact<F>(
    release: &GitHubRelease,
    selector: &GitHubReleaseAssetSelector,
    version: &str,
    arch: &str,
    fetch: &mut F,
    checksum_cache: &mut BTreeMap<String, Vec<u8>>,
) -> Result<ToolDownloadArtifact, ReleaseResolutionError>
where
    F: FnMut(FetchRequest) -> Result<Vec<u8>, GitHubReleaseError>,
{
    let archive_pattern = expand_pattern(&selector.archive, version, &release.tag_name)?;
    let archive = select_unique_asset(release, &archive_pattern, arch, "archive")?;
    validate_asset_metadata(archive, arch, "archive")?;
    let sha256 = match archive.digest.as_deref().and_then(valid_github_digest) {
        Some(hash) => hash,
        None => {
            let checksum_pattern = selector.checksum.as_deref().ok_or_else(|| {
                format!(
                    "{arch} archive '{}' has no valid sha256 digest or checksum selector",
                    archive.name
                )
            })?;
            let checksum_pattern = expand_pattern(checksum_pattern, version, &release.tag_name)?;
            let checksum = select_unique_asset(release, &checksum_pattern, arch, "checksum")?;
            validate_asset_metadata(checksum, arch, "checksum")?;
            let body = if let Some(body) = checksum_cache.get(&checksum.browser_download_url) {
                body.clone()
            } else {
                let body = fetch(FetchRequest::Asset(checksum.browser_download_url.clone()))
                    .map_err(ReleaseResolutionError::Fetch)?;
                checksum_cache.insert(checksum.browser_download_url.clone(), body.clone());
                body
            };
            extract_checksum(&body, &archive.name)?
        }
    };
    Ok(ToolDownloadArtifact {
        url: archive.browser_download_url.clone(),
        sha256,
    })
}

fn expand_pattern(pattern: &str, version: &str, tag: &str) -> Result<Regex, String> {
    let expanded = pattern
        .replace("{version}", &regex::escape(version))
        .replace("{tag}", &regex::escape(tag));
    Regex::new(&expanded).map_err(|error| format!("invalid expanded asset regex: {error}"))
}

fn select_unique_asset<'a>(
    release: &'a GitHubRelease,
    pattern: &Regex,
    arch: &str,
    kind: &str,
) -> Result<&'a GitHubReleaseAsset, String> {
    let matches = release
        .assets
        .iter()
        .filter(|asset| pattern.is_match(&asset.name))
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [asset] => Ok(asset),
        [] => Err(format!("{arch} {kind} regex matched no assets")),
        _ => Err(format!(
            "{arch} {kind} regex matched {} assets",
            matches.len()
        )),
    }
}

fn validate_asset_metadata(
    asset: &GitHubReleaseAsset,
    arch: &str,
    kind: &str,
) -> Result<(), String> {
    if asset.name.is_empty() || asset.name.chars().any(char::is_control) {
        return Err(format!("{arch} {kind} asset has an invalid name"));
    }
    if asset.state != "uploaded" || asset.size == 0 {
        return Err(format!(
            "{arch} {kind} asset '{}' is not uploaded and non-empty",
            asset.name
        ));
    }
    let url = Url::parse(&asset.browser_download_url)
        .map_err(|error| format!("{arch} {kind} asset URL is invalid: {error}"))?;
    if url.scheme() != "https"
        || !url.has_host()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return Err(format!(
            "{arch} {kind} asset '{}' does not have a safe HTTPS browser_download_url",
            asset.name
        ));
    }
    Ok(())
}

fn valid_github_digest(value: &str) -> Option<String> {
    let hash = value.strip_prefix("sha256:")?;
    is_sha256(hash).then(|| hash.to_ascii_lowercase())
}

fn extract_checksum(body: &[u8], archive_name: &str) -> Result<String, String> {
    let text = std::str::from_utf8(body)
        .map_err(|error| format!("checksum asset is not UTF-8: {error}"))?;
    let mut matches = Vec::new();
    for raw_line in text.lines() {
        let line = raw_line.trim_end_matches('\r');
        if line.is_empty() {
            continue;
        }
        let entry = line
            .split_once("  ")
            .or_else(|| line.split_once(" *"))
            .ok_or_else(|| "checksum asset contains a malformed line".to_owned())?;
        if !is_sha256(entry.0) || entry.1.is_empty() {
            return Err("checksum asset contains a malformed line".to_owned());
        }
        if entry.1 == archive_name {
            matches.push(entry.0.to_ascii_lowercase());
        }
    }
    match matches.as_slice() {
        [hash] => Ok(hash.clone()),
        [] => Err(format!(
            "checksum asset has no hash for exact archive name '{archive_name}'"
        )),
        _ => Err(format!(
            "checksum asset has multiple hashes for exact archive name '{archive_name}'"
        )),
    }
}

fn is_sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

fn push_rejection(rejections: &mut Vec<String>, tag: &str, message: &str) {
    if rejections.len() < MAX_REJECTION_DIAGNOSTICS {
        rejections.push(format!("{tag} ({message})"));
    }
}

fn version_from_stable_tag(tag: &str) -> Option<&str> {
    let version = tag.strip_prefix('v')?;
    let mut parts = version.split('.');
    let valid = |part: Option<&str>| {
        part.is_some_and(|part| {
            part == "0"
                || (matches!(part.as_bytes().first(), Some(b'1'..=b'9'))
                    && part.bytes().all(|byte| byte.is_ascii_digit()))
        })
    };
    (valid(parts.next()) && valid(parts.next()) && valid(parts.next()) && parts.next().is_none())
        .then_some(version)
}

fn is_mature(release: &GitHubRelease, minimum_release_age: u32, now: OffsetDateTime) -> bool {
    let Some(published_at) = release.published_at.as_deref() else {
        return false;
    };
    let Ok(published_at) = OffsetDateTime::parse(published_at, &Rfc3339) else {
        return false;
    };
    published_at.unix_timestamp()
        <= now
            .unix_timestamp()
            .saturating_sub(i64::from(minimum_release_age) * 60)
}

fn parse_release(repo: &str, body: &[u8]) -> Result<GitHubRelease, GitHubReleaseError> {
    serde_json::from_slice(body).map_err(|error| GitHubReleaseError::Parse {
        repo: repo.to_owned(),
        message: error.to_string(),
    })
}

fn parse_release_page(repo: &str, body: &[u8]) -> Result<Vec<GitHubRelease>, GitHubReleaseError> {
    serde_json::from_slice(body).map_err(|error| GitHubReleaseError::Parse {
        repo: repo.to_owned(),
        message: error.to_string(),
    })
}

fn fetch_request(repo: &str, request: FetchRequest) -> Result<Vec<u8>, GitHubReleaseError> {
    match request {
        FetchRequest::ReleasesPage(page) => {
            let url = format!(
                "https://api.github.com/repos/{repo}/releases?per_page={RELEASES_PER_PAGE}&page={page}"
            );
            fetch_url(repo, &url, false)
        }
        FetchRequest::ReleaseByTag(tag) => {
            let mut url = Url::parse("https://api.github.com").expect("static GitHub API URL");
            let mut repo_segments = repo.split('/');
            url.path_segments_mut()
                .expect("GitHub API URL supports path segments")
                .extend([
                    "repos",
                    repo_segments.next().expect("validated owner"),
                    repo_segments.next().expect("validated repository"),
                    "releases",
                    "tags",
                    &tag,
                ]);
            fetch_url(repo, url.as_str(), false)
        }
        FetchRequest::Asset(url) => fetch_url(repo, &url, true),
    }
}

#[cfg(test)]
#[path = "github_release_tests.rs"]
mod tests;
