use std::fmt;
use std::fs;
use std::io;
use std::net::TcpStream;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use crate::BROWSER_HOST_LOOPBACK;
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
    profile: tempfile::TempDir,
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
    /// The container uses socat to forward localhost:9222 to the host's
    /// browser via the slirp4netns host-loopback address. Podman 6 pasta
    /// compatibility maps the same address explicitly.
    pub fn socat_command(&self) -> String {
        format!(
            "socat TCP-LISTEN:9222,fork,reuseaddr,bind=127.0.0.1 \
             TCP:{host}:{port} >/tmp/ags-socat.log 2>&1 &",
            host = BROWSER_HOST_LOOPBACK,
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

/// Start an isolated browser sidecar for this session.
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

    validate_command(&config.command)?;
    let sessions = config.profile_dir.join("sessions");
    fs::create_dir_all(&sessions).map_err(BrowserError::ProfileDirCreate)?;
    let profile = tempfile::Builder::new()
        .prefix(&format!("{}-", std::process::id()))
        .tempdir_in(sessions)
        .map_err(BrowserError::ProfileDirCreate)?;
    let child = spawn_browser(config, profile.path())?;
    // Establish ownership before readiness checks so failure also kills the child.
    let mut sidecar = BrowserSidecar {
        child: Some(child),
        profile,
        port: 0,
    };
    sidecar.port = wait_for_ready(sidecar.profile.path())?;
    Ok(Some(sidecar))
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
        if !path.exists() || !crate::util::is_executable(path) {
            return Err(BrowserError::CommandNotExecutable(command.to_owned()));
        }
    } else {
        // Bare command name — check PATH
        if crate::util::which(command).is_none() {
            return Err(BrowserError::CommandNotFound(command.to_owned()));
        }
    }
    Ok(())
}

/// Spawn the browser as a detached background process.
fn spawn_browser(config: &BrowserConfig, profile: &Path) -> Result<Child, BrowserError> {
    Command::new(&config.command)
        .args(&config.command_args)
        .arg("--remote-debugging-port=0")
        .arg(format!("--user-data-dir={}", profile.display()))
        .arg(format!("--class={}", config.window_class))
        .args([
            "--no-first-run",
            "--no-default-browser-check",
            "about:blank",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(BrowserError::SpawnFailed)
}

/// Poll the debug port until the browser is ready or timeout.
fn wait_for_ready(profile: &Path) -> Result<u16, BrowserError> {
    use std::ops::ControlFlow;
    crate::util::poll_until(READY_TIMEOUT, POLL_INTERVAL, || {
        let port = fs::read_to_string(profile.join("DevToolsActivePort"))
            .ok()
            .and_then(|text| text.lines().next()?.parse::<u16>().ok());
        if let Some(port) = port.filter(|port| *port != 0 && is_debug_port_open(*port)) {
            ControlFlow::Break(port)
        } else {
            ControlFlow::Continue(())
        }
    })
    .ok_or(BrowserError::ReadyTimeout {
        port: 0,
        timeout: READY_TIMEOUT,
    })
}
