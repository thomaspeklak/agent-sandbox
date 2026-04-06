use std::fmt;
use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use base64::Engine;
use serde::{Deserialize, Serialize};

pub const SOCKET_NAME: &str = "relay.sock";
pub const UPSTREAM_SOCKET_NAME: &str = "upstream.sock";
pub const SHIM_NAME: &str = "webview-relay-shim";
pub const HELPER_NAME: &str = "ags-webview-url";
const CONTAINER_RUNTIME_DIR: &str = "/run/ags-webview-relay";
const CONTAINER_SOCKET_PATH: &str = "/run/ags-webview-relay/relay.sock";
const CONTAINER_UPSTREAM_SOCKET_PATH: &str = "/run/ags-webview-relay/upstream.sock";
const MAX_REQUEST_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug)]
pub enum WebviewRelayError {
    RuntimeDirCreate(io::Error),
    RegisterSocketBind(io::Error),
}

impl fmt::Display for WebviewRelayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RuntimeDirCreate(e) => {
                write!(f, "webview relay: failed to create runtime dir: {e}")
            }
            Self::RegisterSocketBind(e) => {
                write!(f, "webview relay: failed to bind register socket: {e}")
            }
        }
    }
}

impl std::error::Error for WebviewRelayError {}

struct AppListenerGuard {
    port: u16,
    shutdown: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl AppListenerGuard {
    fn stop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        let _ = TcpStream::connect(("127.0.0.1", self.port));
        if let Some(handle) = self.thread.take() {
            let _ = handle.join();
        }
    }
}

pub struct WebviewRelayGuard {
    pub runtime_dir: PathBuf,
    register_socket_path: PathBuf,
    shutdown: Arc<AtomicBool>,
    register_thread: Option<JoinHandle<()>>,
    listeners: Arc<Mutex<Vec<AppListenerGuard>>>,
}

impl WebviewRelayGuard {
    pub fn container_runtime_dir() -> &'static str {
        CONTAINER_RUNTIME_DIR
    }

    pub fn container_socket_path() -> &'static str {
        CONTAINER_SOCKET_PATH
    }

    pub fn container_upstream_socket_path() -> &'static str {
        CONTAINER_UPSTREAM_SOCKET_PATH
    }
}

impl Drop for WebviewRelayGuard {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        let _ = UnixStream::connect(&self.register_socket_path);
        if let Some(handle) = self.register_thread.take() {
            let _ = handle.join();
        }
        if let Ok(mut listeners) = self.listeners.lock() {
            for listener in listeners.iter_mut() {
                listener.stop();
            }
            listeners.clear();
        }
        let _ = fs::remove_dir_all(&self.runtime_dir);
    }
}

impl fmt::Debug for WebviewRelayGuard {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let listener_count = self.listeners.lock().map(|v| v.len()).unwrap_or_default();
        f.debug_struct("WebviewRelayGuard")
            .field("runtime_dir", &self.runtime_dir)
            .field("register_socket_path", &self.register_socket_path)
            .field("listener_count", &listener_count)
            .finish()
    }
}

pub fn start(runtime_dir: &Path) -> Result<WebviewRelayGuard, WebviewRelayError> {
    crate::util::ensure_private_dir(runtime_dir).map_err(WebviewRelayError::RuntimeDirCreate)?;

    let register_socket_path = runtime_dir.join(SOCKET_NAME);
    if register_socket_path.exists() {
        let _ = fs::remove_file(&register_socket_path);
    }
    let listener =
        UnixListener::bind(&register_socket_path).map_err(WebviewRelayError::RegisterSocketBind)?;

    let shutdown = Arc::new(AtomicBool::new(false));
    let listeners = Arc::new(Mutex::new(Vec::new()));
    let runtime_dir_owned = runtime_dir.to_owned();
    let shutdown_clone = Arc::clone(&shutdown);
    let listeners_clone = Arc::clone(&listeners);

    let register_thread = thread::spawn(move || {
        accept_register_loop(
            listener,
            &runtime_dir_owned,
            &shutdown_clone,
            &listeners_clone,
        )
    });

    Ok(WebviewRelayGuard {
        runtime_dir: runtime_dir.to_owned(),
        register_socket_path,
        shutdown,
        register_thread: Some(register_thread),
        listeners,
    })
}

#[derive(Debug, Clone)]
struct Registration {
    sandbox_port: u16,
    base_path: String,
}

/// Accepts connections from a listener in a loop, spawning `handler` for each.
/// Stops when `shutdown` is set. `S` is the stream type yielded by the listener.
fn accept_loop<S: Send + 'static>(
    incoming: impl Iterator<Item = io::Result<S>>,
    shutdown: &AtomicBool,
    label: &str,
    mut handler: impl FnMut(S),
) {
    for stream in incoming {
        if shutdown.load(Ordering::Relaxed) {
            break;
        }
        match stream {
            Ok(stream) => {
                if shutdown.load(Ordering::Relaxed) {
                    break;
                }
                handler(stream);
            }
            Err(err) => {
                if shutdown.load(Ordering::Relaxed) {
                    break;
                }
                eprintln!("[ags webview-relay] {label} accept error: {err}");
            }
        }
    }
}

fn accept_register_loop(
    listener: UnixListener,
    runtime_dir: &Path,
    shutdown: &AtomicBool,
    listeners: &Arc<Mutex<Vec<AppListenerGuard>>>,
) {
    accept_loop(listener.incoming(), shutdown, "register", |stream| {
        let runtime_dir = runtime_dir.to_owned();
        let listeners = Arc::clone(listeners);
        thread::spawn(move || {
            if let Err(err) = handle_register_client(stream, &runtime_dir, &listeners) {
                eprintln!("[ags webview-relay] register client error: {err}");
            }
        });
    });
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum RegisterRequest {
    Register {
        port: u16,
        #[serde(default)]
        base_path: String,
    },
}

#[derive(Debug, Serialize, Deserialize)]
struct RegisterResponse {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    host_port: Option<u16>,
    #[serde(skip_serializing_if = "Option::is_none")]
    base_path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    url: Option<String>,
}

include!("webview_relay_register.rs");
include!("webview_relay_http.rs");

#[cfg(test)]
#[path = "webview_relay_tests.rs"]
mod tests;
