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
    SpawnFailed(io::Error),
    ReadyTimeout { path: PathBuf, timeout: Duration },
}

impl fmt::Display for HostUiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RuntimeDirCreate(err) => {
                write!(f, "host UI: failed to create runtime dir: {err}")
            }
            Self::SpawnFailed(err) => write!(f, "host UI: failed to start service: {err}"),
            Self::ReadyTimeout { path, timeout } => write!(
                f,
                "host UI: socket {} was not ready within {:.1}s",
                path.display(),
                timeout.as_secs_f64()
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
    fs::create_dir_all(runtime_dir).map_err(HostUiError::RuntimeDirCreate)?;
    let socket_path = runtime_dir.join("host-ui.sock");

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
        .stderr(Stdio::null());

    if let Some(renderer_bin) = &config.renderer_bin {
        cmd.arg("--renderer-bin").arg(renderer_bin);
    }

    let child = cmd.spawn().map_err(HostUiError::SpawnFailed)?;
    wait_for_ready(&socket_path)?;

    Ok(HostUiGuard {
        child,
        runtime_dir: runtime_dir.to_owned(),
        socket_path,
        session_id,
    })
}

fn wait_for_ready(socket_path: &Path) -> Result<(), HostUiError> {
    use std::ops::ControlFlow;
    crate::util::poll_until(READY_TIMEOUT, POLL_INTERVAL, || {
        if socket_path.exists() && UnixStream::connect(socket_path).is_ok() {
            ControlFlow::Break(())
        } else {
            ControlFlow::Continue(())
        }
    })
    .ok_or_else(|| HostUiError::ReadyTimeout {
        path: socket_path.to_owned(),
        timeout: READY_TIMEOUT,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    #[test]
    fn starts_service_and_waits_for_socket_ready() {
        let _guard = env_lock().lock().unwrap();
        let temp = tempfile::tempdir().unwrap();
        let runtime_dir = temp.path().join("host-ui-runtime");
        let stub = temp.path().join("glimpse-host-ui-stub.py");
        fs::write(
            &stub,
            r#"#!/usr/bin/env python3
import os, socket, sys, time
sock_path = None
for index, value in enumerate(sys.argv):
    if value == '--socket' and index + 1 < len(sys.argv):
        sock_path = sys.argv[index + 1]
        break
if not sock_path:
    raise SystemExit('missing --socket')
os.makedirs(os.path.dirname(sock_path), exist_ok=True)
try:
    os.unlink(sock_path)
except FileNotFoundError:
    pass
server = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
server.bind(sock_path)
server.listen(4)
server.settimeout(0.1)
end = time.time() + 5
while time.time() < end:
    try:
        conn, _ = server.accept()
        conn.close()
    except TimeoutError:
        pass
server.close()
"#,
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&stub, fs::Permissions::from_mode(0o755)).unwrap();
        }

        let config = crate::config::HostUiConfig {
            enabled: true,
            binary: stub.to_string_lossy().into_owned(),
            renderer: "stub".to_owned(),
            renderer_bin: None,
            idle_timeout_ms: 1_000,
            log_level: "info".to_owned(),
        };
        let guard = start(&runtime_dir, "ags-test-session".to_owned(), &config).unwrap();
        assert!(guard.socket_path.exists());
        assert_eq!(guard.session_id, "ags-test-session");
        drop(guard);
    }
}
