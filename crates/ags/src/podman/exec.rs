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
    MissingNetworkBackend(String),
    SpawnFailed(io::Error),
}

impl fmt::Display for PodmanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ImageBuild(msg) => write!(f, "image build failed: {msg}"),
            Self::EnvFileCreate(e) => write!(f, "failed to create env file: {e}"),
            Self::MissingNetworkBackend(msg) => f.write_str(msg),
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

/// Check whether a binary is available on PATH inside a built image.
pub fn image_has_binary(image: &str, binary: &str) -> Result<bool, PodmanError> {
    let status = Command::new("podman")
        .args(["run", "--rm", "--entrypoint", "bash", image, "-lc"])
        .arg(format!(
            "command -v {} >/dev/null 2>&1",
            crate::util::shell_quote(binary)
        ))
        .status()
        .map_err(PodmanError::SpawnFailed)?;
    Ok(status.success())
}

/// Build an image from a Containerfile if it does not already exist.
pub fn ensure_image(image: &str, containerfile: &Path) -> Result<(), PodmanError> {
    if image_exists(image) {
        return Ok(());
    }

    eprintln!("Building sandbox image: {image}");

    let context_dir = containerfile.parent().unwrap_or_else(|| Path::new("."));

    let status = Command::new("podman")
        .args(["build", "--pull", "-t", image, "-f"])
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
    crate::util::ensure_private_dir(dir).map_err(PodmanError::EnvFileCreate)?;

    let path = dir.join(format!("ags-env.{}", std::process::id()));

    for (key, value) in entries {
        validate_env_file_entry(key, value).map_err(PodmanError::EnvFileCreate)?;
    }
    let content: String = entries.iter().map(|(k, v)| format!("{k}={v}\n")).collect();

    fs::write(&path, &content).map_err(PodmanError::EnvFileCreate)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&path, fs::Permissions::from_mode(0o600));
    }

    Ok(path)
}

fn validate_env_file_entry(key: &str, value: &str) -> io::Result<()> {
    let valid_key = key
        .bytes()
        .enumerate()
        .all(|(idx, b)| b == b'_' || b.is_ascii_alphabetic() || (idx > 0 && b.is_ascii_digit()));
    if key.is_empty() || !valid_key || key.as_bytes()[0].is_ascii_digit() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("invalid environment variable name: {key:?}"),
        ));
    }
    if value.contains(['\n', '\r']) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("environment variable {key} contains a newline"),
        ));
    }
    Ok(())
}

/// Execute a container from a launch plan.
///
/// Ensures the image exists (building if necessary), writes the env file,
/// builds the podman args, runs the container, and returns the exit code.
/// Cleans up the env file on return.
pub fn execute(plan: &LaunchPlan, passthrough_args: &[String]) -> Result<u8, PodmanError> {
    ensure_network_backend_available(&plan.network_mode)?;

    // Ensure image
    ensure_image(&plan.image, &plan.containerfile)?;

    // Write env file
    let env_dir = crate::util::runtime_dir().map_err(PodmanError::EnvFileCreate)?;

    let env_file = write_env_file(&plan.env.env_file_entries, &env_dir)?;

    let result = run_container(plan, &env_file, passthrough_args);

    // Cleanup env file
    let _ = fs::remove_file(&env_file);

    result
}

fn ensure_network_backend_available(network_mode: &str) -> Result<(), PodmanError> {
    if required_network_binary(network_mode) == Some("pasta") {
        if podman_pasta_executable()?.is_some() {
            return Ok(());
        }
        return Err(PodmanError::MissingNetworkBackend(
            "Podman did not report a pasta executable; install the package providing pasta where Podman runs (commonly passt)".to_owned(),
        ));
    }
    Ok(())
}

fn podman_pasta_executable() -> Result<Option<String>, PodmanError> {
    let output = Command::new("podman")
        .args(["info", "--format", "{{.Host.Pasta.Executable}}"])
        .output()
        .map_err(PodmanError::SpawnFailed)?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        let detail = if stderr.is_empty() {
            format!("podman info exited with {}", output.status)
        } else {
            stderr
        };
        return Err(PodmanError::MissingNetworkBackend(format!(
            "failed to inspect Podman's pasta backend: {detail}"
        )));
    }

    Ok(parse_pasta_executable(&output.stdout))
}

fn parse_pasta_executable(stdout: &[u8]) -> Option<String> {
    let executable = String::from_utf8_lossy(stdout).trim().to_owned();
    if executable.is_empty() || executable == "<no value>" {
        None
    } else {
        Some(executable)
    }
}

fn required_network_binary(network_mode: &str) -> Option<&'static str> {
    if network_mode == "pasta" || network_mode.starts_with("pasta:") {
        Some("pasta")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_pasta_executable, required_network_binary};

    #[test]
    fn pasta_network_requires_podman_pasta_backend() {
        assert_eq!(required_network_binary("pasta"), Some("pasta"));
        assert_eq!(required_network_binary("pasta:--map-gw"), Some("pasta"));
        assert_eq!(
            required_network_binary("pasta:--map-host-loopback=169.254.1.2"),
            Some("pasta")
        );
        assert_eq!(required_network_binary("slirp4netns"), None);
    }

    #[test]
    fn parses_podman_pasta_executable() {
        assert_eq!(
            parse_pasta_executable(b"/usr/bin/pasta\n"),
            Some("/usr/bin/pasta".to_owned())
        );
        assert_eq!(parse_pasta_executable(b"\n"), None);
        assert_eq!(parse_pasta_executable(b"<no value>\n"), None);
    }
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
