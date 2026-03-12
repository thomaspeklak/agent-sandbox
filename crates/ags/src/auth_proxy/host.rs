use std::fmt;
use std::fs;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use crate::auth_proxy::protocol::{HostMessage, ShimMessage};

pub const SOCKET_NAME: &str = "auth-proxy.sock";
const CONTAINER_RUNTIME_DIR: &str = "/run/ags-auth-proxy";
const CONTAINER_SOCKET_PATH: &str = "/run/ags-auth-proxy/auth-proxy.sock";

/// Timeout for a single auth session (5 minutes).
const SESSION_TIMEOUT: Duration = Duration::from_secs(300);

/// Timeout for reading the callback response from the shim.
const CALLBACK_RELAY_TIMEOUT: Duration = Duration::from_secs(60);

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

/// Abstraction over prompt and browser-open operations for testability.
pub trait AuthProxyHost {
    /// Prompt the user to allow or deny a URL open.
    ///
    /// `has_callback` indicates whether the URL includes a localhost callback
    /// (changes the prompt wording).
    fn prompt_user(&self, url: &str, has_callback: bool) -> bool;

    /// Open a URL in the host browser.
    fn open_browser(&self, url: &str) -> Result<(), String>;
}

/// Real implementation that uses zenity/kdialog for prompts and xdg-open for browser.
pub struct OsAuthProxyHost {
    auto_allow_domains: Vec<String>,
}

impl OsAuthProxyHost {
    pub fn new(auto_allow_domains: Vec<String>) -> Self {
        Self {
            auto_allow_domains,
        }
    }
}

impl AuthProxyHost for OsAuthProxyHost {
    fn prompt_user(&self, url: &str, has_callback: bool) -> bool {
        if is_auto_allowed(url, &self.auto_allow_domains) {
            return true;
        }
        prompt_with_dialog(url, has_callback)
    }

    fn open_browser(&self, url: &str) -> Result<(), String> {
        open_url_on_host(url)
    }
}

/// Start the auth proxy on a Unix socket inside `runtime_dir`.
///
/// Creates the runtime directory and spawns a background thread that
/// accepts connections from the container shim.
pub fn start(
    runtime_dir: &Path,
    auto_allow_domains: Vec<String>,
) -> Result<AuthProxyGuard, AuthProxyError> {
    start_with_host(runtime_dir, Arc::new(OsAuthProxyHost::new(auto_allow_domains)))
}

/// Start the auth proxy with a custom host implementation (for testing).
pub fn start_with_host(
    runtime_dir: &Path,
    host: Arc<dyn AuthProxyHost + Send + Sync>,
) -> Result<AuthProxyGuard, AuthProxyError> {
    fs::create_dir_all(runtime_dir).map_err(AuthProxyError::RuntimeDirCreate)?;

    let sock_path = runtime_dir.join(SOCKET_NAME);
    // Remove stale socket if present
    let _ = fs::remove_file(&sock_path);

    let listener = UnixListener::bind(&sock_path).map_err(AuthProxyError::SocketBind)?;

    // Make socket world-accessible (container user may have different UID mapping)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(&sock_path, fs::Permissions::from_mode(0o666));
    }

    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_clone = shutdown.clone();
    let runtime_dir_owned = runtime_dir.to_owned();

    let thread = thread::spawn(move || {
        accept_loop(&listener, &shutdown_clone, &host);
    });

    Ok(AuthProxyGuard {
        runtime_dir: runtime_dir_owned,
        shutdown,
        thread: Some(thread),
    })
}

fn accept_loop(
    listener: &UnixListener,
    shutdown: &AtomicBool,
    host: &Arc<dyn AuthProxyHost + Send + Sync>,
) {
    for stream in listener.incoming() {
        if shutdown.load(Ordering::Relaxed) {
            break;
        }
        match stream {
            Ok(stream) => {
                let host = Arc::clone(host);
                thread::spawn(move || {
                    if let Err(e) = handle_session(stream, host.as_ref()) {
                        eprintln!("[ags auth-proxy] session error: {e}");
                    }
                });
            }
            Err(e) => {
                if shutdown.load(Ordering::Relaxed) {
                    break;
                }
                eprintln!("[ags auth-proxy] accept error: {e}");
            }
        }
    }
}

fn handle_session(
    stream: UnixStream,
    host: &dyn AuthProxyHost,
) -> Result<(), Box<dyn std::error::Error>> {
    stream
        .set_read_timeout(Some(SESSION_TIMEOUT))
        .ok();

    let reader_stream = stream.try_clone()?;
    let mut reader = BufReader::new(reader_stream);
    let mut writer = stream;

    let mut line = String::new();
    reader.read_line(&mut line)?;

    if line.is_empty() {
        return Ok(()); // shutdown wake-up connection
    }

    let msg: ShimMessage = serde_json::from_str(line.trim())?;

    match msg {
        ShimMessage::OpenUrl {
            session_id,
            url,
            callback_port,
        } => handle_open_url(&session_id, &url, callback_port, &mut reader, &mut writer, host),
        _ => {
            send_message(
                &mut writer,
                &HostMessage::Error {
                    session_id: "unknown".into(),
                    message: "expected open_url as first message".into(),
                },
            )?;
            Ok(())
        }
    }
}

fn handle_open_url(
    session_id: &str,
    url: &str,
    callback_port: Option<u16>,
    reader: &mut BufReader<UnixStream>,
    writer: &mut UnixStream,
    host: &dyn AuthProxyHost,
) -> Result<(), Box<dyn std::error::Error>> {
    let has_callback = callback_port.is_some();

    // Prompt user
    let allowed = host.prompt_user(url, has_callback);

    send_message(
        writer,
        &HostMessage::PromptResult {
            session_id: session_id.to_owned(),
            allowed,
        },
    )?;

    if !allowed {
        send_message(
            writer,
            &HostMessage::SessionComplete {
                session_id: session_id.to_owned(),
            },
        )?;
        return Ok(());
    }

    if let Some(port) = callback_port {
        handle_callback_flow(session_id, url, port, reader, writer, host)?;
    } else {
        // Simple open: just open the browser
        if let Err(e) = host.open_browser(url) {
            send_message(
                writer,
                &HostMessage::Error {
                    session_id: session_id.to_owned(),
                    message: format!("failed to open browser: {e}"),
                },
            )?;
            return Ok(());
        }
        send_message(
            writer,
            &HostMessage::SessionComplete {
                session_id: session_id.to_owned(),
            },
        )?;
    }

    Ok(())
}

fn handle_callback_flow(
    session_id: &str,
    url: &str,
    callback_port: u16,
    reader: &mut BufReader<UnixStream>,
    writer: &mut UnixStream,
    host: &dyn AuthProxyHost,
) -> Result<(), Box<dyn std::error::Error>> {
    // Bind the callback listener on the host loopback BEFORE opening the browser,
    // so the callback port is ready when the browser redirects.
    // Use SO_REUSEADDR so rapid retry (deny → allow, or successive OAuth flows)
    // doesn't fail with EADDRINUSE from TIME_WAIT sockets.
    let callback_listener = bind_callback_listener(callback_port)?;

    // Open the browser
    if let Err(e) = host.open_browser(url) {
        drop(callback_listener);
        send_message(
            writer,
            &HostMessage::Error {
                session_id: session_id.to_owned(),
                message: format!("failed to open browser: {e}"),
            },
        )?;
        return Ok(());
    }

    // Wait for the callback HTTP request from the browser, then drop the
    // listener immediately so the port is released.
    let (mut tcp_stream, _addr) = callback_listener.accept()?;
    drop(callback_listener);
    tcp_stream
        .set_read_timeout(Some(SESSION_TIMEOUT))
        .ok();

    // Read the raw HTTP request
    let (method, path, headers, body) = read_http_request(&mut tcp_stream)?;

    let request_id = format!("{session_id}-cb");

    // Relay the callback to the container shim
    send_message(
        writer,
        &HostMessage::CallbackRequest {
            session_id: session_id.to_owned(),
            request_id: request_id.clone(),
            method,
            path,
            headers,
            body,
        },
    )?;

    // Shorten the read timeout for the callback relay phase
    reader
        .get_ref()
        .set_read_timeout(Some(CALLBACK_RELAY_TIMEOUT))
        .ok();

    let mut line = String::new();
    reader.read_line(&mut line)?;
    let response: ShimMessage = serde_json::from_str(line.trim())?;

    // Send the HTTP response back to the browser
    match response {
        ShimMessage::CallbackResponse {
            status,
            headers,
            body,
            ..
        } => {
            write_http_response(&mut tcp_stream, status, &headers, &body)?;
        }
        _ => {
            write_http_response(
                &mut tcp_stream,
                502,
                &[("Content-Type".to_owned(), "text/plain".to_owned())],
                "auth proxy: unexpected response from container",
            )?;
        }
    }

    send_message(
        writer,
        &HostMessage::SessionComplete {
            session_id: session_id.to_owned(),
        },
    )?;

    Ok(())
}

// --- JSON messaging ---

fn send_message(writer: &mut dyn Write, msg: &HostMessage) -> io::Result<()> {
    let json = serde_json::to_string(msg).map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;
    writer.write_all(json.as_bytes())?;
    writer.write_all(b"\n")?;
    writer.flush()
}

// --- Callback listener ---

/// Bind a TCP listener on the loopback callback port with SO_REUSEADDR set
/// **before** bind, so that back-to-back OAuth flows don't hit EADDRINUSE
/// from lingering TIME_WAIT sockets.
fn bind_callback_listener(port: u16) -> io::Result<TcpListener> {
    use std::os::unix::io::FromRawFd;

    unsafe {
        let fd = libc::socket(libc::AF_INET, libc::SOCK_STREAM, 0);
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }

        let yes: libc::c_int = 1;
        libc::setsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_REUSEADDR,
            &yes as *const _ as *const libc::c_void,
            std::mem::size_of::<libc::c_int>() as libc::socklen_t,
        );

        let addr = libc::sockaddr_in {
            sin_family: libc::AF_INET as libc::sa_family_t,
            sin_port: port.to_be(),
            sin_addr: libc::in_addr {
                s_addr: u32::from_ne_bytes([127, 0, 0, 1]),
            },
            sin_zero: [0; 8],
        };

        if libc::bind(
            fd,
            &addr as *const _ as *const libc::sockaddr,
            std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t,
        ) < 0
        {
            let err = io::Error::last_os_error();
            libc::close(fd);
            return Err(err);
        }

        if libc::listen(fd, 1) < 0 {
            let err = io::Error::last_os_error();
            libc::close(fd);
            return Err(err);
        }

        Ok(TcpListener::from_raw_fd(fd))
    }
}

// --- Minimal HTTP parsing ---

/// Read an HTTP/1.x request from a stream. Returns (method, path, headers, body).
fn read_http_request(
    stream: &mut dyn Read,
) -> Result<(String, String, Vec<(String, String)>, String), Box<dyn std::error::Error>> {
    let mut buf = Vec::with_capacity(8192);
    let mut byte = [0u8; 1];

    // Read until we see \r\n\r\n (end of headers)
    loop {
        stream.read_exact(&mut byte)?;
        buf.push(byte[0]);
        if buf.len() >= 4 && &buf[buf.len() - 4..] == b"\r\n\r\n" {
            break;
        }
        if buf.len() > 65536 {
            return Err("HTTP request headers too large".into());
        }
    }

    let header_text = String::from_utf8_lossy(&buf);
    let mut lines = header_text.lines();

    // Request line: "GET /path HTTP/1.1"
    let request_line = lines.next().ok_or("empty HTTP request")?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next().ok_or("missing HTTP method")?.to_owned();
    let path = parts.next().ok_or("missing HTTP path")?.to_owned();

    // Headers
    let mut headers = Vec::new();
    let mut content_length: usize = 0;
    for line in lines {
        let line = line.trim();
        if line.is_empty() {
            break;
        }
        if let Some((key, value)) = line.split_once(':') {
            let key = key.trim().to_owned();
            let value = value.trim().to_owned();
            if key.eq_ignore_ascii_case("content-length") {
                content_length = value.parse().unwrap_or(0);
            }
            headers.push((key, value));
        }
    }

    // Body
    let mut body_bytes = vec![0u8; content_length];
    if content_length > 0 {
        stream.read_exact(&mut body_bytes)?;
    }
    let body = String::from_utf8_lossy(&body_bytes).into_owned();

    Ok((method, path, headers, body))
}

/// Write an HTTP/1.1 response to a stream.
fn write_http_response(
    stream: &mut dyn Write,
    status: u16,
    headers: &[(String, String)],
    body: &str,
) -> io::Result<()> {
    let reason = match status {
        200 => "OK",
        301 => "Moved Permanently",
        302 => "Found",
        400 => "Bad Request",
        500 => "Internal Server Error",
        502 => "Bad Gateway",
        _ => "OK",
    };

    write!(stream, "HTTP/1.1 {status} {reason}\r\n")?;

    let mut has_content_length = false;
    let mut has_connection = false;
    for (key, value) in headers {
        write!(stream, "{key}: {value}\r\n")?;
        if key.eq_ignore_ascii_case("content-length") {
            has_content_length = true;
        }
        if key.eq_ignore_ascii_case("connection") {
            has_connection = true;
        }
    }
    if !has_content_length {
        write!(stream, "Content-Length: {}\r\n", body.len())?;
    }
    if !has_connection {
        write!(stream, "Connection: close\r\n")?;
    }
    write!(stream, "\r\n")?;
    stream.write_all(body.as_bytes())?;
    stream.flush()
}

// --- Prompt ---

/// Check if the URL's host matches any auto-allowed domain.
fn is_auto_allowed(url: &str, domains: &[String]) -> bool {
    if domains.is_empty() {
        return false;
    }
    // Extract host from URL: skip "https://" or "http://", take up to next '/' or ':'
    let host = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
        .unwrap_or(url)
        .split(['/', ':', '?'])
        .next()
        .unwrap_or("");
    domains
        .iter()
        .any(|d| host == d.as_str() || host.ends_with(&format!(".{d}")))
}

/// Try zenity, then kdialog, then deny.
fn prompt_with_dialog(url: &str, has_callback: bool) -> bool {
    if let Some(result) = try_zenity(url, has_callback) {
        return result;
    }
    if let Some(result) = try_kdialog(url, has_callback) {
        return result;
    }

    eprintln!("[ags auth-proxy] no dialog tool available (install zenity or kdialog)");
    eprintln!("[ags auth-proxy] denying URL open: {url}");
    false
}

/// Produce a display-safe URL: strip query string and escape Pango/XML markup characters.
fn display_url(url: &str) -> String {
    let short = match url.find('?') {
        Some(i) => format!("{}?...", &url[..i]),
        None => url.to_owned(),
    };
    short
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn try_zenity(url: &str, has_callback: bool) -> Option<bool> {
    let display = display_url(url);
    let text = if has_callback {
        format!(
            "A sandbox tool wants to open this URL and capture a localhost callback:\n\n{display}\n\nAllow this browser open and callback relay?"
        )
    } else {
        format!("A sandbox tool wants to open this URL:\n\n{display}\n\nAllow this browser open?")
    };

    let status = std::process::Command::new("zenity")
        .args(["--question", "--title", "AGS Auth Proxy", "--width", "500", "--no-wrap"])
        .arg("--text")
        .arg(&text)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .ok()?;

    Some(status.success())
}

fn try_kdialog(url: &str, has_callback: bool) -> Option<bool> {
    let display = display_url(url);
    let text = if has_callback {
        format!(
            "A sandbox tool wants to open this URL and capture a localhost callback:\n\n{display}\n\nAllow?"
        )
    } else {
        format!("A sandbox tool wants to open:\n\n{display}\n\nAllow?")
    };

    let status = std::process::Command::new("kdialog")
        .args(["--yesno", &text, "--title", "AGS Auth Proxy"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .ok()?;

    Some(status.success())
}

// --- Browser open ---

fn open_url_on_host(url: &str) -> Result<(), String> {
    let status = std::process::Command::new("xdg-open")
        .arg(url)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|e| format!("xdg-open failed to start: {e}"))?;

    if status.success() {
        Ok(())
    } else {
        Err(format!("xdg-open exited with {status}"))
    }
}
