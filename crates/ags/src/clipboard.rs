use std::fmt;
use std::fs;
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};

use base64::Engine;
use serde_json::{Value, json};

use crate::config::ClipboardMode;

pub const SOCKET_NAME: &str = "clipboard.sock";
pub const SHIM_NAME: &str = "clipboard-shim";
const CONTAINER_RUNTIME_DIR: &str = "/run/ags-clipboard";
const CONTAINER_SOCKET_PATH: &str = "/run/ags-clipboard/clipboard.sock";

#[derive(Debug)]
pub enum ClipboardError {
    RuntimeDirCreate(io::Error),
    SocketBind(io::Error),
}

impl fmt::Display for ClipboardError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RuntimeDirCreate(e) => {
                write!(f, "clipboard bridge: failed to create runtime dir: {e}")
            }
            Self::SocketBind(e) => write!(f, "clipboard bridge: failed to bind socket: {e}"),
        }
    }
}

impl std::error::Error for ClipboardError {}

pub struct ClipboardGuard {
    pub runtime_dir: PathBuf,
    socket_path: PathBuf,
    shutdown: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

impl ClipboardGuard {
    pub fn container_runtime_dir() -> &'static str {
        CONTAINER_RUNTIME_DIR
    }

    pub fn container_socket_path() -> &'static str {
        CONTAINER_SOCKET_PATH
    }
}

impl Drop for ClipboardGuard {
    fn drop(&mut self) {
        self.shutdown.store(true, Ordering::Relaxed);
        let _ = UnixStream::connect(&self.socket_path);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        let _ = fs::remove_dir_all(&self.runtime_dir);
    }
}

pub fn start(
    runtime_dir: &Path,
    mode: ClipboardMode,
    max_bytes: usize,
) -> Result<ClipboardGuard, ClipboardError> {
    start_with_backend(runtime_dir, mode, max_bytes, Arc::new(OsClipboardBackend))
}

fn start_with_backend(
    runtime_dir: &Path,
    mode: ClipboardMode,
    max_bytes: usize,
    backend: Arc<dyn ClipboardBackend>,
) -> Result<ClipboardGuard, ClipboardError> {
    crate::util::ensure_private_dir(runtime_dir).map_err(ClipboardError::RuntimeDirCreate)?;
    let socket_path = runtime_dir.join(SOCKET_NAME);
    if socket_path.exists() {
        let _ = fs::remove_file(&socket_path);
    }
    let listener = UnixListener::bind(&socket_path).map_err(ClipboardError::SocketBind)?;
    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_clone = Arc::clone(&shutdown);
    let backend_clone = Arc::clone(&backend);

    let thread = thread::spawn(move || {
        for stream in listener.incoming() {
            if shutdown_clone.load(Ordering::Relaxed) {
                break;
            }
            match stream {
                Ok(stream) => {
                    let backend = Arc::clone(&backend_clone);
                    thread::spawn(move || handle_client(stream, mode, max_bytes, backend));
                }
                Err(err) => {
                    if !shutdown_clone.load(Ordering::Relaxed) {
                        eprintln!("[ags clipboard] accept error: {err}");
                    }
                }
            }
        }
    });

    Ok(ClipboardGuard {
        runtime_dir: runtime_dir.to_owned(),
        socket_path,
        shutdown,
        thread: Some(thread),
    })
}

trait ClipboardBackend: Send + Sync + 'static {
    fn list_types(&self) -> Result<Vec<String>, String>;
    fn read(&self, mime: Option<&str>, max_bytes: usize) -> Result<(String, Vec<u8>), String>;
    fn write(&self, mime: &str, data: &[u8]) -> Result<(), String>;
}

struct OsClipboardBackend;

impl ClipboardBackend for OsClipboardBackend {
    fn list_types(&self) -> Result<Vec<String>, String> {
        let output = Command::new("wl-paste")
            .arg("--list-types")
            .output()
            .map_err(|e| format!("failed to run wl-paste --list-types: {e}"))?;
        if !output.status.success() {
            return Err(command_error("wl-paste --list-types", &output.stderr));
        }
        Ok(String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
            .collect())
    }

    fn read(&self, mime: Option<&str>, max_bytes: usize) -> Result<(String, Vec<u8>), String> {
        let selected = mime.unwrap_or("text/plain;charset=utf-8");
        let mut cmd = Command::new("wl-paste");
        if let Some(mime) = mime {
            cmd.arg("--type").arg(mime);
        }
        let output = cmd
            .output()
            .map_err(|e| format!("failed to run wl-paste: {e}"))?;
        if !output.status.success() {
            return Err(command_error("wl-paste", &output.stderr));
        }
        if output.stdout.len() > max_bytes {
            return Err(format!(
                "clipboard payload is {} bytes, above limit {max_bytes}",
                output.stdout.len()
            ));
        }
        Ok((selected.to_owned(), output.stdout))
    }

    fn write(&self, mime: &str, data: &[u8]) -> Result<(), String> {
        let mut child = Command::new("wl-copy")
            .arg("--type")
            .arg(mime)
            .stdin(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("failed to run wl-copy: {e}"))?;
        child
            .stdin
            .as_mut()
            .ok_or_else(|| "failed to open wl-copy stdin".to_owned())?
            .write_all(data)
            .map_err(|e| format!("failed to write wl-copy stdin: {e}"))?;
        let output = child
            .wait_with_output()
            .map_err(|e| format!("failed to wait for wl-copy: {e}"))?;
        if output.status.success() {
            Ok(())
        } else {
            Err(command_error("wl-copy", &output.stderr))
        }
    }
}

fn command_error(command: &str, stderr: &[u8]) -> String {
    let msg = String::from_utf8_lossy(stderr).trim().to_owned();
    if msg.is_empty() {
        format!("{command} failed")
    } else {
        format!("{command} failed: {msg}")
    }
}

fn handle_client(
    stream: UnixStream,
    mode: ClipboardMode,
    max_bytes: usize,
    backend: Arc<dyn ClipboardBackend>,
) {
    if let Err(err) = handle_client_result(stream, mode, max_bytes, backend) {
        eprintln!("[ags clipboard] client error: {err}");
    }
}

fn handle_client_result(
    mut stream: UnixStream,
    mode: ClipboardMode,
    max_bytes: usize,
    backend: Arc<dyn ClipboardBackend>,
) -> io::Result<()> {
    let mut line = String::new();
    BufReader::new(stream.try_clone()?).read_line(&mut line)?;
    let response = match serde_json::from_str::<Value>(&line) {
        Ok(request) => handle_request(&request, mode, max_bytes, backend.as_ref()),
        Err(err) => json!({ "ok": false, "error": format!("invalid JSON: {err}") }),
    };
    let encoded = serde_json::to_string(&response).map_err(io::Error::other)?;
    stream.write_all(encoded.as_bytes())?;
    stream.write_all(b"\n")?;
    stream.flush()
}

fn handle_request(
    request: &Value,
    mode: ClipboardMode,
    max_bytes: usize,
    backend: &dyn ClipboardBackend,
) -> Value {
    match request.get("op").and_then(Value::as_str) {
        Some("list") if mode.can_read() => match backend.list_types() {
            Ok(types) => json!({ "ok": true, "types": types }),
            Err(error) => json!({ "ok": false, "error": error }),
        },
        Some("read") if mode.can_read() => {
            let mime = request
                .get("mime")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty());
            match backend.read(mime, max_bytes) {
                Ok((mime, data)) => json!({
                    "ok": true,
                    "mime": mime,
                    "data_b64": base64::engine::general_purpose::STANDARD.encode(data),
                }),
                Err(error) => json!({ "ok": false, "error": error }),
            }
        }
        Some("write") if mode.can_write() => {
            let mime = request
                .get("mime")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .unwrap_or("text/plain;charset=utf-8");
            let encoded = request
                .get("data_b64")
                .and_then(Value::as_str)
                .unwrap_or("");
            match decode_limited(encoded, max_bytes).and_then(|data| backend.write(mime, &data)) {
                Ok(()) => json!({ "ok": true }),
                Err(error) => json!({ "ok": false, "error": error }),
            }
        }
        Some("list") | Some("read") => json!({ "ok": false, "error": "clipboard read disabled" }),
        Some("write") => json!({ "ok": false, "error": "clipboard write disabled" }),
        Some(other) => {
            json!({ "ok": false, "error": format!("unsupported clipboard op: {other}") })
        }
        None => json!({ "ok": false, "error": "missing clipboard op" }),
    }
}

fn decode_limited(encoded: &str, max_bytes: usize) -> Result<Vec<u8>, String> {
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|e| format!("invalid base64 data: {e}"))?;
    if decoded.len() > max_bytes {
        Err(format!(
            "clipboard payload is {} bytes, above limit {max_bytes}",
            decoded.len()
        ))
    } else {
        Ok(decoded)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Default)]
    struct MockBackend {
        writes: Mutex<Vec<(String, Vec<u8>)>>,
    }

    impl ClipboardBackend for MockBackend {
        fn list_types(&self) -> Result<Vec<String>, String> {
            Ok(vec!["text/plain".to_owned(), "image/png".to_owned()])
        }

        fn read(&self, mime: Option<&str>, _max_bytes: usize) -> Result<(String, Vec<u8>), String> {
            Ok((mime.unwrap_or("text/plain").to_owned(), b"hello".to_vec()))
        }

        fn write(&self, mime: &str, data: &[u8]) -> Result<(), String> {
            self.writes
                .lock()
                .unwrap()
                .push((mime.to_owned(), data.to_vec()));
            Ok(())
        }
    }

    #[test]
    fn read_request_returns_base64_payload() {
        let backend = MockBackend::default();
        let response = handle_request(
            &json!({"op":"read", "mime":"text/plain"}),
            ClipboardMode::Read,
            1024,
            &backend,
        );
        assert_eq!(response["ok"], true);
        assert_eq!(response["data_b64"], "aGVsbG8=");
    }

    #[test]
    fn write_request_respects_mode() {
        let backend = MockBackend::default();
        let response = handle_request(
            &json!({"op":"write", "mime":"text/plain", "data_b64":"aGk="}),
            ClipboardMode::Read,
            1024,
            &backend,
        );
        assert_eq!(response["ok"], false);
        assert!(
            response["error"]
                .as_str()
                .unwrap()
                .contains("write disabled")
        );
    }

    #[test]
    fn oversized_write_is_rejected() {
        let backend = MockBackend::default();
        let response = handle_request(
            &json!({"op":"write", "data_b64":"aGVsbG8="}),
            ClipboardMode::ReadWrite,
            2,
            &backend,
        );
        assert_eq!(response["ok"], false);
        assert!(response["error"].as_str().unwrap().contains("above limit"));
    }
}
