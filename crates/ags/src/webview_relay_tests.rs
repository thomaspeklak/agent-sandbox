use super::{SOCKET_NAME, UPSTREAM_SOCKET_NAME, start};
use base64::Engine;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
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
    let status = status_line
        .split_whitespace()
        .nth(1)
        .unwrap()
        .parse::<u16>()
        .unwrap();
    let body = buf[header_end + 4..].to_vec();
    (status, header_text.into_owned(), body)
}

fn register_app(runtime_dir: &std::path::Path, port: u16, base_path: &str) -> serde_json::Value {
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

fn spawn_echo_app(count: usize) -> (u16, mpsc::Receiver<String>, JoinHandle<()>) {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let (tx, rx) = mpsc::channel();
    let handle = thread::spawn(move || {
        for _ in 0..count {
            let (mut stream, _) = listener.accept().unwrap();
            let request = super::read_http_request(&mut stream).unwrap();
            tx.send(request.target.clone()).unwrap();
            let body = b"ok";
            write!(
                    stream,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\nContent-Type: text/plain\r\n\r\n",
                    body.len()
                ).unwrap();
            stream.write_all(body).unwrap();
        }
    });
    (port, rx, handle)
}

fn start_shim(runtime_dir: &std::path::Path) -> Child {
    let upstream_socket = runtime_dir.join(UPSTREAM_SOCKET_NAME);
    let child = Command::new("python3")
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
    child
}

fn collect_targets(rx: &mpsc::Receiver<String>, count: usize) -> Vec<String> {
    (0..count)
        .map(|_| rx.recv_timeout(Duration::from_secs(2)).unwrap())
        .collect()
}

#[test]
fn interview_style_root_absolute_requests_work_unchanged() {
    let (app_port, app_rx, app_thread) = spawn_echo_app(4);

    let dir = tempfile::tempdir().unwrap();
    let runtime_dir = dir.path().join("relay-runtime");
    fs::create_dir_all(&runtime_dir).unwrap();
    let mut shim = start_shim(&runtime_dir);

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

    assert_eq!(
        collect_targets(&app_rx, 4),
        [
            "/",
            "/styles.css",
            "/submit",
            "/media?path=image.png&session=abc123"
        ],
    );

    drop(guard);
    let _ = shim.kill();
    let _ = shim.wait();
    app_thread.join().unwrap();
}

#[test]
fn base_path_registrations_keep_root_absolute_assets_working() {
    let (app_port, app_rx, app_thread) = spawn_echo_app(3);

    let dir = tempfile::tempdir().unwrap();
    let runtime_dir = dir.path().join("relay-runtime");
    fs::create_dir_all(&runtime_dir).unwrap();
    let mut shim = start_shim(&runtime_dir);

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

    assert_eq!(
        collect_targets(&app_rx, 3),
        [
            "/app?session=abc123",
            "/app/styles.css",
            "/app/media?path=a.png&session=abc123"
        ],
    );

    drop(guard);
    let _ = shim.kill();
    let _ = shim.wait();
    app_thread.join().unwrap();
}
