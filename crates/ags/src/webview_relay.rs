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
    fs::create_dir_all(runtime_dir).map_err(WebviewRelayError::RuntimeDirCreate)?;

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

fn accept_register_loop(
    listener: UnixListener,
    runtime_dir: &Path,
    shutdown: &AtomicBool,
    listeners: &Arc<Mutex<Vec<AppListenerGuard>>>,
) {
    for stream in listener.incoming() {
        if shutdown.load(Ordering::Relaxed) {
            break;
        }
        match stream {
            Ok(stream) => {
                if shutdown.load(Ordering::Relaxed) {
                    break;
                }
                let runtime_dir = runtime_dir.to_owned();
                let listeners = Arc::clone(listeners);
                thread::spawn(move || {
                    if let Err(err) = handle_register_client(stream, &runtime_dir, &listeners) {
                        eprintln!("[ags webview-relay] register client error: {err}");
                    }
                });
            }
            Err(err) => {
                if shutdown.load(Ordering::Relaxed) {
                    break;
                }
                eprintln!("[ags webview-relay] register accept error: {err}");
            }
        }
    }
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

pub(crate) fn register_local_app(
    socket_path: &Path,
    port: u16,
    base_path: &str,
) -> io::Result<String> {
    let mut stream = UnixStream::connect(socket_path)?;
    let line = serde_json::json!({
        "type": "register",
        "port": port,
        "base_path": base_path,
    })
    .to_string();
    writeln!(stream, "{line}")?;
    stream.flush()?;
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let response: RegisterResponse = serde_json::from_str(line.trim())
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    if !response.ok {
        return Err(io::Error::other(
            response
                .error
                .unwrap_or_else(|| "unknown relay registration error".to_owned()),
        ));
    }
    response
        .url
        .ok_or_else(|| io::Error::other("relay response omitted url"))
}

fn handle_register_client(
    mut stream: UnixStream,
    runtime_dir: &Path,
    listeners: &Arc<Mutex<Vec<AppListenerGuard>>>,
) -> io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(15))).ok();
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let response = match serde_json::from_str::<RegisterRequest>(line.trim()) {
        Ok(RegisterRequest::Register { port, base_path }) => {
            if port == 0 {
                RegisterResponse {
                    ok: false,
                    error: Some("invalid port".to_owned()),
                    host_port: None,
                    base_path: None,
                    url: None,
                }
            } else {
                let registration = Registration {
                    sandbox_port: port,
                    base_path: normalize_base_path(&base_path),
                };
                match start_app_listener(runtime_dir, registration.clone()) {
                    Ok(listener) => {
                        let host_port = listener.port;
                        if let Ok(mut guards) = listeners.lock() {
                            guards.push(listener);
                        }
                        RegisterResponse {
                            ok: true,
                            error: None,
                            host_port: Some(host_port),
                            url: Some(format!(
                                "http://127.0.0.1:{host_port}{}",
                                registration.base_path
                            )),
                            base_path: Some(registration.base_path),
                        }
                    }
                    Err(err) => RegisterResponse {
                        ok: false,
                        error: Some(format!("failed to allocate host listener: {err}")),
                        host_port: None,
                        base_path: None,
                        url: None,
                    },
                }
            }
        }
        Err(err) => RegisterResponse {
            ok: false,
            error: Some(format!("invalid request: {err}")),
            host_port: None,
            base_path: None,
            url: None,
        },
    };

    let body = serde_json::to_string(&response)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    writeln!(stream, "{body}")?;
    stream.flush()
}

fn normalize_base_path(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed == "/" {
        return "/".to_owned();
    }
    let mut normalized = if trimmed.starts_with('/') {
        trimmed.to_owned()
    } else {
        format!("/{trimmed}")
    };
    while normalized.len() > 1 && normalized.ends_with('/') {
        normalized.pop();
    }
    normalized
}

fn start_app_listener(
    runtime_dir: &Path,
    registration: Registration,
) -> io::Result<AppListenerGuard> {
    let listener = TcpListener::bind(("127.0.0.1", 0))?;
    let port = listener.local_addr()?.port();
    let shutdown = Arc::new(AtomicBool::new(false));
    let runtime_dir_owned = runtime_dir.to_owned();
    let shutdown_clone = Arc::clone(&shutdown);

    let thread = thread::spawn(move || {
        accept_http_loop(listener, &runtime_dir_owned, &registration, &shutdown_clone)
    });

    Ok(AppListenerGuard {
        port,
        shutdown,
        thread: Some(thread),
    })
}

fn accept_http_loop(
    listener: TcpListener,
    runtime_dir: &Path,
    registration: &Registration,
    shutdown: &AtomicBool,
) {
    for stream in listener.incoming() {
        if shutdown.load(Ordering::Relaxed) {
            break;
        }
        match stream {
            Ok(stream) => {
                if shutdown.load(Ordering::Relaxed) {
                    break;
                }
                let runtime_dir = runtime_dir.to_owned();
                let registration = registration.clone();
                thread::spawn(move || {
                    if let Err(err) = handle_http_client(stream, &runtime_dir, &registration) {
                        eprintln!("[ags webview-relay] client error: {err}");
                    }
                });
            }
            Err(err) => {
                if shutdown.load(Ordering::Relaxed) {
                    break;
                }
                eprintln!("[ags webview-relay] accept error: {err}");
            }
        }
    }
}

#[derive(Debug)]
struct HttpRequest {
    method: String,
    target: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

fn handle_http_client(
    mut stream: TcpStream,
    runtime_dir: &Path,
    registration: &Registration,
) -> io::Result<()> {
    stream.set_read_timeout(Some(Duration::from_secs(15))).ok();
    let request = read_http_request(&mut stream)?;

    if is_websocket_upgrade(&request.headers) {
        return write_http_response(
            &mut stream,
            501,
            "Not Implemented",
            &[("content-type", "text/plain; charset=utf-8")],
            b"WebSocket relay is not implemented yet\n",
        );
    }

    let relay_sock = runtime_dir.join(UPSTREAM_SOCKET_NAME);
    let relay_response = match send_relay_request(
        &relay_sock,
        &RelayRequest::HttpRequest {
            port: registration.sandbox_port,
            base_path: registration.base_path.clone(),
            method: request.method,
            path: request.target,
            headers: request.headers,
            body_base64: if request.body.is_empty() {
                None
            } else {
                Some(base64::engine::general_purpose::STANDARD.encode(request.body))
            },
        },
    ) {
        Ok(response) => response,
        Err(err) => {
            return write_http_response(
                &mut stream,
                502,
                "Bad Gateway",
                &[("content-type", "text/plain; charset=utf-8")],
                format!("Sandbox relay unavailable: {err}\n").as_bytes(),
            );
        }
    };

    if !relay_response.ok {
        let msg = relay_response
            .error
            .unwrap_or_else(|| "sandbox relay error".to_owned());
        return write_http_response(
            &mut stream,
            502,
            "Bad Gateway",
            &[("content-type", "text/plain; charset=utf-8")],
            format!("{msg}\n").as_bytes(),
        );
    }

    let status = relay_response.status.unwrap_or(500);
    let reason = relay_response
        .reason
        .unwrap_or_else(|| reason_phrase(status).to_owned());
    let body = relay_response
        .body_base64
        .as_deref()
        .and_then(|b| base64::engine::general_purpose::STANDARD.decode(b).ok())
        .unwrap_or_default();
    let headers = relay_response.headers.unwrap_or_default();
    write_http_response_owned(&mut stream, status, &reason, &headers, &body)
}

fn is_websocket_upgrade(headers: &[(String, String)]) -> bool {
    headers.iter().any(|(key, value)| {
        key.eq_ignore_ascii_case("upgrade") && value.eq_ignore_ascii_case("websocket")
    })
}

fn read_http_request(stream: &mut TcpStream) -> io::Result<HttpRequest> {
    let mut buf = Vec::new();
    let mut temp = [0u8; 4096];
    let header_end;
    loop {
        let read = stream.read(&mut temp)?;
        if read == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "request ended early",
            ));
        }
        buf.extend_from_slice(&temp[..read]);
        if buf.len() > MAX_REQUEST_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "request too large",
            ));
        }
        if let Some(idx) = find_header_end(&buf) {
            header_end = idx;
            break;
        }
    }

    let header_text = String::from_utf8_lossy(&buf[..header_end]);
    let mut lines = header_text.split("\r\n");
    let request_line = lines
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing request line"))?;
    let mut parts = request_line.split_whitespace();
    let method = parts
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing method"))?
        .to_owned();
    let target = parts
        .next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "missing target"))?
        .to_owned();

    let mut headers = Vec::new();
    let mut content_length = 0usize;
    for line in lines {
        if line.is_empty() {
            continue;
        }
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let name = name.trim().to_owned();
        let value = value.trim().to_owned();
        if name.eq_ignore_ascii_case("content-length") {
            content_length = value.parse::<usize>().unwrap_or(0);
        }
        headers.push((name, value));
    }

    let mut body = buf[header_end + 4..].to_vec();
    while body.len() < content_length {
        let read = stream.read(&mut temp)?;
        if read == 0 {
            break;
        }
        body.extend_from_slice(&temp[..read]);
        if body.len() > MAX_REQUEST_BYTES {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "body too large"));
        }
    }
    body.truncate(content_length);

    Ok(HttpRequest {
        method,
        target,
        headers,
        body,
    })
}

fn find_header_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

fn write_http_response(
    stream: &mut TcpStream,
    status: u16,
    reason: &str,
    headers: &[(&str, &str)],
    body: &[u8],
) -> io::Result<()> {
    let headers_owned: Vec<(String, String)> = headers
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect();
    write_http_response_owned(stream, status, reason, &headers_owned, body)
}

fn write_http_response_owned(
    stream: &mut TcpStream,
    status: u16,
    reason: &str,
    headers: &[(String, String)],
    body: &[u8],
) -> io::Result<()> {
    write!(stream, "HTTP/1.1 {status} {reason}\r\n")?;
    write!(stream, "Content-Length: {}\r\n", body.len())?;
    write!(stream, "Connection: close\r\n")?;
    for (name, value) in headers {
        if is_hop_by_hop_header(name) || name.eq_ignore_ascii_case("content-length") {
            continue;
        }
        write!(stream, "{name}: {value}\r\n")?;
    }
    write!(stream, "\r\n")?;
    stream.write_all(body)
}

fn is_hop_by_hop_header(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "connection"
            | "keep-alive"
            | "proxy-authenticate"
            | "proxy-authorization"
            | "te"
            | "trailer"
            | "transfer-encoding"
            | "upgrade"
    )
}

fn reason_phrase(status: u16) -> &'static str {
    match status {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        301 => "Moved Permanently",
        302 => "Found",
        304 => "Not Modified",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        405 => "Method Not Allowed",
        408 => "Request Timeout",
        409 => "Conflict",
        413 => "Payload Too Large",
        500 => "Internal Server Error",
        501 => "Not Implemented",
        502 => "Bad Gateway",
        503 => "Service Unavailable",
        504 => "Gateway Timeout",
        _ => "OK",
    }
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum RelayRequest {
    HttpRequest {
        port: u16,
        base_path: String,
        method: String,
        path: String,
        headers: Vec<(String, String)>,
        #[serde(skip_serializing_if = "Option::is_none")]
        body_base64: Option<String>,
    },
}

#[derive(Debug, Deserialize)]
struct RelayResponse {
    ok: bool,
    error: Option<String>,
    status: Option<u16>,
    reason: Option<String>,
    headers: Option<Vec<(String, String)>>,
    body_base64: Option<String>,
}

fn send_relay_request(sock_path: &Path, msg: &RelayRequest) -> io::Result<RelayResponse> {
    let mut stream = UnixStream::connect(sock_path)?;
    stream.set_read_timeout(Some(Duration::from_secs(15))).ok();
    let line = serde_json::to_string(msg)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    stream.write_all(line.as_bytes())?;
    stream.write_all(b"\n")?;
    stream.flush()?;

    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    if line.trim().is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "sandbox relay closed without a response",
        ));
    }
    serde_json::from_str(line.trim()).map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))
}

#[cfg(test)]
mod tests {
    use super::{SOCKET_NAME, UPSTREAM_SOCKET_NAME, start};
    use base64::Engine;
    use std::fs;
    use std::io::{BufRead, BufReader, Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::PathBuf;
    use std::process::{Command, Stdio};
    use std::sync::mpsc;
    use std::thread::{self, JoinHandle};
    use std::time::Duration;

    fn http_request(port: u16, target: &str, headers: &[(&str, &str)]) -> (u16, String, Vec<u8>) {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(stream, "GET {target} HTTP/1.1\r\nHost: 127.0.0.1\r\n").unwrap();
        for (name, value) in headers {
            write!(stream, "{name}: {value}\r\n").unwrap();
        }
        write!(stream, "Connection: close\r\n\r\n").unwrap();
        stream.flush().unwrap();

        let mut buf = Vec::new();
        stream.read_to_end(&mut buf).unwrap();
        let header_end = buf.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
        let header_text = String::from_utf8_lossy(&buf[..header_end]);
        let mut lines = header_text.split("\r\n");
        let status_line = lines.next().unwrap();
        let mut parts = status_line.split_whitespace();
        let _http = parts.next().unwrap();
        let status = parts.next().unwrap().parse::<u16>().unwrap();
        let body = buf[header_end + 4..].to_vec();
        (status, header_text.into_owned(), body)
    }

    fn register_app(
        runtime_dir: &std::path::Path,
        port: u16,
        base_path: &str,
    ) -> serde_json::Value {
        let socket_path = runtime_dir.join(SOCKET_NAME);
        let mut stream = UnixStream::connect(socket_path).unwrap();
        let line = serde_json::json!({
            "type": "register",
            "port": port,
            "base_path": base_path,
        })
        .to_string();
        writeln!(stream, "{line}").unwrap();
        stream.flush().unwrap();
        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        reader.read_line(&mut line).unwrap();
        serde_json::from_str(line.trim()).unwrap()
    }

    fn spawn_upstream_stub(
        socket_path: PathBuf,
        expected: usize,
    ) -> (mpsc::Receiver<serde_json::Value>, JoinHandle<()>) {
        let _ = fs::remove_file(&socket_path);
        let listener = UnixListener::bind(socket_path).unwrap();
        let (tx, rx) = mpsc::channel();
        let handle = thread::spawn(move || {
            for _ in 0..expected {
                let (stream, _) = listener.accept().unwrap();
                let mut reader = BufReader::new(stream);
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                let request: serde_json::Value = serde_json::from_str(line.trim()).unwrap();
                tx.send(request).unwrap();
                let response = serde_json::json!({
                    "ok": true,
                    "status": 200,
                    "reason": "OK",
                    "headers": [["content-type", "text/plain; charset=utf-8"]],
                    "body_base64": base64::engine::general_purpose::STANDARD.encode("hello from sandbox"),
                });
                let mut stream = reader.into_inner();
                writeln!(stream, "{response}").unwrap();
            }
        });
        (rx, handle)
    }

    fn app_server_script_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../agent/webview-relay-shim")
    }

    #[test]
    fn register_returns_dedicated_host_url_and_cleanup_works() {
        let dir = tempfile::tempdir().unwrap();
        let runtime_dir = dir.path().join("relay-runtime");
        let guard = start(&runtime_dir).unwrap();

        let response = register_app(&runtime_dir, 4173, "/");
        assert_eq!(response["ok"], true);
        assert_eq!(response["base_path"], "/");
        let url = response["url"].as_str().unwrap();
        assert!(url.starts_with("http://127.0.0.1:"));
        assert!(url.ends_with('/'));

        let runtime_dir = guard.runtime_dir.clone();
        drop(guard);
        assert!(!runtime_dir.exists());
    }

    #[test]
    fn missing_upstream_socket_returns_502() {
        let dir = tempfile::tempdir().unwrap();
        let runtime_dir = dir.path().join("relay-runtime");
        let guard = start(&runtime_dir).unwrap();
        let response = register_app(&runtime_dir, 4173, "/");
        let host_port = response["host_port"].as_u64().unwrap() as u16;

        let (status, _headers, body) = http_request(host_port, "/index.html", &[]);
        assert_eq!(status, 502);
        assert!(
            String::from_utf8(body)
                .unwrap()
                .contains("Sandbox relay unavailable")
        );

        drop(guard);
    }

    #[test]
    fn websocket_upgrade_returns_501() {
        let dir = tempfile::tempdir().unwrap();
        let runtime_dir = dir.path().join("relay-runtime");
        let guard = start(&runtime_dir).unwrap();
        let response = register_app(&runtime_dir, 4173, "/");
        let host_port = response["host_port"].as_u64().unwrap() as u16;

        let (status, _headers, body) = http_request(
            host_port,
            "/socket",
            &[("Upgrade", "websocket"), ("Connection", "Upgrade")],
        );
        assert_eq!(status, 501);
        assert!(
            String::from_utf8(body)
                .unwrap()
                .contains("WebSocket relay is not implemented yet")
        );

        drop(guard);
    }

    #[test]
    fn forwards_requests_using_allocated_host_port() {
        let dir = tempfile::tempdir().unwrap();
        let runtime_dir = dir.path().join("relay-runtime");
        fs::create_dir_all(&runtime_dir).unwrap();
        let (rx, server) = spawn_upstream_stub(runtime_dir.join(UPSTREAM_SOCKET_NAME), 1);

        let guard = start(&runtime_dir).unwrap();
        let response = register_app(&runtime_dir, 4173, "/");
        let host_port = response["host_port"].as_u64().unwrap() as u16;

        let (status, headers, body) = http_request(
            host_port,
            "/index.html?x=1",
            &[("Accept", "text/html"), ("X-Test", "1")],
        );
        assert_eq!(status, 200);
        assert!(
            headers.contains("content-type: text/plain; charset=utf-8")
                || headers.contains("Content-Type: text/plain; charset=utf-8")
        );
        assert_eq!(String::from_utf8(body).unwrap(), "hello from sandbox");

        let request = rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(request["type"], "http_request");
        assert_eq!(request["port"], 4173);
        assert_eq!(request["base_path"], "/");
        assert_eq!(request["method"], "GET");
        assert_eq!(request["path"], "/index.html?x=1");
        assert_eq!(request["headers"][0][0], "Host");

        drop(guard);
        server.join().unwrap();
    }

    #[test]
    fn helper_contract_returns_final_host_url() {
        let dir = tempfile::tempdir().unwrap();
        let runtime_dir = dir.path().join("relay-runtime");
        let guard = start(&runtime_dir).unwrap();

        let output = Command::new("python3")
            .arg(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../agent/webview-url-helper"))
            .arg("4173")
            .arg("/app")
            .env("AGS_WEBVIEW_RELAY_SOCKET", runtime_dir.join(SOCKET_NAME))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        let url = String::from_utf8(output.stdout).unwrap();
        assert!(url.trim().starts_with("http://127.0.0.1:"));
        assert!(url.trim().ends_with("/app"));

        drop(guard);
    }

    #[test]
    fn interview_style_root_absolute_requests_work_unchanged() {
        let app_listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let app_port = app_listener.local_addr().unwrap().port();
        let (app_tx, app_rx) = mpsc::channel();
        let app_thread = thread::spawn(move || {
            for _ in 0..4 {
                let (mut stream, _) = app_listener.accept().unwrap();
                let request = super::read_http_request(&mut stream).unwrap();
                app_tx.send(request.target.clone()).unwrap();
                let body = b"ok";
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\nContent-Type: text/plain\r\n\r\n",
                    body.len()
                );
                stream.write_all(response.as_bytes()).unwrap();
                stream.write_all(body).unwrap();
            }
        });

        let dir = tempfile::tempdir().unwrap();
        let runtime_dir = dir.path().join("relay-runtime");
        fs::create_dir_all(&runtime_dir).unwrap();
        let upstream_socket = runtime_dir.join(UPSTREAM_SOCKET_NAME);
        let mut shim = Command::new("python3")
            .arg(app_server_script_path())
            .env("AGS_WEBVIEW_RELAY_UPSTREAM_SOCKET", &upstream_socket)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();

        for _ in 0..50 {
            if upstream_socket.exists() {
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }
        assert!(
            upstream_socket.exists(),
            "shim did not create upstream socket"
        );

        let guard = start(&runtime_dir).unwrap();
        let response = register_app(&runtime_dir, app_port, "/");
        let host_port = response["host_port"].as_u64().unwrap() as u16;

        for path in [
            "/",
            "/styles.css",
            "/submit",
            "/media?path=image.png&session=abc123",
        ] {
            let (status, _, body) = http_request(host_port, path, &[]);
            assert_eq!(status, 200);
            assert_eq!(String::from_utf8(body).unwrap(), "ok");
        }

        let received: Vec<String> = (0..4)
            .map(|_| app_rx.recv_timeout(Duration::from_secs(2)).unwrap())
            .collect();
        assert_eq!(
            received,
            vec![
                "/".to_owned(),
                "/styles.css".to_owned(),
                "/submit".to_owned(),
                "/media?path=image.png&session=abc123".to_owned(),
            ]
        );

        drop(guard);
        let _ = shim.kill();
        let _ = shim.wait();
        app_thread.join().unwrap();
    }

    #[test]
    fn base_path_registrations_keep_root_absolute_assets_working() {
        let app_listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let app_port = app_listener.local_addr().unwrap().port();
        let (app_tx, app_rx) = mpsc::channel();
        let app_thread = thread::spawn(move || {
            for _ in 0..3 {
                let (mut stream, _) = app_listener.accept().unwrap();
                let request = super::read_http_request(&mut stream).unwrap();
                app_tx.send(request.target.clone()).unwrap();
                let body = b"ok";
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\nContent-Type: text/plain\r\n\r\n",
                    body.len()
                );
                stream.write_all(response.as_bytes()).unwrap();
                stream.write_all(body).unwrap();
            }
        });

        let dir = tempfile::tempdir().unwrap();
        let runtime_dir = dir.path().join("relay-runtime");
        fs::create_dir_all(&runtime_dir).unwrap();
        let upstream_socket = runtime_dir.join(UPSTREAM_SOCKET_NAME);
        let mut shim = Command::new("python3")
            .arg(app_server_script_path())
            .env("AGS_WEBVIEW_RELAY_UPSTREAM_SOCKET", &upstream_socket)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();

        for _ in 0..50 {
            if upstream_socket.exists() {
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }
        assert!(
            upstream_socket.exists(),
            "shim did not create upstream socket"
        );

        let guard = start(&runtime_dir).unwrap();
        let response = register_app(&runtime_dir, app_port, "/app");
        let host_port = response["host_port"].as_u64().unwrap() as u16;
        assert!(response["url"].as_str().unwrap().ends_with("/app"));

        for path in [
            "/app?session=abc123",
            "/styles.css",
            "/media?path=a.png&session=abc123",
        ] {
            let (status, _, body) = http_request(host_port, path, &[]);
            assert_eq!(status, 200);
            assert_eq!(String::from_utf8(body).unwrap(), "ok");
        }

        let received: Vec<String> = (0..3)
            .map(|_| app_rx.recv_timeout(Duration::from_secs(2)).unwrap())
            .collect();
        assert_eq!(
            received,
            vec![
                "/app?session=abc123".to_owned(),
                "/app/styles.css".to_owned(),
                "/app/media?path=a.png&session=abc123".to_owned(),
            ]
        );

        drop(guard);
        let _ = shim.kill();
        let _ = shim.wait();
        app_thread.join().unwrap();
    }
}
