use std::fmt;
use std::fs;
use std::io;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

const READY_TIMEOUT: Duration = Duration::from_secs(5);
const POLL_INTERVAL: Duration = Duration::from_millis(100);
const CONTAINER_RUNTIME_DIR: &str = "/run/ags-host-ui";
const CONTAINER_SOCKET_PATH: &str = "/run/ags-host-ui/host-ui.sock";

#[derive(Debug)]
pub enum HostUiError {
    RuntimeDirCreate(io::Error),
    LogCreate(io::Error),
    SpawnFailed(io::Error),
    ServiceExited {
        status: std::process::ExitStatus,
        log_path: PathBuf,
    },
    ReadyTimeout {
        path: PathBuf,
        timeout: Duration,
        log_path: PathBuf,
    },
}

impl fmt::Display for HostUiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RuntimeDirCreate(err) => {
                write!(f, "host UI: failed to create runtime dir: {err}")
            }
            Self::LogCreate(err) => write!(f, "host UI: failed to create diagnostics log: {err}"),
            Self::SpawnFailed(err) => write!(f, "host UI: failed to start service: {err}"),
            Self::ServiceExited { status, log_path } => write!(
                f,
                "host UI: service exited with {status}; diagnostics: {}",
                log_path.display()
            ),
            Self::ReadyTimeout {
                path,
                timeout,
                log_path,
            } => write!(
                f,
                "host UI: socket {} was not ready within {:.1}s; diagnostics: {}",
                path.display(),
                timeout.as_secs_f64(),
                log_path.display()
            ),
        }
    }
}

impl std::error::Error for HostUiError {}

pub struct HostUiGuard {
    child: Child,
    pub runtime_dir: PathBuf,
    pub socket_path: PathBuf,
    pub session_id: String,
    /// Host-only diagnostics, retained after the session's socket directory is removed.
    pub log_path: PathBuf,
}

impl HostUiGuard {
    pub fn container_runtime_dir() -> &'static str {
        CONTAINER_RUNTIME_DIR
    }

    pub fn container_socket_path() -> &'static str {
        CONTAINER_SOCKET_PATH
    }

    pub fn stop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = fs::remove_dir_all(&self.runtime_dir);
    }
}

impl Drop for HostUiGuard {
    fn drop(&mut self) {
        self.stop();
    }
}

pub fn start(
    runtime_dir: &Path,
    session_id: String,
    config: &crate::config::HostUiConfig,
) -> Result<HostUiGuard, HostUiError> {
    crate::util::ensure_private_dir(runtime_dir).map_err(HostUiError::RuntimeDirCreate)?;
    let socket_path = runtime_dir.join("host-ui.sock");
    let (log, log_path) = create_log(runtime_dir).map_err(HostUiError::LogCreate)?;

    let mut cmd = Command::new(&config.binary);
    cmd.arg("--socket")
        .arg(&socket_path)
        .arg("--idle-timeout-ms")
        .arg(config.idle_timeout_ms.to_string())
        .arg("--renderer")
        .arg(&config.renderer)
        .arg("--log-level")
        .arg(&config.log_level)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        // Native renderer libraries (Mesa, GTK, etc.) bypass service log levels.
        // A file also captures inherited child stderr without a pipe to drain.
        .stderr(Stdio::from(log));

    if let Some(renderer_bin) = &config.renderer_bin {
        cmd.arg("--renderer-bin").arg(renderer_bin);
    }

    let child = cmd.spawn().map_err(HostUiError::SpawnFailed)?;
    let mut guard = HostUiGuard {
        child,
        runtime_dir: runtime_dir.to_owned(),
        socket_path,
        session_id,
        log_path,
    };
    // On readiness failure, the guard stops the child but retains its diagnostics.
    wait_for_ready(&mut guard)?;
    Ok(guard)
}

fn create_log(runtime_dir: &Path) -> io::Result<(fs::File, PathBuf)> {
    // Keep logs outside the directory mounted into the sandbox.
    let log_dir = runtime_dir
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("host-ui-logs");
    crate::util::ensure_private_dir(&log_dir)?;
    tempfile::Builder::new()
        .prefix("host-ui-")
        .suffix(".log")
        .tempfile_in(log_dir)?
        .keep()
        .map_err(|err| err.error)
}

fn wait_for_ready(guard: &mut HostUiGuard) -> Result<(), HostUiError> {
    use std::ops::ControlFlow;
    crate::util::poll_until(READY_TIMEOUT, POLL_INTERVAL, || {
        if let Ok(Some(status)) = guard.child.try_wait() {
            return ControlFlow::Break(Err(HostUiError::ServiceExited {
                status,
                log_path: guard.log_path.clone(),
            }));
        }
        if guard.socket_path.exists() && UnixStream::connect(&guard.socket_path).is_ok() {
            ControlFlow::Break(Ok(()))
        } else {
            ControlFlow::Continue(())
        }
    })
    .unwrap_or_else(|| {
        Err(HostUiError::ReadyTimeout {
            path: guard.socket_path.clone(),
            timeout: READY_TIMEOUT,
            log_path: guard.log_path.clone(),
        })
    })
}
