use std::fmt;
use std::path::Path;
use std::process::{Command, ExitStatus};

use serde::Deserialize;

use crate::config::ValidatedConfig;

const BR_REPO: &str = "Dicklesworthstone/beads_rust";
const BV_REPO: &str = "Dicklesworthstone/beads_viewer";
const DCG_REPO: &str = "Dicklesworthstone/destructive_command_guard";

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
                "failed to resolve latest bundled tool releases: {msg} (check network/GitHub access)"
            ),
            Self::ReleaseParseFailed(msg) => write!(f, "failed to parse release metadata: {msg}"),
            Self::ImageInspectFailed(msg) => write!(f, "failed to inspect existing image: {msg}"),
            Self::BuildFailed(msg) => write!(f, "podman build failed: {msg}"),
            Self::CleanupFailed(msg) => write!(f, "failed to remove previous image: {msg}"),
        }
    }
}

impl std::error::Error for UpdateError {}

/// Rebuild the sandbox container image and refresh bundled br/bv/dcg release binaries.
pub fn run(config: &ValidatedConfig, opts: &UpdateOptions) -> Result<(), UpdateError> {
    let image = &config.sandbox.image;
    let containerfile = &config.sandbox.containerfile;

    if !containerfile.exists() {
        return Err(UpdateError::MissingContainerfile(
            containerfile.display().to_string(),
        ));
    }

    let br_version = resolve_latest_tag(BR_REPO)?;
    let bv_version = resolve_latest_tag(BV_REPO)?;
    let dcg_version = resolve_latest_tag(DCG_REPO)?;

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
        &bv_version,
        &dcg_version,
        opts.pull,
    );

    println!("Rebuilding {image}");
    println!("  br release: {br_version}");
    println!("  bv release: {bv_version}");
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
    } else if let Some(id) = remove_previous_image(image, previous_image_id.as_deref())? {
        println!("Removed previous image {}.", short_image_id(&id));
    }

    println!("\nDone. Image rebuilt with br/bv/dcg refreshed.");
    println!("Verify inside sandbox with: br --version && bv --version && dcg --version");
    println!("Run 'ags update-agents' to install/update agent CLIs in volumes.");
    Ok(())
}

fn remove_previous_image(
    image: &str,
    previous_image_id: Option<&str>,
) -> Result<Option<String>, UpdateError> {
    let Some(previous_image_id) = previous_image_id else {
        return Ok(None);
    };

    let current_id = current_image_id(image)?.ok_or_else(|| {
        UpdateError::ImageInspectFailed(format!("{image}: missing after successful build"))
    })?;

    if current_id == previous_image_id {
        return Ok(None);
    }

    let status = Command::new("podman")
        .args(build_podman_image_rm_args(previous_image_id))
        .status()
        .map_err(|e| UpdateError::CleanupFailed(e.to_string()))?;

    if !status.success() {
        return Err(UpdateError::CleanupFailed(format!("exited with {status}")));
    }

    Ok(Some(previous_image_id.to_owned()))
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

fn build_podman_build_args(
    image: &str,
    containerfile: &Path,
    context_dir: &Path,
    br_version: &str,
    bv_version: &str,
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

    for (name, version) in [
        ("BR_VERSION", br_version),
        ("BV_VERSION", bv_version),
        ("DCG_VERSION", dcg_version),
    ] {
        args.push("--build-arg".to_owned());
        args.push(format!("{name}={version}"));
    }

    if pull {
        args.push("--pull".to_owned());
    }

    args.push(context_dir.display().to_string());
    args
}

#[derive(Debug, Deserialize)]
struct LatestRelease {
    tag_name: String,
}

fn resolve_latest_tag(repo: &str) -> Result<String, UpdateError> {
    let url = format!("https://api.github.com/repos/{repo}/releases/latest");
    let output = Command::new("curl")
        .args([
            "-fsSL",
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

    let body = String::from_utf8(output.stdout)
        .map_err(|e| UpdateError::ReleaseParseFailed(format!("{repo}: non-UTF8 response: {e}")))?;

    parse_latest_tag(&body).map_err(|e| match e {
        UpdateError::ReleaseParseFailed(msg) => {
            UpdateError::ReleaseParseFailed(format!("{repo}: {msg}"))
        }
        other => other,
    })
}

fn parse_latest_tag(body: &str) -> Result<String, UpdateError> {
    let release: LatestRelease =
        serde_json::from_str(body).map_err(|e| UpdateError::ReleaseParseFailed(e.to_string()))?;
    let tag = release.tag_name.trim();

    if tag.is_empty() || tag == "null" {
        return Err(UpdateError::ReleaseParseFailed(
            "missing tag_name in GitHub response".to_owned(),
        ));
    }

    Ok(tag.to_owned())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{
        build_podman_build_args, build_podman_image_exists_args, build_podman_image_inspect_args,
        build_podman_image_rm_args, parse_latest_tag, short_image_id,
    };

    #[test]
    fn parse_latest_tag_extracts_tag_name() {
        let input = r#"{"tag_name":"v0.1.24"}"#;
        let tag = parse_latest_tag(input).expect("tag should parse");
        assert_eq!(tag, "v0.1.24");
    }

    #[test]
    fn parse_latest_tag_rejects_empty_tag() {
        let input = r#"{"tag_name":""}"#;
        let err = parse_latest_tag(input).expect_err("empty tag should fail");
        assert!(err.to_string().contains("missing tag_name"));
    }

    #[test]
    fn build_args_include_dcg_version_and_pull_flag() {
        let args = build_podman_build_args(
            "localhost/agent-sandbox:latest",
            Path::new("/tmp/Containerfile"),
            Path::new("/tmp"),
            "v1.0.0",
            "v2.0.0",
            "v3.0.0",
            true,
        );

        assert!(args.contains(&"--pull".to_owned()));
        assert!(args.contains(&"BR_VERSION=v1.0.0".to_owned()));
        assert!(args.contains(&"BV_VERSION=v2.0.0".to_owned()));
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
    }

    #[test]
    fn short_image_id_strips_prefix_and_truncates() {
        assert_eq!(short_image_id("sha256:1234567890abcdef"), "1234567890ab");
        assert_eq!(short_image_id("abc"), "abc");
    }
}
