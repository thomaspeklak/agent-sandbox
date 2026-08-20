use std::fmt;
use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use serde_json::json;

use crate::auth_proxy::protocol::{HostMessage, ShimMessage};
use crate::webview_relay;

pub const SOCKET_NAME: &str = "auth-proxy.sock";
const CONTAINER_RUNTIME_DIR: &str = "/run/ags-auth-proxy";
const CONTAINER_SOCKET_PATH: &str = "/run/ags-auth-proxy/auth-proxy.sock";

/// Timeout for a single auth session (5 minutes).
const SESSION_TIMEOUT: Duration = Duration::from_secs(300);

/// Timeout for reading the callback response from the shim.
const CALLBACK_RELAY_TIMEOUT: Duration = Duration::from_secs(60);

type HttpHeaders = Vec<(String, String)>;
type HttpRequest = (String, String, HttpHeaders, String);

#[derive(Debug)]
pub enum AuthProxyError {
    RuntimeDirCreate(io::Error),
    SocketBind(io::Error),
}

impl fmt::Display for AuthProxyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RuntimeDirCreate(e) => write!(f, "auth proxy: failed to create runtime dir: {e}"),
            Self::SocketBind(e) => write!(f, "auth proxy: failed to bind socket: {e}"),
        }
    }
}

impl std::error::Error for AuthProxyError {}

/// Guard that manages the auth proxy lifetime.
///
/// The proxy runs in a background thread and is stopped when dropped.
/// The runtime directory is cleaned up on drop.
pub struct AuthProxyGuard {
    pub runtime_dir: PathBuf,
    shutdown: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl AuthProxyGuard {
    /// Container-side path where the runtime dir is mounted.
    pub fn container_runtime_dir() -> &'static str {
        CONTAINER_RUNTIME_DIR
    }

    /// Container-side socket path.
    pub fn container_socket_path() -> &'static str {
        CONTAINER_SOCKET_PATH
    }
}

impl Drop for AuthProxyGuard {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        // Connect to the socket to unblock the accept() call
        let _ = UnixStream::connect(self.runtime_dir.join(SOCKET_NAME));
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        let _ = fs::remove_dir_all(&self.runtime_dir);
    }
}

impl fmt::Debug for AuthProxyGuard {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AuthProxyGuard")
            .field("runtime_dir", &self.runtime_dir)
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenDecision {
    Cancel,
    OpenOriginal,
    Proxy,
}

/// Abstraction over prompt and browser-open operations for testability.
pub trait AuthProxyHost {
    /// Prompt the user to allow or deny a URL open.
    ///
    /// `has_callback` indicates whether the URL includes a localhost callback
    /// (changes the prompt wording). `can_proxy` indicates whether AGS can
    /// relay the URL through the sandbox-app webview relay instead of opening
    /// the raw host-local localhost URL.
    fn prompt_user(&self, url: &str, has_callback: bool, can_proxy: bool) -> OpenDecision;

    /// Whether this host can proxy the given URL through AGS.
    fn can_proxy(&self, _url: &str) -> bool {
        false
    }

    /// Rewrite a sandbox-local localhost URL through the AGS relay.
    fn resolve_proxy_url(&self, _url: &str) -> Result<String, String> {
        Err("proxy unavailable".to_owned())
    }

    /// Open a proxied localhost URL in the preferred host UI.
    fn open_proxy_target(&self, url: &str) -> Result<(), String> {
        self.open_browser(url)
    }

    /// Open a URL in the host browser.
    fn open_browser(&self, url: &str) -> Result<(), String>;
}

/// Real implementation that uses zenity/kdialog for prompts and xdg-open for browser.
struct HostUiWindowLease {
    _writer: UnixStream,
}

pub struct OsAuthProxyHost {
    auto_allow_domains: Vec<String>,
    webview_relay_socket_path: Option<PathBuf>,
    host_ui_socket_path: Option<PathBuf>,
    host_ui_windows: Mutex<Vec<HostUiWindowLease>>,
    next_host_ui_request_id: AtomicU64,
}

impl OsAuthProxyHost {
    pub fn new(
        auto_allow_domains: Vec<String>,
        webview_relay_socket_path: Option<PathBuf>,
        host_ui_socket_path: Option<PathBuf>,
    ) -> Self {
        Self {
            auto_allow_domains,
            webview_relay_socket_path,
            host_ui_socket_path,
            host_ui_windows: Mutex::new(Vec::new()),
            next_host_ui_request_id: AtomicU64::new(1),
        }
    }

    fn next_request_id(&self) -> String {
        format!(
            "auth_proxy_{}",
            self.next_host_ui_request_id.fetch_add(1, Ordering::Relaxed)
        )
    }
}

impl AuthProxyHost for OsAuthProxyHost {
    fn prompt_user(&self, url: &str, has_callback: bool, can_proxy: bool) -> OpenDecision {
        if is_auto_allowed(url, &self.auto_allow_domains) {
            return OpenDecision::OpenOriginal;
        }
        prompt_with_dialog(
            url,
            has_callback,
            can_proxy,
            self.host_ui_socket_path.as_deref(),
        )
    }

    fn can_proxy(&self, url: &str) -> bool {
        self.webview_relay_socket_path.is_some() && is_proxyable_localhost_url(url)
    }

    fn resolve_proxy_url(&self, url: &str) -> Result<String, String> {
        let Some(socket_path) = self.webview_relay_socket_path.as_deref() else {
            return Err("AGS webview relay is unavailable".to_owned());
        };
        rewrite_localhost_url_via_relay(url, socket_path)
    }

    fn open_proxy_target(&self, url: &str) -> Result<(), String> {
        if let Some(socket_path) = self.host_ui_socket_path.as_deref() {
            let lease = open_url_in_host_ui(socket_path, url, || self.next_request_id())?;
            self.host_ui_windows
                .lock()
                .map_err(|_| "host UI state lock poisoned".to_owned())?
                .push(lease);
            Ok(())
        } else {
            open_url_on_host(url)
        }
    }

    fn open_browser(&self, url: &str) -> Result<(), String> {
        open_url_on_host(url)
    }
}

// Start the auth proxy on a Unix socket inside `runtime_dir`.
//
// Creates the runtime directory and spawns a background thread that
// accepts connections from the container shim.

include!("host_session.rs");
include!("host_http.rs");
include!("host_ui_helpers.rs");

#[cfg(test)]
#[path = "host_tests.rs"]
mod tests;
