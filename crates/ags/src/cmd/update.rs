use std::fmt;
use std::path::Path;
use std::process::{Command, ExitStatus};

use crate::config::ValidatedConfig;

mod github_release;

use github_release::{
    BuildArchitecture, BundledDependency, resolve_latest_compatible_tag, warn_if_fallback,
};

/// Options for the update command.
pub struct UpdateOptions {
    pub pull: bool,
    pub keep_existing: bool,
}

impl Default for UpdateOptions {
    fn default() -> Self {
        Self {
            pull: true,
            keep_existing: false,
        }
    }
}

#[derive(Debug)]
pub enum UpdateError {
    MissingContainerfile(String),
    ReleaseResolveFailed(String),
    ReleaseParseFailed(String),
    ImageInspectFailed(String),
    BuildFailed(String),
    CleanupFailed(String),
}

impl fmt::Display for UpdateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingContainerfile(p) => write!(f, "missing Containerfile: {p}"),
            Self::ReleaseResolveFailed(msg) => write!(
                f,
                "failed to resolve compatible bundled tool releases: {msg}"
            ),
            Self::ReleaseParseFailed(msg) => write!(f, "failed to parse release metadata: {msg}"),
            Self::ImageInspectFailed(msg) => write!(f, "failed to inspect existing image: {msg}"),
            Self::BuildFailed(msg) => write!(f, "podman build failed: {msg}"),
            Self::CleanupFailed(msg) => write!(f, "failed to remove previous image: {msg}"),
        }
    }
}

impl std::error::Error for UpdateError {}

#[derive(Debug, PartialEq)]
enum PreviousImageCleanup {
    NotNeeded,
    Removed(String),
    Retained {
        image_id: String,
        container_ids: Vec<String>,
    },
}

/// Rebuild the sandbox container image and refresh bundled br/dcg release binaries.
pub fn run(config: &ValidatedConfig, opts: &UpdateOptions) -> Result<(), UpdateError> {
    let image = &config.sandbox.image;
    let containerfile = &config.sandbox.containerfile;

    if !containerfile.exists() {
        return Err(UpdateError::MissingContainerfile(
            containerfile.display().to_string(),
        ));
    }

    let arch = BuildArchitecture::detect()?;
    let br_release = resolve_latest_compatible_tag(BundledDependency::Br, arch)?;
    let dcg_release = resolve_latest_compatible_tag(BundledDependency::Dcg, arch)?;
    warn_if_fallback(BundledDependency::Br, arch, &br_release);
    warn_if_fallback(BundledDependency::Dcg, arch, &dcg_release);
    let br_version = br_release.tag_name;
    let dcg_version = dcg_release.tag_name;

    let context_dir = containerfile
        .parent()
        .expect("containerfile must have a parent directory");

    let previous_image_id = if opts.keep_existing {
        None
    } else {
        current_image_id(image)?
    };

    let args = build_podman_build_args(
        image,
        containerfile,
        context_dir,
        &br_version,
        &dcg_version,
        opts.pull,
    );

    println!("Rebuilding {image}");
    println!("  br release: {br_version}");
    println!("  dcg release: {dcg_version}");

    let status = Command::new("podman")
        .args(&args)
        .status()
        .map_err(|e| UpdateError::BuildFailed(e.to_string()))?;

    if !status.success() {
        return Err(UpdateError::BuildFailed(format!("exited with {status}")));
    }

    if opts.keep_existing {
        println!("Keeping previous image because --keep-existing was provided.");
    } else {
        match remove_previous_image(image, previous_image_id.as_deref())? {
            PreviousImageCleanup::NotNeeded => {}
            PreviousImageCleanup::Removed(id) => {
                println!("Removed previous image {}.", short_image_id(&id));
            }
            PreviousImageCleanup::Retained {
                image_id,
                container_ids,
            } => {
                eprintln!(
                    "warning: previous image {} is still used by container(s) {}; keeping it\n  remove those containers when no longer needed, then run: podman image rm {}",
                    short_image_id(&image_id),
                    container_ids.join(", "),
                    image_id
                );
            }
        }
    }

    println!("\nDone. Image rebuilt with br/dcg refreshed.");
    println!("Verify inside sandbox with: br --version && dcg --version");
    println!("Run 'ags update-agents' to install/update agent CLIs in volumes.");
    Ok(())
}

fn remove_previous_image(
    image: &str,
    previous_image_id: Option<&str>,
) -> Result<PreviousImageCleanup, UpdateError> {
    let Some(previous_image_id) = previous_image_id else {
        return Ok(PreviousImageCleanup::NotNeeded);
    };

    let current_id = current_image_id(image)?.ok_or_else(|| {
        UpdateError::ImageInspectFailed(format!("{image}: missing after successful build"))
    })?;

    if current_id == previous_image_id {
        return Ok(PreviousImageCleanup::NotNeeded);
    }

    let container_ids = containers_using_image(previous_image_id)?;
    if let Some(retained) = retain_referenced_image(previous_image_id, container_ids) {
        return Ok(retained);
    }

    let output = Command::new("podman")
        .args(build_podman_image_rm_args(previous_image_id))
        .output()
        .map_err(|e| UpdateError::CleanupFailed(e.to_string()))?;

    if !output.status.success() {
        if is_image_reference_conflict(output.status.code()) {
            let container_ids = containers_using_image(previous_image_id)?;
            if let Some(retained) = retain_referenced_image(previous_image_id, container_ids) {
                return Ok(retained);
            }
        }

        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(UpdateError::CleanupFailed(format!(
            "image rm exited with {}{}",
            output.status,
            if stderr.is_empty() {
                String::new()
            } else {
                format!(" ({stderr})")
            }
        )));
    }

    Ok(PreviousImageCleanup::Removed(previous_image_id.to_owned()))
}

fn containers_using_image(image_id: &str) -> Result<Vec<String>, UpdateError> {
    let output = Command::new("podman")
        .args(build_podman_container_image_refs_args())
        .output()
        .map_err(|e| UpdateError::CleanupFailed(e.to_string()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(UpdateError::CleanupFailed(format!(
            "container lookup exited with {}{}",
            output.status,
            if stderr.is_empty() {
                String::new()
            } else {
                format!(" ({stderr})")
            }
        )));
    }

    let stdout = String::from_utf8(output.stdout).map_err(|e| {
        UpdateError::CleanupFailed(format!("container lookup returned non-UTF8 output: {e}"))
    })?;
    parse_container_image_refs(&stdout, image_id)
}

fn parse_container_image_refs(stdout: &str, image_id: &str) -> Result<Vec<String>, UpdateError> {
    let mut container_ids = Vec::new();
    for line in stdout.lines().filter(|line| !line.trim().is_empty()) {
        let (container_id, container_image_id) = line.split_once('\t').ok_or_else(|| {
            UpdateError::CleanupFailed(format!("unexpected container lookup output: {line}"))
        })?;
        let container_id = container_id.trim();
        let container_image_id = container_image_id.trim();
        if container_id.is_empty() {
            return Err(UpdateError::CleanupFailed(
                "container lookup returned an empty container ID".to_owned(),
            ));
        }
        if normalized_image_id(container_image_id) == normalized_image_id(image_id) {
            container_ids.push(container_id.to_owned());
        }
    }
    Ok(container_ids)
}

fn normalized_image_id(image_id: &str) -> &str {
    image_id
        .trim()
        .strip_prefix("sha256:")
        .unwrap_or(image_id.trim())
}

fn is_image_reference_conflict(exit_code: Option<i32>) -> bool {
    exit_code == Some(2)
}

fn retain_referenced_image(
    image_id: &str,
    container_ids: Vec<String>,
) -> Option<PreviousImageCleanup> {
    if container_ids.is_empty() {
        None
    } else {
        Some(PreviousImageCleanup::Retained {
            image_id: image_id.to_owned(),
            container_ids,
        })
    }
}

fn current_image_id(image: &str) -> Result<Option<String>, UpdateError> {
    let status = Command::new("podman")
        .args(build_podman_image_exists_args(image))
        .status()
        .map_err(|e| UpdateError::ImageInspectFailed(e.to_string()))?;

    match status.code() {
        Some(0) => {}
        Some(1) => return Ok(None),
        _ => {
            return Err(UpdateError::ImageInspectFailed(exit_message(
                "image exists",
                status,
            )));
        }
    }

    let output = Command::new("podman")
        .args(build_podman_image_inspect_args(image))
        .output()
        .map_err(|e| UpdateError::ImageInspectFailed(e.to_string()))?;

    if !output.status.success() {
        return Err(UpdateError::ImageInspectFailed(exit_message(
            "image inspect",
            output.status,
        )));
    }

    let id = String::from_utf8(output.stdout)
        .map_err(|e| UpdateError::ImageInspectFailed(format!("non-UTF8 image id: {e}")))?
        .trim()
        .to_owned();

    if id.is_empty() {
        return Err(UpdateError::ImageInspectFailed(
            "image inspect returned an empty id".to_owned(),
        ));
    }

    Ok(Some(id))
}

fn exit_message(command: &str, status: ExitStatus) -> String {
    format!("{command} exited with {status}")
}

fn short_image_id(id: &str) -> String {
    id.strip_prefix("sha256:")
        .unwrap_or(id)
        .chars()
        .take(12)
        .collect()
}

fn build_podman_image_exists_args(image: &str) -> Vec<String> {
    vec!["image".to_owned(), "exists".to_owned(), image.to_owned()]
}

fn build_podman_image_inspect_args(image: &str) -> Vec<String> {
    vec![
        "image".to_owned(),
        "inspect".to_owned(),
        "--format".to_owned(),
        "{{.Id}}".to_owned(),
        image.to_owned(),
    ]
}

fn build_podman_image_rm_args(image_id: &str) -> Vec<String> {
    vec!["image".to_owned(), "rm".to_owned(), image_id.to_owned()]
}

fn build_podman_container_image_refs_args() -> Vec<String> {
    vec![
        "ps".to_owned(),
        "--all".to_owned(),
        "--external".to_owned(),
        "--no-trunc".to_owned(),
        "--format".to_owned(),
        "{{.ID}}\t{{.ImageID}}".to_owned(),
    ]
}

fn build_podman_build_args(
    image: &str,
    containerfile: &Path,
    context_dir: &Path,
    br_version: &str,
    dcg_version: &str,
    pull: bool,
) -> Vec<String> {
    let mut args = vec![
        "build".to_owned(),
        "-t".to_owned(),
        image.to_owned(),
        "-f".to_owned(),
        containerfile.display().to_string(),
    ];

    for (name, version) in [("BR_VERSION", br_version), ("DCG_VERSION", dcg_version)] {
        args.push("--build-arg".to_owned());
        args.push(format!("{name}={version}"));
    }

    if pull {
        args.push("--pull".to_owned());
    }

    args.push(context_dir.display().to_string());
    args
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{
        PreviousImageCleanup, build_podman_build_args, build_podman_container_image_refs_args,
        build_podman_image_exists_args, build_podman_image_inspect_args,
        build_podman_image_rm_args, is_image_reference_conflict, parse_container_image_refs,
        retain_referenced_image, short_image_id,
    };

    #[test]
    fn build_args_include_dcg_version_and_pull_flag() {
        let args = build_podman_build_args(
            "localhost/agent-sandbox:latest",
            Path::new("/tmp/Containerfile"),
            Path::new("/tmp"),
            "v1.0.0",
            "v3.0.0",
            true,
        );

        assert!(args.contains(&"--pull".to_owned()));
        assert!(args.contains(&"BR_VERSION=v1.0.0".to_owned()));
        assert!(!args.iter().any(|arg| arg.starts_with("BV_VERSION=")));
        assert!(args.contains(&"DCG_VERSION=v3.0.0".to_owned()));
        assert_eq!(args.last().unwrap(), "/tmp");
    }

    #[test]
    fn image_cleanup_args_target_previous_image_id() {
        assert_eq!(
            build_podman_image_exists_args("localhost/agent-sandbox:latest"),
            vec!["image", "exists", "localhost/agent-sandbox:latest"]
        );
        assert_eq!(
            build_podman_image_inspect_args("localhost/agent-sandbox:latest"),
            vec![
                "image",
                "inspect",
                "--format",
                "{{.Id}}",
                "localhost/agent-sandbox:latest"
            ]
        );
        assert_eq!(
            build_podman_image_rm_args("sha256:old"),
            vec!["image", "rm", "sha256:old"]
        );
        assert_eq!(
            build_podman_container_image_refs_args(),
            vec![
                "ps",
                "--all",
                "--external",
                "--no-trunc",
                "--format",
                "{{.ID}}\t{{.ImageID}}"
            ]
        );
    }

    #[test]
    fn cleanup_retains_previous_image_used_by_containers() {
        let cleanup = retain_referenced_image(
            "sha256:old",
            vec!["3d1419eaf9fd".to_owned(), "another".to_owned()],
        );

        assert_eq!(
            cleanup,
            Some(PreviousImageCleanup::Retained {
                image_id: "sha256:old".to_owned(),
                container_ids: vec!["3d1419eaf9fd".to_owned(), "another".to_owned()]
            })
        );
        assert_eq!(retain_referenced_image("sha256:old", Vec::new()), None);
    }

    #[test]
    fn cleanup_matches_only_exact_image_references() {
        let output = "direct\tsha256:abc123\nchild\tsha256:def456\nexternal\tabc123\nimageless\t\n";

        assert_eq!(
            parse_container_image_refs(output, "sha256:abc123").unwrap(),
            vec!["direct", "external"]
        );
    }

    #[test]
    fn cleanup_only_downgrades_podman_reference_conflicts() {
        assert!(is_image_reference_conflict(Some(2)));
        assert!(!is_image_reference_conflict(Some(125)));
        assert!(!is_image_reference_conflict(None));
    }

    #[test]
    fn short_image_id_strips_prefix_and_truncates() {
        assert_eq!(short_image_id("sha256:1234567890abcdef"), "1234567890ab");
        assert_eq!(short_image_id("abc"), "abc");
    }
}
