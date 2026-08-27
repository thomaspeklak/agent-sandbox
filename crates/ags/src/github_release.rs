use std::fmt;
use std::process::Command;

use serde::Deserialize;
use time::{OffsetDateTime, format_description::well_known::Rfc3339};

const RELEASES_PER_PAGE: usize = 100;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ReleaseSelection {
    pub(crate) tag_name: String,
    pub(crate) latest_tag_name: String,
}

#[derive(Debug)]
pub(crate) enum GitHubReleaseError {
    Fetch {
        repo: String,
        message: String,
    },
    Parse {
        repo: String,
        message: String,
    },
    NoEligibleRelease {
        repo: String,
        minimum_release_age: u32,
        required_assets: Vec<String>,
    },
}

impl fmt::Display for GitHubReleaseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Fetch { repo, message } => {
                write!(f, "{repo}: could not fetch GitHub releases: {message}")
            }
            Self::Parse { repo, message } => {
                write!(f, "{repo}: could not parse GitHub releases: {message}")
            }
            Self::NoEligibleRelease {
                repo,
                minimum_release_age,
                required_assets,
            } => {
                write!(
                    f,
                    "{repo}: no stable vMAJOR.MINOR.PATCH release published at least {minimum_release_age} minutes ago"
                )?;
                if !required_assets.is_empty() {
                    write!(
                        f,
                        " with uploaded, non-empty assets {}",
                        required_assets.join(" and ")
                    )?;
                }
                Ok(())
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
}

/// Resolve the newest stable, mature release whose tag and required assets meet AGS policy.
pub(crate) fn resolve_latest_mature_release(
    repo: &str,
    minimum_release_age: u32,
    required_assets: &[String],
) -> Result<ReleaseSelection, GitHubReleaseError> {
    resolve_latest_mature_release_with(
        repo,
        minimum_release_age,
        required_assets,
        OffsetDateTime::now_utc(),
        |page| fetch_release_page(repo, page),
    )
}

fn resolve_latest_mature_release_with<F>(
    repo: &str,
    minimum_release_age: u32,
    required_assets: &[String],
    now: OffsetDateTime,
    mut fetch_page: F,
) -> Result<ReleaseSelection, GitHubReleaseError>
where
    F: FnMut(usize) -> Result<Vec<GitHubRelease>, GitHubReleaseError>,
{
    let mut page = 1;
    let mut releases = Vec::new();

    loop {
        let mut page_releases = fetch_page(page)?;
        let page_len = page_releases.len();
        releases.append(&mut page_releases);

        if let Some(selection) =
            select_latest_mature_release(&releases, minimum_release_age, required_assets, now)
        {
            return Ok(selection);
        }
        if page_len < RELEASES_PER_PAGE {
            break;
        }
        page += 1;
    }

    Err(GitHubReleaseError::NoEligibleRelease {
        repo: repo.to_owned(),
        minimum_release_age,
        required_assets: required_assets.to_vec(),
    })
}

fn fetch_release_page(repo: &str, page: usize) -> Result<Vec<GitHubRelease>, GitHubReleaseError> {
    let url = format!(
        "https://api.github.com/repos/{repo}/releases?per_page={RELEASES_PER_PAGE}&page={page}"
    );
    let output = Command::new("curl")
        .args([
            "--proto",
            "=https",
            "--tlsv1.2",
            "-fsSL",
            "--connect-timeout",
            "10",
            "--max-time",
            "30",
            "--retry",
            "2",
            "--retry-delay",
            "1",
            "-H",
            "Accept: application/vnd.github+json",
            "-H",
            "User-Agent: ags",
            &url,
        ])
        .output()
        .map_err(|error| GitHubReleaseError::Fetch {
            repo: repo.to_owned(),
            message: error.to_string(),
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(GitHubReleaseError::Fetch {
            repo: repo.to_owned(),
            message: format!(
                "curl exited with {}{}",
                output.status,
                if stderr.is_empty() {
                    String::new()
                } else {
                    format!(" ({stderr})")
                }
            ),
        });
    }

    parse_release_page(repo, &output.stdout)
}

fn select_latest_mature_release(
    releases: &[GitHubRelease],
    minimum_release_age: u32,
    required_assets: &[String],
    now: OffsetDateTime,
) -> Option<ReleaseSelection> {
    let stable_releases = releases
        .iter()
        .filter(|release| is_stable_semver_release(release))
        .collect::<Vec<_>>();
    let latest_tag_name = stable_releases.first()?.tag_name.trim().to_owned();

    stable_releases.into_iter().find_map(|release| {
        if !is_mature(release, minimum_release_age, now)
            || !has_required_assets(release, required_assets)
        {
            return None;
        }
        Some(ReleaseSelection {
            tag_name: release.tag_name.trim().to_owned(),
            latest_tag_name: latest_tag_name.clone(),
        })
    })
}

fn is_stable_semver_release(release: &GitHubRelease) -> bool {
    !release.draft && !release.prerelease && is_version_tag(release.tag_name.trim())
}

fn is_version_tag(tag: &str) -> bool {
    let Some(version) = tag.strip_prefix('v') else {
        return false;
    };
    let mut parts = version.split('.');
    let valid_component = |part: Option<&str>| {
        part.is_some_and(|value| {
            value == "0"
                || (matches!(value.as_bytes().first(), Some(b'1'..=b'9'))
                    && value.bytes().all(|byte| byte.is_ascii_digit()))
        })
    };

    valid_component(parts.next())
        && valid_component(parts.next())
        && valid_component(parts.next())
        && parts.next().is_none()
}

fn is_mature(release: &GitHubRelease, minimum_release_age: u32, now: OffsetDateTime) -> bool {
    let Some(published_at) = release.published_at.as_deref() else {
        return false;
    };
    let Ok(published_at) = OffsetDateTime::parse(published_at, &Rfc3339) else {
        return false;
    };
    let age_seconds = i64::from(minimum_release_age) * 60;
    published_at.unix_timestamp() <= now.unix_timestamp().saturating_sub(age_seconds)
}

fn has_required_assets(release: &GitHubRelease, required_assets: &[String]) -> bool {
    let version = release.tag_name.trim().trim_start_matches('v');
    required_assets.iter().all(|required| {
        let required = required.replace("{version}", version);
        release
            .assets
            .iter()
            .any(|asset| asset.name == required && asset.state == "uploaded" && asset.size > 0)
    })
}

fn parse_release_page(repo: &str, body: &[u8]) -> Result<Vec<GitHubRelease>, GitHubReleaseError> {
    serde_json::from_slice(body).map_err(|error| GitHubReleaseError::Parse {
        repo: repo.to_owned(),
        message: error.to_string(),
    })
}

#[cfg(test)]
#[path = "github_release_tests.rs"]
mod tests;
