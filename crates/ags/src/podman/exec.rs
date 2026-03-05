use std::fmt;
use std::fs;
use std::io;
use std::path::Path;
use std::process::Command;

use crate::plan::LaunchPlan;
use crate::podman::args::build_run_args;

#[derive(Debug)]
pub enum PodmanError {
    ImageBuild(String),
    EnvFileCreate(io::Error),
    SpawnFailed(io::Error),
}

impl fmt::Display for PodmanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ImageBuild(msg) => write!(f, "image build failed: {msg}"),
            Self::EnvFileCreate(e) => write!(f, "failed to create env file: {e}"),
            Self::SpawnFailed(e) => write!(f, "failed to start podman: {e}"),
        }
    }
}

impl std::error::Error for PodmanError {}

/// Check if an image exists locally.
pub fn image_exists(image: &str) -> bool {
    Command::new("podman")
        .args(["image", "exists", image])
        .status()
        .is_ok_and(|s| s.success())
}

/// Build an image from a Containerfile if it does not already exist.
pub fn ensure_image(image: &str, containerfile: &Path) -> Result<(), PodmanError> {
    if image_exists(image) {
        return Ok(());
    }

    eprintln!("Building sandbox image: {image}");

    let context_dir = containerfile.parent().unwrap_or_else(|| Path::new("."));

    let status = Command::new("podman")
        .args(["build", "-t", image, "-f"])
        .arg(containerfile)
        .arg(context_dir)
        .status()
        .map_err(|e| PodmanError::ImageBuild(e.to_string()))?;

    if !status.success() {
        return Err(PodmanError::ImageBuild(format!(
            "podman build exited with {status}"
        )));
    }

    Ok(())
}

/// Write the env file with KEY=VALUE entries, one per line.
///
/// The file is created with mode 0600. The caller is responsible for
/// cleaning it up after the container exits.
pub fn write_env_file(
    entries: &[(String, String)],
    dir: &Path,
) -> Result<std::path::PathBuf, PodmanError> {
    fs::create_dir_all(dir).map_err(PodmanError::EnvFileCreate)?;

    let path = dir.join(format!("ags-env.{}", std::process::id()));

    let content: String = entries.iter().map(|(k, v)| format!("{k}={v}\n")).collect();

    fs::write(&path, &content).map_err(PodmanError::EnvFileCreate)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
    }

    Ok(path)
}

/// Execute a container from a launch plan.
///
/// Ensures the image exists (building if necessary), writes the env file,
/// builds the podman args, runs the container, and returns the exit code.
/// Cleans up the env file on return.
pub fn execute(plan: &LaunchPlan, passthrough_args: &[String]) -> Result<u8, PodmanError> {
    // Ensure image
    ensure_image(&plan.image, &plan.containerfile)?;

    // Write env file
    let env_dir = std::env::var("XDG_RUNTIME_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir());

    let env_file = write_env_file(&plan.env.env_file_entries, &env_dir)?;

    let result = run_container(plan, &env_file, passthrough_args);

    // Cleanup env file
    let _ = fs::remove_file(&env_file);

    result
}

fn run_container(
    plan: &LaunchPlan,
    env_file: &Path,
    passthrough_args: &[String],
) -> Result<u8, PodmanError> {
    let mut args = build_run_args(plan, env_file);
    args.extend(passthrough_args.iter().cloned());

    let status = Command::new("podman")
        .args(&args)
        .status()
        .map_err(PodmanError::SpawnFailed)?;

    Ok(status.code().unwrap_or(1) as u8)
}
