use std::ffi::CString;
use std::fmt;
use std::fs;
use std::io;
use std::os::fd::OwnedFd;
use std::path::Path;
use std::process::Command;

use super::fd_exec::spawn_with_payload_fds;

use crate::plan::LaunchPlan;
use crate::podman::args::build_run_args;
use crate::podman::network::{
    adapt_network_mode_for_installed_podman, fallback_network_mode_after_run_failure,
    should_probe_network_mode_after_run_failure,
};

#[derive(Debug)]
pub enum PodmanError {
    ImageBuild(String),
    EnvFileCreate(io::Error),
    SpawnFailed(io::Error),
    PayloadCountMismatch { expected: usize, received: usize },
    RemotePodmanUnsupported,
    LocalPodmanProbe(io::Error),
    InvalidPodmanArgument,
    PayloadPrepare(String),
}

impl fmt::Display for PodmanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ImageBuild(msg) => write!(f, "image build failed: {msg}"),
            Self::EnvFileCreate(e) => write!(f, "failed to create env file: {e}"),
            Self::SpawnFailed(e) => write!(f, "failed to start podman: {e}"),
            Self::PayloadCountMismatch { expected, received } => write!(
                f,
                "payload descriptor count mismatch (expected {expected}, received {received})"
            ),
            Self::RemotePodmanUnsupported => f.write_str(
                "--op-secret-set requires local Podman; remote Podman connections cannot preserve anonymous descriptors",
            ),
            Self::LocalPodmanProbe(error) => write!(
                f,
                "could not verify a local Podman connection required by --op-secret-set: {error}"
            ),
            Self::InvalidPodmanArgument => f.write_str("invalid Podman argument"),
            Self::PayloadPrepare(error) => {
                write!(f, "1Password payload preparation failed: {error}")
            }
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
    if plan.payload_fd_count > 0 {
        return Err(PodmanError::PayloadCountMismatch {
            expected: plan.payload_fd_count,
            received: 0,
        });
    }
    execute_inner(plan, passthrough_args)
}

/// Execute a plan while handing sealed payload descriptors directly to Podman.
/// The descriptors are consumed and closed in the parent as soon as Podman is
/// forked; they never enter a plan, environment, argument, or temporary file.
/// Resolve source metadata only at the final Podman handoff. A network retry
/// obtains fresh one-shot descriptors rather than retaining the first set.
pub(crate) fn execute_with_payload_sources(
    plan: &LaunchPlan,
    passthrough_args: &[String],
    sources: &[crate::onepassword::SourceRef],
) -> Result<u8, PodmanError> {
    if plan.bootstrap_path.is_none() || plan.payload_fd_count != sources.len() || sources.is_empty()
    {
        return Err(PodmanError::PayloadCountMismatch {
            expected: plan.payload_fd_count,
            received: sources.len(),
        });
    }
    ensure_local_podman()?;
    let mut plan = plan.clone();
    adapt_network_mode_for_installed_podman(&mut plan);
    ensure_image(&plan.image, &plan.containerfile)?;
    let env_dir = crate::util::runtime_dir().map_err(PodmanError::EnvFileCreate)?;
    let env_file = write_env_file(&plan.env.env_file_entries, &env_dir)?;
    let result = run_payload_sources(&plan, &env_file, passthrough_args, sources);
    let _ = fs::remove_file(&env_file);
    result
}

/// Reject remote Podman before `op` can materialize any plaintext descriptor.
/// `CONTAINER_HOST` and `CONTAINER_CONNECTION` select Podman's remote client;
/// a default configured connection does too. A local API socket is not enough:
/// descriptors cannot cross the remote client/server protocol safely.
fn ensure_local_podman() -> Result<(), PodmanError> {
    if ["CONTAINER_HOST", "CONTAINER_CONNECTION"]
        .iter()
        .any(|name| std::env::var_os(name).is_some_and(|value| !value.is_empty()))
    {
        return Err(PodmanError::RemotePodmanUnsupported);
    }
    let output = Command::new("podman")
        .args(["system", "connection", "list", "--format", "{{.Default}}"])
        .output()
        .map_err(PodmanError::LocalPodmanProbe)?;
    if !output.status.success() {
        return Err(PodmanError::LocalPodmanProbe(io::Error::other(
            "podman connection-list probe failed",
        )));
    }
    if String::from_utf8_lossy(&output.stdout)
        .lines()
        .any(|line| line.trim().eq_ignore_ascii_case("true"))
    {
        return Err(PodmanError::RemotePodmanUnsupported);
    }
    Ok(())
}

fn execute_inner(plan: &LaunchPlan, passthrough_args: &[String]) -> Result<u8, PodmanError> {
    let mut plan = plan.clone();
    adapt_network_mode_for_installed_podman(&mut plan);
    ensure_image(&plan.image, &plan.containerfile)?;
    let env_dir = crate::util::runtime_dir().map_err(PodmanError::EnvFileCreate)?;
    let env_file = write_env_file(&plan.env.env_file_entries, &env_dir)?;
    let result = run_container(&plan, &env_file, passthrough_args);
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

    let exit_code = status.code().unwrap_or(1) as u8;
    if should_probe_network_mode_after_run_failure(&plan.network_mode, exit_code)
        && let Some(network_mode) = fallback_network_mode_after_run_failure(
            &plan.network_mode,
            exit_code,
            &probe_network_mode_failure(plan)?,
        )
    {
        eprintln!(
            "[ags] Podman rejected --network={}; retrying with --network={network_mode}",
            plan.network_mode
        );

        let mut retry_plan = plan.clone();
        retry_plan.network_mode = network_mode;
        let mut retry_args = build_run_args(&retry_plan, env_file);
        retry_args.extend(passthrough_args.iter().cloned());

        let retry_status = Command::new("podman")
            .args(&retry_args)
            .status()
            .map_err(PodmanError::SpawnFailed)?;
        return Ok(retry_status.code().unwrap_or(1) as u8);
    }

    Ok(exit_code)
}

fn run_payload_sources(
    plan: &LaunchPlan,
    env_file: &Path,
    passthrough_args: &[String],
    sources: &[crate::onepassword::SourceRef],
) -> Result<u8, PodmanError> {
    let mut args = build_run_args(plan, env_file);
    args.extend(passthrough_args.iter().cloned());
    let status = run_podman_with_payload_fds(&args, prepare_payloads(sources)?)?;
    let exit_code = status.code().unwrap_or(1) as u8;
    if should_probe_network_mode_after_run_failure(&plan.network_mode, exit_code)
        && let Some(network_mode) = fallback_network_mode_after_run_failure(
            &plan.network_mode,
            exit_code,
            &probe_network_mode_failure(plan)?,
        )
    {
        eprintln!(
            "[ags] Podman rejected --network={}; retrying with --network={network_mode}",
            plan.network_mode
        );
        let mut retry_plan = plan.clone();
        retry_plan.network_mode = network_mode;
        let mut retry_args = build_run_args(&retry_plan, env_file);
        retry_args.extend(passthrough_args.iter().cloned());
        let status = run_podman_with_payload_fds(&retry_args, prepare_payloads(sources)?)?;
        return Ok(status.code().unwrap_or(1) as u8);
    }
    Ok(exit_code)
}

fn prepare_payloads(
    sources: &[crate::onepassword::SourceRef],
) -> Result<Vec<OwnedFd>, PodmanError> {
    crate::onepassword::prepare(sources)
        // OnePassword errors contain source metadata only, never payload bytes.
        .map_err(|error| PodmanError::PayloadPrepare(error.to_string()))
        .map(|items| {
            items
                .into_iter()
                .map(crate::onepassword::PreparedItem::into_fd)
                .collect()
        })
}

fn run_podman_with_payload_fds(
    args: &[String],
    payloads: Vec<OwnedFd>,
) -> Result<std::process::ExitStatus, PodmanError> {
    let command = CString::new("podman").expect("static command has no NUL");
    let mut child_args = Vec::with_capacity(args.len() + 1);
    child_args.push(command.clone());
    for arg in args {
        child_args
            .push(CString::new(arg.as_bytes()).map_err(|_| PodmanError::InvalidPodmanArgument)?);
    }
    spawn_with_payload_fds(command.as_c_str(), &child_args, payloads)
        .map_err(PodmanError::SpawnFailed)?
        .wait()
        .map_err(PodmanError::SpawnFailed)
}

fn probe_network_mode_failure(plan: &LaunchPlan) -> Result<String, PodmanError> {
    let output = Command::new("podman")
        .args(["run", "--rm", "--pull=never", "--network"])
        .arg(&plan.network_mode)
        .args(["--entrypoint", "bash"])
        .arg(&plan.image)
        .args(["-lc", "true"])
        .output()
        .map_err(PodmanError::SpawnFailed)?;

    let mut message = String::from_utf8_lossy(&output.stderr).into_owned();
    message.push_str(&String::from_utf8_lossy(&output.stdout));
    Ok(message)
}
