use std::fmt;
use std::fs;
use std::io;
use std::net::TcpStream;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use crate::config::BrowserConfig;

/// How long to wait for the browser debug endpoint to become reachable.
const READY_TIMEOUT: Duration = Duration::from_secs(5);

/// How long to sleep between readiness polls.
const POLL_INTERVAL: Duration = Duration::from_millis(200);

#[derive(Debug)]
pub enum BrowserError {
    NotEnabled,
    EmptyCommand,
    CommandNotFound(String),
    CommandNotExecutable(String),
    ProfileDirCreate(io::Error),
    SpawnFailed(io::Error),
    ReadyTimeout { port: u16, timeout: Duration },
}

impl fmt::Display for BrowserError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotEnabled => {
                f.write_str("browser mode requested but [browser].enabled is false")
            }
            Self::EmptyCommand => {
                f.write_str("browser mode requested but [browser].command is empty")
            }
            Self::CommandNotFound(cmd) => {
                write!(f, "browser command not found in PATH: {cmd}")
            }
            Self::CommandNotExecutable(cmd) => {
                write!(f, "browser command is not executable: {cmd}")
            }
            Self::ProfileDirCreate(err) => {
                write!(f, "failed to create browser profile directory: {err}")
            }
            Self::SpawnFailed(err) => write!(f, "failed to start browser: {err}"),
            Self::ReadyTimeout { port, timeout } => {
                write!(
                    f,
                    "browser did not become ready on port {port} within {:.1}s",
                    timeout.as_secs_f64()
                )
            }
        }
    }
}

/// A running browser sidecar with its debug port.
///
/// When dropped, the browser process is killed.
pub struct BrowserSidecar {
    child: Option<Child>,
    pub port: u16,
}

impl fmt::Debug for BrowserSidecar {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BrowserSidecar")
            .field("port", &self.port)
            .field("has_child", &self.child.is_some())
            .finish()
    }
}

impl BrowserSidecar {
    /// Build the socat proxy command for use inside the container.
    ///
    /// The container uses socat to forward localhost:{port} to the host's
    /// browser via slirp4netns (10.0.2.2).
    pub fn socat_command(&self) -> String {
        format!(
            "socat TCP-LISTEN:{port},fork,reuseaddr,bind=127.0.0.1 \
             TCP:10.0.2.2:{port} >/tmp/ags-socat.log 2>&1 &",
            port = self.port
        )
    }

    /// Kill the browser process if still running.
    pub fn stop(&mut self) {
        if let Some(ref mut child) = self.child {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.child = None;
    }
}

impl Drop for BrowserSidecar {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Start the browser sidecar if not already running.
///
/// Returns `Ok(None)` if browser mode is not requested.
/// Returns `Ok(Some(sidecar))` with a running browser.
/// Returns `Err` if browser mode is requested but something fails.
pub fn start_if_needed(
    browser_mode: bool,
    config: &BrowserConfig,
) -> Result<Option<BrowserSidecar>, BrowserError> {
    if !browser_mode {
        return Ok(None);
    }

    if !config.enabled {
        return Err(BrowserError::NotEnabled);
    }

    if config.command.is_empty() {
        return Err(BrowserError::EmptyCommand);
    }

    // Already running? Check if debug port is reachable.
    if is_debug_port_open(config.debug_port) {
        return Ok(Some(BrowserSidecar {
            child: None,
            port: config.debug_port,
        }));
    }

    validate_command(&config.command)?;

    fs::create_dir_all(&config.profile_dir).map_err(BrowserError::ProfileDirCreate)?;

    let child = spawn_browser(config)?;

    wait_for_ready(config.debug_port)?;

    Ok(Some(BrowserSidecar {
        child: Some(child),
        port: config.debug_port,
    }))
}

/// Check if the debug port is already accepting connections.
fn is_debug_port_open(port: u16) -> bool {
    TcpStream::connect_timeout(
        &format!("127.0.0.1:{port}").parse().unwrap(),
        Duration::from_secs(1),
    )
    .is_ok()
}

/// Validate the browser command exists and is executable.
fn validate_command(command: &str) -> Result<(), BrowserError> {
    if command.contains('/') {
        // Absolute or relative path — check executability
        let path = Path::new(command);
        if !path.exists() || !is_executable(path) {
            return Err(BrowserError::CommandNotExecutable(command.to_owned()));
        }
    } else {
        // Bare command name — check PATH
        if which(command).is_none() {
            return Err(BrowserError::CommandNotFound(command.to_owned()));
        }
    }
    Ok(())
}

/// Spawn the browser as a detached background process.
fn spawn_browser(config: &BrowserConfig) -> Result<Child, BrowserError> {
    let mut cmd = Command::new(&config.command);

    for arg in &config.command_args {
        cmd.arg(arg);
    }

    cmd.arg(format!("--remote-debugging-port={}", config.debug_port));
    cmd.arg(format!("--user-data-dir={}", config.profile_dir.display()));
    cmd.arg("--no-first-run");
    cmd.arg("--no-default-browser-check");
    cmd.arg("about:blank");

    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::null());
    cmd.stderr(Stdio::null());

    cmd.spawn().map_err(BrowserError::SpawnFailed)
}

/// Poll the debug port until the browser is ready or timeout.
fn wait_for_ready(port: u16) -> Result<(), BrowserError> {
    let start = Instant::now();
    while start.elapsed() < READY_TIMEOUT {
        if is_debug_port_open(port) {
            return Ok(());
        }
        std::thread::sleep(POLL_INTERVAL);
    }
    Err(BrowserError::ReadyTimeout {
        port,
        timeout: READY_TIMEOUT,
    })
}

/// Check if a path is executable (unix).
#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .map(|m| m.permissions().mode() & 0o111 != 0)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.exists()
}

/// Simple PATH lookup for a command name.
fn which(name: &str) -> Option<std::path::PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|dir| dir.join(name))
            .find(|candidate| candidate.is_file() && is_executable(candidate))
    })
}
