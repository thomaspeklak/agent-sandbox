use std::fmt;
use std::fs;
use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use base64::Engine;
use serde_json::{Value, json};

use crate::config::ClipboardMode;

pub const SOCKET_NAME: &str = "clipboard.sock";
pub const SHIM_NAME: &str = "clipboard-shim";
const CONTAINER_RUNTIME_DIR: &str = "/run/ags-clipboard";
const CONTAINER_SOCKET_PATH: &str = "/run/ags-clipboard/clipboard.sock";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ClipboardApprovalConfig {
    pub required: bool,
    pub window_seconds: u64,
    pub approve_writes: bool,
}

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
    approval: ClipboardApprovalConfig,
    host_ui_socket: Option<&Path>,
) -> Result<ClipboardGuard, ClipboardError> {
    start_with_backend(
        runtime_dir,
        mode,
        max_bytes,
        Arc::new(PromptingClipboardAccess::new(
            approval,
            host_ui_socket.map(Path::to_owned),
        )),
        Arc::new(OsClipboardBackend),
    )
}

fn start_with_backend(
    runtime_dir: &Path,
    mode: ClipboardMode,
    max_bytes: usize,
    access: Arc<dyn ClipboardAccessAuthorizer>,
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
    let access_clone = Arc::clone(&access);
    let backend_clone = Arc::clone(&backend);

    let thread = thread::spawn(move || {
        for stream in listener.incoming() {
            if shutdown_clone.load(Ordering::Relaxed) {
                break;
            }
            match stream {
                Ok(stream) => {
                    let access = Arc::clone(&access_clone);
                    let backend = Arc::clone(&backend_clone);
                    thread::spawn(move || handle_client(stream, mode, max_bytes, access, backend));
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClipboardOperation {
    Read,
    Write,
}

trait ClipboardAccessAuthorizer: Send + Sync + 'static {
    fn authorize(&self, operation: ClipboardOperation, mime: Option<&str>) -> Result<(), String>;
}

#[cfg(test)]
struct AllowAllClipboardAccess;

#[cfg(test)]
impl ClipboardAccessAuthorizer for AllowAllClipboardAccess {
    fn authorize(&self, _operation: ClipboardOperation, _mime: Option<&str>) -> Result<(), String> {
        Ok(())
    }
}

struct PromptingClipboardAccess {
    approval: ClipboardApprovalConfig,
    host_ui_socket: Option<PathBuf>,
    approved_until: Mutex<Option<Instant>>,
}

impl PromptingClipboardAccess {
    fn new(approval: ClipboardApprovalConfig, host_ui_socket: Option<PathBuf>) -> Self {
        Self {
            approval,
            host_ui_socket,
            approved_until: Mutex::new(None),
        }
    }

    fn requires_prompt(&self, operation: ClipboardOperation) -> bool {
        self.approval.required
            && (matches!(operation, ClipboardOperation::Read)
                || (self.approval.approve_writes && matches!(operation, ClipboardOperation::Write)))
    }
}

impl ClipboardAccessAuthorizer for PromptingClipboardAccess {
    fn authorize(&self, operation: ClipboardOperation, mime: Option<&str>) -> Result<(), String> {
        if !self.requires_prompt(operation) {
            return Ok(());
        }

        let mut approved_until = self
            .approved_until
            .lock()
            .map_err(|_| "clipboard approval state lock poisoned".to_owned())?;
        if approved_until.is_some_and(|until| Instant::now() < until) {
            return Ok(());
        }

        match prompt_clipboard_access(operation, mime, self.approval, self.host_ui_socket.as_deref()) {
            ClipboardApprovalDecision::AllowOnce => Ok(()),
            ClipboardApprovalDecision::AllowWindow => {
                *approved_until = Some(Instant::now() + Duration::from_secs(self.approval.window_seconds));
                Ok(())
            }
            ClipboardApprovalDecision::Deny => Err("clipboard access denied by user".to_owned()),
            ClipboardApprovalDecision::Unavailable => {
                Err("clipboard access denied: no dialog tool available (enable [host_ui] or install zenity/kdialog, or set [clipboard].approval_required = false)".to_owned())
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ClipboardApprovalDecision {
    AllowOnce,
    AllowWindow,
    Deny,
    Unavailable,
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

fn prompt_clipboard_access(
    operation: ClipboardOperation,
    mime: Option<&str>,
    approval: ClipboardApprovalConfig,
    host_ui_socket: Option<&Path>,
) -> ClipboardApprovalDecision {
    let action = match operation {
        ClipboardOperation::Read => "read host clipboard contents",
        ClipboardOperation::Write => "write to the host clipboard",
    };
    let mut choices = Vec::new();
    if approval.window_seconds > 0 {
        choices.push(crate::host_dialog::DialogChoice::new(
            "allow_window",
            format!("Allow for {}", format_duration(approval.window_seconds)),
            crate::host_dialog::DialogChoiceRole::Primary,
        ));
    }
    choices.push(crate::host_dialog::DialogChoice::new(
        "allow_once",
        "Allow once",
        if approval.window_seconds > 0 {
            crate::host_dialog::DialogChoiceRole::Secondary
        } else {
            crate::host_dialog::DialogChoiceRole::Primary
        },
    ));
    choices.push(crate::host_dialog::DialogChoice::new(
        "deny",
        "Deny",
        crate::host_dialog::DialogChoiceRole::Cancel,
    ));

    let request = crate::host_dialog::DialogRequest {
        title: "AGS Clipboard".to_owned(),
        heading: "Allow sandbox clipboard access?".to_owned(),
        message: format!("A sandbox process wants to {action}."),
        details: vec![
            crate::host_dialog::DialogDetail::new(
                "Operation",
                match operation {
                    ClipboardOperation::Read => "Read host clipboard",
                    ClipboardOperation::Write => "Write host clipboard",
                },
            ),
            crate::host_dialog::DialogDetail::new(
                "MIME type",
                mime.unwrap_or("default selection"),
            ),
        ],
        note: Some(
            "This grants clipboard access through the narrow AGS bridge; it does not expose the raw Wayland compositor socket."
                .to_owned(),
        ),
        choices,
        width: 540,
        height: 380,
    };

    match crate::host_dialog::prompt_choice(&request, host_ui_socket) {
        crate::host_dialog::DialogOutcome::Choice(choice) if choice == "allow_once" => {
            ClipboardApprovalDecision::AllowOnce
        }
        crate::host_dialog::DialogOutcome::Choice(choice) if choice == "allow_window" => {
            ClipboardApprovalDecision::AllowWindow
        }
        crate::host_dialog::DialogOutcome::Choice(_)
        | crate::host_dialog::DialogOutcome::Cancelled => ClipboardApprovalDecision::Deny,
        crate::host_dialog::DialogOutcome::Unavailable => ClipboardApprovalDecision::Unavailable,
    }
}

fn format_duration(seconds: u64) -> String {
    if seconds.is_multiple_of(3600) {
        let hours = seconds / 3600;
        format!("{hours} hour{}", if hours == 1 { "" } else { "s" })
    } else if seconds.is_multiple_of(60) {
        let minutes = seconds / 60;
        format!("{minutes} minute{}", if minutes == 1 { "" } else { "s" })
    } else {
        format!("{seconds} second{}", if seconds == 1 { "" } else { "s" })
    }
}

fn handle_client(
    stream: UnixStream,
    mode: ClipboardMode,
    max_bytes: usize,
    access: Arc<dyn ClipboardAccessAuthorizer>,
    backend: Arc<dyn ClipboardBackend>,
) {
    if let Err(err) = handle_client_result(stream, mode, max_bytes, access, backend) {
        eprintln!("[ags clipboard] client error: {err}");
    }
}

fn handle_client_result(
    mut stream: UnixStream,
    mode: ClipboardMode,
    max_bytes: usize,
    access: Arc<dyn ClipboardAccessAuthorizer>,
    backend: Arc<dyn ClipboardBackend>,
) -> io::Result<()> {
    let mut line = String::new();
    BufReader::new(stream.try_clone()?).read_line(&mut line)?;
    let response = match serde_json::from_str::<Value>(&line) {
        Ok(request) => handle_request(&request, mode, max_bytes, access.as_ref(), backend.as_ref()),
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
    access: &dyn ClipboardAccessAuthorizer,
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
            if let Err(error) = access.authorize(ClipboardOperation::Read, mime) {
                return json!({ "ok": false, "error": error });
            }
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
            if let Err(error) = access.authorize(ClipboardOperation::Write, Some(mime)) {
                return json!({ "ok": false, "error": error });
            }
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
#[path = "clipboard_tests.rs"]
mod tests;
