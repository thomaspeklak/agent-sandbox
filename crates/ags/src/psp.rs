use std::fmt;
use std::fs;
use std::io;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::time::Duration;

/// Container-side directory where the PSP socket is mounted.
const CONTAINER_PSP_DIR: &str = "/run/psp";

/// Container-side path where the PSP socket is mounted.
const CONTAINER_PSP_SOCK: &str = "/run/psp/psp.sock";

/// Timeout for PSP to become ready after starting.
const READINESS_TIMEOUT: Duration = Duration::from_secs(10);

/// Poll interval for readiness check.
const POLL_INTERVAL: Duration = Duration::from_millis(100);

/// How long to wait for PSP to shut down gracefully (SIGTERM) before SIGKILL.
const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug)]
pub enum PspError {
    BinaryNotFound(String),
    SocketDirCreate(io::Error),
    Spawn(io::Error),
    ReadinessTimeout,
    ChildExited(ExitStatus),
}

impl fmt::Display for PspError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BinaryNotFound(bin) => write!(
                f,
                "psp binary not found: '{bin}' (install podman-socket-proxy or set [psp].binary in config)"
            ),
            Self::SocketDirCreate(e) => write!(f, "psp: failed to create socket directory: {e}"),
            Self::Spawn(e) => write!(f, "psp: failed to start: {e}"),
            Self::ReadinessTimeout => write!(
                f,
                "psp: timed out waiting for readiness ({}s)",
                READINESS_TIMEOUT.as_secs()
            ),
            Self::ChildExited(status) => {
                write!(f, "psp: process exited immediately ({status})")
            }
        }
    }
}

impl std::error::Error for PspError {}

/// Guard that manages the PSP sidecar lifetime.
///
/// PSP is spawned as a child process and killed when dropped.
/// The per-run socket file is cleaned up on drop.
pub struct PspGuard {
    pub socket_path: PathBuf,
    socket_dir: PathBuf,
    child: Child,
}

impl PspGuard {
    /// Container-side socket path for DOCKER_HOST.
    pub fn container_socket_path() -> &'static str {
        CONTAINER_PSP_SOCK
    }

    /// Container-side directory where the socket dir is mounted.
    pub fn container_socket_dir() -> &'static str {
        CONTAINER_PSP_DIR
    }
}

impl Drop for PspGuard {
    fn drop(&mut self) {
        // Send SIGTERM so PSP can clean up tracked containers gracefully.
        #[cfg(unix)]
        unsafe {
            libc::kill(self.child.id() as libc::pid_t, libc::SIGTERM);
        }

        // Wait for graceful exit, then SIGKILL as fallback.
        let exited = crate::util::poll_until(SHUTDOWN_TIMEOUT, POLL_INTERVAL, || {
            use std::ops::ControlFlow;
            match self.child.try_wait() {
                Ok(Some(_)) => ControlFlow::Break(()),
                _ => ControlFlow::Continue(()),
            }
        });
        if exited.is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }

        let _ = fs::remove_dir_all(&self.socket_dir);
    }
}

impl fmt::Debug for PspGuard {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PspGuard")
            .field("socket_path", &self.socket_path)
            .finish()
    }
}

/// Resolve the psp binary path: use config override if non-empty, else PATH lookup.
fn resolve_binary(config_binary: &str) -> Result<PathBuf, PspError> {
    if !config_binary.is_empty() {
        return Ok(PathBuf::from(config_binary));
    }
    crate::util::which("psp").ok_or_else(|| PspError::BinaryNotFound("psp".to_owned()))
}

/// Start PSP as a sidecar process with a per-PID socket.
///
/// Blocks until PSP is ready (socket accepts connections) or times out.
/// When `keep_on_failure` is true, PSP will retain containers on shutdown
/// for debugging (sets `PSP_KEEP_ON_FAILURE=true`).
pub fn start(config_binary: &str, keep_on_failure: bool) -> Result<PspGuard, PspError> {
    let binary = resolve_binary(config_binary)?;

    let runtime_base = crate::util::runtime_dir();

    let socket_dir = runtime_base.join(format!("ags-psp-{}", std::process::id()));
    fs::create_dir_all(&socket_dir).map_err(PspError::SocketDirCreate)?;

    let socket_path = socket_dir.join("psp.sock");

    let mut cmd = Command::new(&binary);
    cmd.arg("run")
        .env("PSP_LISTEN_SOCKET", &socket_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit());

    if keep_on_failure {
        cmd.env("PSP_KEEP_ON_FAILURE", "true");
    }

    let child = cmd.spawn().map_err(PspError::Spawn)?;

    let mut guard = PspGuard {
        socket_path,
        socket_dir,
        child,
    };

    // Wait for PSP to be ready
    if let Err(e) = wait_ready(&guard.socket_path, &mut guard.child) {
        drop(guard);
        return Err(e);
    }

    Ok(guard)
}

/// Poll the Unix socket until it accepts a connection, or the child exits.
fn wait_ready(socket_path: &Path, child: &mut Child) -> Result<(), PspError> {
    use std::ops::ControlFlow;

    enum Ready {
        Connected,
        ChildDied(ExitStatus),
    }

    let result = crate::util::poll_until(READINESS_TIMEOUT, POLL_INTERVAL, || {
        if let Some(status) = child.try_wait().ok().flatten() {
            return ControlFlow::Break(Ready::ChildDied(status));
        }
        if UnixStream::connect(socket_path).is_ok() {
            return ControlFlow::Break(Ready::Connected);
        }
        ControlFlow::Continue(())
    });

    match result {
        Some(Ready::Connected) => Ok(()),
        Some(Ready::ChildDied(status)) => Err(PspError::ChildExited(status)),
        None => Err(PspError::ReadinessTimeout),
    }
}
