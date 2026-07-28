use std::process::Command;

use serde::Deserialize;

use super::UpdateError;

const BR_REPO: &str = "Dicklesworthstone/beads_rust";
const DCG_REPO: &str = "Dicklesworthstone/destructive_command_guard";
const RELEASES_PER_PAGE: usize = 100;

#[derive(Clone, Copy, Debug)]
pub(super) enum BuildArchitecture {
    X86_64,
    Aarch64,
}

impl BuildArchitecture {
    pub(super) fn detect() -> Result<Self, UpdateError> {
        let output = Command::new("podman")
            .args(["info", "--format", "{{.Host.Arch}}"])
            .output()
            .map_err(|e| {
                UpdateError::ReleaseResolveFailed(format!(
                    "could not inspect Podman build architecture: {e}"
                ))
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            return Err(UpdateError::ReleaseResolveFailed(format!(
                "could not inspect Podman build architecture: podman info exited with {}{}",
                output.status,
                if stderr.is_empty() {
                    String::new()
                } else {
                    format!(" ({stderr})")
                }
            )));
        }

        let arch = String::from_utf8(output.stdout).map_err(|e| {
            UpdateError::ReleaseResolveFailed(format!(
                "Podman returned a non-UTF8 build architecture: {e}"
            ))
        })?;
        Self::from_podman_arch(arch.trim())
    }

    fn from_podman_arch(arch: &str) -> Result<Self, UpdateError> {
        match arch {
            "amd64" | "x86_64" => Ok(Self::X86_64),
            "aarch64" | "arm64" => Ok(Self::Aarch64),
            _ => Err(UpdateError::ReleaseResolveFailed(format!(
                "unsupported architecture for bundled GitHub dependencies: {arch}"
            ))),
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::X86_64 => "x86_64",
            Self::Aarch64 => "aarch64",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) enum BundledDependency {
    Br,
    Dcg,
}

impl BundledDependency {
    fn name(self) -> &'static str {
        match self {
            Self::Br => "br",
            Self::Dcg => "dcg",
        }
    }

    fn repo(self) -> &'static str {
        match self {
            Self::Br => BR_REPO,
            Self::Dcg => DCG_REPO,
        }
    }

    fn required_assets(self, tag: &str, arch: BuildArchitecture) -> [String; 2] {
        let archive = match self {
            Self::Br => {
                let version = tag.strip_prefix('v').unwrap_or(tag);
                let arch = match arch {
                    BuildArchitecture::X86_64 => "amd64",
                    BuildArchitecture::Aarch64 => "arm64",
                };
                format!("br-{version}-linux_{arch}.tar.gz")
            }
            Self::Dcg => {
                let target = match arch {
                    BuildArchitecture::X86_64 => "x86_64-unknown-linux-musl",
                    BuildArchitecture::Aarch64 => "aarch64-unknown-linux-gnu",
                };
                format!("dcg-{target}.tar.xz")
            }
        };
        let checksum = format!("{archive}.sha256");

        [archive, checksum]
    }
}

#[derive(Debug, PartialEq)]
pub(super) struct ReleaseSelection {
    pub(super) tag_name: String,
    pub(super) latest_tag_name: String,
}

#[derive(Debug, Deserialize)]
struct GitHubRelease {
    tag_name: String,
    draft: bool,
    prerelease: bool,
    assets: Vec<GitHubReleaseAsset>,
}

#[derive(Debug, Deserialize)]
struct GitHubReleaseAsset {
    name: String,
    state: String,
    size: u64,
}

pub(super) fn resolve_latest_compatible_tag(
    dependency: BundledDependency,
    arch: BuildArchitecture,
) -> Result<ReleaseSelection, UpdateError> {
    let repo = dependency.repo();
    resolve_latest_compatible_tag_with(dependency, arch, |page| fetch_release_page(repo, page))
}

fn resolve_latest_compatible_tag_with<F>(
    dependency: BundledDependency,
    arch: BuildArchitecture,
    mut fetch_page: F,
) -> Result<ReleaseSelection, UpdateError>
where
    F: FnMut(usize) -> Result<Vec<GitHubRelease>, UpdateError>,
{
    let mut page = 1;
    let mut releases = Vec::new();

    loop {
        let mut page_releases = fetch_page(page)?;
        let page_len = page_releases.len();
        releases.append(&mut page_releases);

        if let Some(selection) = select_latest_compatible_tag(&releases, dependency, arch) {
            return Ok(selection);
        }
        if page_len < RELEASES_PER_PAGE {
            break;
        }
        page += 1;
    }

    Err(no_compatible_release_error(&releases, dependency, arch))
}

fn fetch_release_page(repo: &str, page: usize) -> Result<Vec<GitHubRelease>, UpdateError> {
    let url = format!(
        "https://api.github.com/repos/{repo}/releases?per_page={RELEASES_PER_PAGE}&page={page}"
    );
    let output = Command::new("curl")
        .args([
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
        .map_err(|e| {
            UpdateError::ReleaseResolveFailed(format!("{repo}: could not run curl: {e}"))
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(UpdateError::ReleaseResolveFailed(format!(
            "{repo}: curl exited with {}{}",
            output.status,
            if stderr.is_empty() {
                String::new()
            } else {
                format!(" ({stderr})")
            }
        )));
    }

    serde_json::from_slice(&output.stdout)
        .map_err(|e| UpdateError::ReleaseParseFailed(format!("{repo}: {e}")))
}

#[cfg(test)]
pub(super) fn parse_latest_compatible_tag(
    body: &str,
    dependency: BundledDependency,
    arch: BuildArchitecture,
) -> Result<ReleaseSelection, UpdateError> {
    let releases: Vec<GitHubRelease> =
        serde_json::from_str(body).map_err(|e| UpdateError::ReleaseParseFailed(e.to_string()))?;
    select_latest_compatible_tag(&releases, dependency, arch)
        .ok_or_else(|| no_compatible_release_error(&releases, dependency, arch))
}

#[cfg(test)]
pub(super) fn resolve_latest_compatible_tag_from_pages(
    dependency: BundledDependency,
    arch: BuildArchitecture,
    pages: &[String],
) -> Result<ReleaseSelection, UpdateError> {
    let mut pages = pages.iter();
    resolve_latest_compatible_tag_with(dependency, arch, |_| {
        let body = pages.next().map_or("[]", String::as_str);
        serde_json::from_str(body).map_err(|e| UpdateError::ReleaseParseFailed(e.to_string()))
    })
}

fn select_latest_compatible_tag(
    releases: &[GitHubRelease],
    dependency: BundledDependency,
    arch: BuildArchitecture,
) -> Option<ReleaseSelection> {
    let stable_releases: Vec<&GitHubRelease> = releases
        .iter()
        .filter(|release| {
            let tag = release.tag_name.trim();
            !release.draft && !release.prerelease && !tag.is_empty() && tag != "null"
        })
        .collect();
    let latest = stable_releases.first()?;
    let latest_tag_name = latest.tag_name.trim().to_owned();

    for release in &stable_releases {
        let tag = release.tag_name.trim();
        let required_assets = dependency.required_assets(tag, arch);
        let has_required_assets = required_assets.iter().all(|required| {
            release
                .assets
                .iter()
                .any(|asset| asset.name == *required && asset.state == "uploaded" && asset.size > 0)
        });

        if has_required_assets {
            return Some(ReleaseSelection {
                tag_name: tag.to_owned(),
                latest_tag_name,
            });
        }
    }

    None
}

fn no_compatible_release_error(
    releases: &[GitHubRelease],
    dependency: BundledDependency,
    arch: BuildArchitecture,
) -> UpdateError {
    let stable_releases: Vec<&GitHubRelease> = releases
        .iter()
        .filter(|release| {
            let tag = release.tag_name.trim();
            !release.draft && !release.prerelease && !tag.is_empty() && tag != "null"
        })
        .collect();
    let Some(latest) = stable_releases.first() else {
        return UpdateError::ReleaseResolveFailed(format!(
            "{}: GitHub returned no published stable releases",
            dependency.repo()
        ));
    };
    let latest_tag_name = latest.tag_name.trim();
    let required_assets = dependency.required_assets(latest_tag_name, arch);

    UpdateError::ReleaseResolveFailed(format!(
        "{}: none of the latest {} stable releases contains uploaded, non-empty assets {}",
        dependency.repo(),
        stable_releases.len(),
        required_assets.join(" and ")
    ))
}

pub(super) fn warn_if_fallback(
    dependency: BundledDependency,
    arch: BuildArchitecture,
    selection: &ReleaseSelection,
) {
    if selection.tag_name != selection.latest_tag_name {
        eprintln!(
            "warning: latest {} release {} lacks a complete {} archive/checksum pair; using latest compatible release {}",
            dependency.name(),
            selection.latest_tag_name,
            arch.label(),
            selection.tag_name
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{
        BuildArchitecture, BundledDependency, parse_latest_compatible_tag,
        resolve_latest_compatible_tag_from_pages,
    };

    fn release(tag: &str, prerelease: bool, assets: &[&str]) -> serde_json::Value {
        serde_json::json!({
            "tag_name": tag,
            "draft": false,
            "prerelease": prerelease,
            "assets": assets
                .iter()
                .map(|name| serde_json::json!({
                    "name": name,
                    "state": "uploaded",
                    "size": 1
                }))
                .collect::<Vec<_>>()
        })
    }

    #[test]
    fn recognizes_podman_architecture_names() {
        assert!(matches!(
            BuildArchitecture::from_podman_arch("amd64").unwrap(),
            BuildArchitecture::X86_64
        ));
        assert!(matches!(
            BuildArchitecture::from_podman_arch("arm64").unwrap(),
            BuildArchitecture::Aarch64
        ));
        assert!(BuildArchitecture::from_podman_arch("riscv64").is_err());
    }

    #[test]
    fn skips_newer_releases_without_linux_assets() {
        let input = serde_json::json!([
            release("v0.7.0", false, &["dcg-x86_64-pc-windows-msvc.zip"]),
            release("v0.6.12", false, &["dcg-x86_64-pc-windows-msvc.zip"]),
            release(
                "v0.6.7",
                false,
                &[
                    "dcg-x86_64-unknown-linux-musl.tar.xz",
                    "dcg-x86_64-unknown-linux-musl.tar.xz.sha256"
                ]
            )
        ])
        .to_string();

        let release =
            parse_latest_compatible_tag(&input, BundledDependency::Dcg, BuildArchitecture::X86_64)
                .expect("compatible release should resolve");

        assert_eq!(release.tag_name, "v0.6.7");
        assert_eq!(release.latest_tag_name, "v0.7.0");
    }

    #[test]
    fn finds_compatible_release_after_first_page() {
        let first_page = (0..100)
            .rev()
            .map(|patch| {
                release(
                    &format!("v0.7.{patch}"),
                    false,
                    &["dcg-x86_64-pc-windows-msvc.zip"],
                )
            })
            .collect::<Vec<_>>();
        let pages = [
            serde_json::Value::Array(first_page).to_string(),
            serde_json::json!([release(
                "v0.6.7",
                false,
                &[
                    "dcg-x86_64-unknown-linux-musl.tar.xz",
                    "dcg-x86_64-unknown-linux-musl.tar.xz.sha256"
                ]
            )])
            .to_string(),
        ];

        let release = resolve_latest_compatible_tag_from_pages(
            BundledDependency::Dcg,
            BuildArchitecture::X86_64,
            &pages,
        )
        .expect("compatible release should resolve");

        assert_eq!(release.tag_name, "v0.6.7");
        assert_eq!(release.latest_tag_name, "v0.7.99");
    }

    #[test]
    fn uses_dependency_specific_asset_names() {
        let input = serde_json::json!([release(
            "v0.2.19",
            false,
            &[
                "br-0.2.19-linux_arm64.tar.gz",
                "br-0.2.19-linux_arm64.tar.gz.sha256"
            ]
        )])
        .to_string();

        let release =
            parse_latest_compatible_tag(&input, BundledDependency::Br, BuildArchitecture::Aarch64)
                .expect("compatible release should resolve");

        assert_eq!(release.tag_name, "v0.2.19");
        assert_eq!(release.latest_tag_name, "v0.2.19");
    }

    #[test]
    fn fallback_applies_to_br() {
        let input = serde_json::json!([
            release("v0.2.20", false, &["br-0.2.20-linux_amd64.tar.gz"]),
            release(
                "v0.2.19",
                false,
                &[
                    "br-0.2.19-linux_amd64.tar.gz",
                    "br-0.2.19-linux_amd64.tar.gz.sha256"
                ]
            )
        ])
        .to_string();

        let release =
            parse_latest_compatible_tag(&input, BundledDependency::Br, BuildArchitecture::X86_64)
                .expect("compatible release should resolve");

        assert_eq!(release.tag_name, "v0.2.19");
        assert_eq!(release.latest_tag_name, "v0.2.20");
    }

    #[test]
    fn requires_archive_and_checksum() {
        let input = serde_json::json!([release(
            "v0.6.7",
            false,
            &["dcg-x86_64-unknown-linux-musl.tar.xz"]
        )])
        .to_string();

        let err =
            parse_latest_compatible_tag(&input, BundledDependency::Dcg, BuildArchitecture::X86_64)
                .expect_err("release without checksum should fail");

        assert!(err.to_string().contains(".sha256"));
    }

    #[test]
    fn ignores_prereleases() {
        let assets = [
            "dcg-x86_64-unknown-linux-musl.tar.xz",
            "dcg-x86_64-unknown-linux-musl.tar.xz.sha256",
        ];
        let input = serde_json::json!([
            release("v0.8.0-rc.1", true, &assets),
            release("v0.7.1", false, &assets)
        ])
        .to_string();

        let release =
            parse_latest_compatible_tag(&input, BundledDependency::Dcg, BuildArchitecture::X86_64)
                .expect("stable compatible release should resolve");

        assert_eq!(release.tag_name, "v0.7.1");
        assert_eq!(release.latest_tag_name, "v0.7.1");
    }
}
