use std::fmt;
use std::process::Command;

use serde::Deserialize;

use crate::config::ValidatedConfig;

const BR_REPO: &str = "Dicklesworthstone/beads_rust";
const BV_REPO: &str = "Dicklesworthstone/beads_viewer";

/// Options for the update command.
pub struct UpdateOptions {
    pub pull: bool,
}

impl Default for UpdateOptions {
    fn default() -> Self {
        Self { pull: true }
    }
}

#[derive(Debug)]
pub enum UpdateError {
    MissingContainerfile(String),
    ReleaseResolveFailed(String),
    ReleaseParseFailed(String),
    BuildFailed(String),
}

impl fmt::Display for UpdateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingContainerfile(p) => write!(f, "missing Containerfile: {p}"),
            Self::ReleaseResolveFailed(msg) => write!(
                f,
                "failed to resolve latest beads releases: {msg} (check network/GitHub access)"
            ),
            Self::ReleaseParseFailed(msg) => write!(f, "failed to parse release metadata: {msg}"),
            Self::BuildFailed(msg) => write!(f, "podman build failed: {msg}"),
        }
    }
}

impl std::error::Error for UpdateError {}

/// Rebuild the sandbox container image and refresh bundled br/bv release binaries.
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

    let context_dir = containerfile
        .parent()
        .expect("containerfile must have a parent directory");

    let mut args: Vec<String> = vec![
        "build".into(),
        "-t".into(),
        image.clone(),
        "-f".into(),
        containerfile.display().to_string(),
        "--build-arg".into(),
        format!("BR_VERSION={br_version}"),
        "--build-arg".into(),
        format!("BV_VERSION={bv_version}"),
    ];

    if opts.pull {
        args.push("--pull".into());
    }

    args.push(context_dir.display().to_string());

    println!("Rebuilding {image}");
    println!("  br release: {br_version}");
    println!("  bv release: {bv_version}");

    let status = Command::new("podman")
        .args(&args)
        .status()
        .map_err(|e| UpdateError::BuildFailed(e.to_string()))?;

    if !status.success() {
        return Err(UpdateError::BuildFailed(format!("exited with {status}")));
    }

    println!("\nDone. Image rebuilt with br/bv refreshed.");
    println!("Verify inside sandbox with: br --version && bv --version");
    println!("Run 'ags update-agents' to install/update agent CLIs in volumes.");
    Ok(())
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
    use super::parse_latest_tag;

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
}
