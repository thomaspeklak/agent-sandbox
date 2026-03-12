use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::os::unix::net::UnixStream;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use ags::auth_proxy::host::{self, AuthProxyGuard, AuthProxyHost};
use ags::auth_proxy::protocol::{HostMessage, ShimMessage};

// --- Test helpers ---

fn send_shim_msg(stream: &mut UnixStream, msg: &ShimMessage) {
    let json = serde_json::to_string(msg).unwrap();
    stream.write_all(json.as_bytes()).unwrap();
    stream.write_all(b"\n").unwrap();
    stream.flush().unwrap();
}

fn recv_host_msg(reader: &mut BufReader<UnixStream>) -> HostMessage {
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    serde_json::from_str(line.trim()).unwrap()
}

/// Test host that auto-allows all prompts and records open_browser calls.
struct AllowHost;

impl AuthProxyHost for AllowHost {
    fn prompt_user(&self, _url: &str, _has_callback: bool) -> bool {
        true
    }

    fn open_browser(&self, _url: &str) -> Result<(), String> {
        Ok(())
    }
}

/// Test host that auto-denies all prompts.
struct DenyHost;

impl AuthProxyHost for DenyHost {
    fn prompt_user(&self, _url: &str, _has_callback: bool) -> bool {
        false
    }

    fn open_browser(&self, _url: &str) -> Result<(), String> {
        Ok(())
    }
}

/// Test host that allows prompts but fails to open the browser.
struct BrowserFailHost;

impl AuthProxyHost for BrowserFailHost {
    fn prompt_user(&self, _url: &str, _has_callback: bool) -> bool {
        true
    }

    fn open_browser(&self, _url: &str) -> Result<(), String> {
        Err("browser not available".to_owned())
    }
}

fn start_proxy(host: Arc<dyn AuthProxyHost + Send + Sync>) -> AuthProxyGuard {
    let dir = tempfile::tempdir().unwrap();
    let runtime_dir = dir.keep();
    host::start_with_host(&runtime_dir, host).unwrap()
}

fn connect(guard: &AuthProxyGuard) -> (UnixStream, BufReader<UnixStream>) {
    let sock_path = guard.runtime_dir.join(host::SOCKET_NAME);
    // Brief retry loop for socket readiness
    let stream = {
        let mut attempts = 0;
        loop {
            match UnixStream::connect(&sock_path) {
                Ok(s) => break s,
                Err(_) if attempts < 5 => {
                    attempts += 1;
                    thread::sleep(Duration::from_millis(50));
                }
                Err(e) => panic!("failed to connect to auth proxy socket: {e}"),
            }
        }
    };
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    let reader = BufReader::new(stream.try_clone().unwrap());
    (stream, reader)
}

// --- Protocol serialization tests ---

#[test]
fn shim_message_open_url_roundtrips() {
    let msg = ShimMessage::OpenUrl {
        session_id: "s1".into(),
        url: "https://example.com/auth".into(),
        callback_port: Some(8080),
    };
    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains("\"type\":\"open_url\""));
    let parsed: ShimMessage = serde_json::from_str(&json).unwrap();
    match parsed {
        ShimMessage::OpenUrl {
            session_id,
            url,
            callback_port,
        } => {
            assert_eq!(session_id, "s1");
            assert_eq!(url, "https://example.com/auth");
            assert_eq!(callback_port, Some(8080));
        }
        _ => panic!("wrong variant"),
    }
}

#[test]
fn host_message_prompt_result_roundtrips() {
    let msg = HostMessage::PromptResult {
        session_id: "s1".into(),
        allowed: true,
    };
    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains("\"type\":\"prompt_result\""));
    let parsed: HostMessage = serde_json::from_str(&json).unwrap();
    match parsed {
        HostMessage::PromptResult {
            session_id,
            allowed,
        } => {
            assert_eq!(session_id, "s1");
            assert!(allowed);
        }
        _ => panic!("wrong variant"),
    }
}

#[test]
fn host_message_callback_request_roundtrips() {
    let msg = HostMessage::CallbackRequest {
        session_id: "s1".into(),
        request_id: "r1".into(),
        method: "GET".into(),
        path: "/callback?code=abc123".into(),
        headers: vec![("Host".into(), "localhost:8080".into())],
        body: String::new(),
    };
    let json = serde_json::to_string(&msg).unwrap();
    let parsed: HostMessage = serde_json::from_str(&json).unwrap();
    match parsed {
        HostMessage::CallbackRequest {
            method, path, headers, ..
        } => {
            assert_eq!(method, "GET");
            assert_eq!(path, "/callback?code=abc123");
            assert_eq!(headers.len(), 1);
        }
        _ => panic!("wrong variant"),
    }
}

// --- Simple URL open tests ---

#[test]
fn simple_url_open_allowed() {
    let guard = start_proxy(Arc::new(AllowHost));
    let (mut stream, mut reader) = connect(&guard);

    send_shim_msg(
        &mut stream,
        &ShimMessage::OpenUrl {
            session_id: "s1".into(),
            url: "https://example.com".into(),
            callback_port: None,
        },
    );

    let msg = recv_host_msg(&mut reader);
    match msg {
        HostMessage::PromptResult { allowed, .. } => assert!(allowed),
        other => panic!("expected PromptResult, got: {other:?}"),
    }

    let msg = recv_host_msg(&mut reader);
    match msg {
        HostMessage::SessionComplete { session_id } => assert_eq!(session_id, "s1"),
        other => panic!("expected SessionComplete, got: {other:?}"),
    }
}

#[test]
fn simple_url_open_denied() {
    let guard = start_proxy(Arc::new(DenyHost));
    let (mut stream, mut reader) = connect(&guard);

    send_shim_msg(
        &mut stream,
        &ShimMessage::OpenUrl {
            session_id: "s1".into(),
            url: "https://example.com".into(),
            callback_port: None,
        },
    );

    let msg = recv_host_msg(&mut reader);
    match msg {
        HostMessage::PromptResult { allowed, .. } => assert!(!allowed),
        other => panic!("expected PromptResult, got: {other:?}"),
    }

    let msg = recv_host_msg(&mut reader);
    match msg {
        HostMessage::SessionComplete { .. } => {}
        other => panic!("expected SessionComplete, got: {other:?}"),
    }
}

#[test]
fn browser_failure_sends_error() {
    let guard = start_proxy(Arc::new(BrowserFailHost));
    let (mut stream, mut reader) = connect(&guard);

    send_shim_msg(
        &mut stream,
        &ShimMessage::OpenUrl {
            session_id: "s1".into(),
            url: "https://example.com".into(),
            callback_port: None,
        },
    );

    // Prompt allowed
    let msg = recv_host_msg(&mut reader);
    match msg {
        HostMessage::PromptResult { allowed, .. } => assert!(allowed),
        other => panic!("expected PromptResult, got: {other:?}"),
    }

    // But browser open fails
    let msg = recv_host_msg(&mut reader);
    match msg {
        HostMessage::Error { message, .. } => {
            assert!(
                message.contains("browser"),
                "error should mention browser: {message}"
            );
        }
        other => panic!("expected Error, got: {other:?}"),
    }
}

// --- Callback flow tests ---

#[test]
fn callback_flow_end_to_end() {
    let guard = start_proxy(Arc::new(AllowHost));
    let (mut stream, mut reader) = connect(&guard);

    // Find a free port for the callback
    let cb_listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let cb_port = cb_listener.local_addr().unwrap().port();
    drop(cb_listener); // Free the port for the proxy to bind

    send_shim_msg(
        &mut stream,
        &ShimMessage::OpenUrl {
            session_id: "s-cb".into(),
            url: format!(
                "https://provider.example/auth?redirect_uri=http://localhost:{cb_port}/callback"
            ),
            callback_port: Some(cb_port),
        },
    );

    // Should get PromptResult(allowed)
    let msg = recv_host_msg(&mut reader);
    match msg {
        HostMessage::PromptResult { allowed, .. } => assert!(allowed),
        other => panic!("expected PromptResult, got: {other:?}"),
    }

    // The host proxy is now listening on cb_port for a callback.
    // Simulate the browser redirect by making an HTTP request to cb_port.
    let browser_thread = thread::spawn(move || {
        // Small delay to let the proxy's TCP listener start
        thread::sleep(Duration::from_millis(100));
        let mut tcp = std::net::TcpStream::connect(format!("127.0.0.1:{cb_port}")).unwrap();
        tcp.set_read_timeout(Some(Duration::from_secs(5))).ok();
        tcp.write_all(
            b"GET /callback?code=test_auth_code HTTP/1.1\r\nHost: localhost\r\n\r\n",
        )
        .unwrap();

        // Read HTTP response
        let mut response = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            match tcp.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => response.extend_from_slice(&buf[..n]),
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(ref e) if e.kind() == std::io::ErrorKind::ConnectionReset => break,
                Err(e) => panic!("read error: {e}"),
            }
            // Check if we got a complete response
            if response.windows(4).any(|w| w == b"\r\n\r\n") {
                break;
            }
        }
        String::from_utf8_lossy(&response).to_string()
    });

    // Should receive CallbackRequest from the proxy
    let msg = recv_host_msg(&mut reader);
    let (session_id, request_id) = match msg {
        HostMessage::CallbackRequest {
            session_id,
            request_id,
            method,
            path,
            ..
        } => {
            assert_eq!(method, "GET");
            assert!(path.contains("code=test_auth_code"), "path: {path}");
            (session_id, request_id)
        }
        other => panic!("expected CallbackRequest, got: {other:?}"),
    };

    // Send CallbackResponse (simulating the shim replaying to the container server)
    send_shim_msg(
        &mut stream,
        &ShimMessage::CallbackResponse {
            session_id: session_id.clone(),
            request_id,
            status: 200,
            headers: vec![("Content-Type".into(), "text/html".into())],
            body: "<html>Auth complete</html>".into(),
        },
    );

    // Should get SessionComplete
    let msg = recv_host_msg(&mut reader);
    match msg {
        HostMessage::SessionComplete { session_id: sid } => {
            assert_eq!(sid, "s-cb");
        }
        other => panic!("expected SessionComplete, got: {other:?}"),
    }

    // Browser thread should have received the HTTP response
    let browser_response = browser_thread.join().unwrap();
    assert!(
        browser_response.contains("200"),
        "browser should get 200 response: {browser_response}"
    );
    assert!(
        browser_response.contains("Auth complete"),
        "browser should get the body: {browser_response}"
    );
}

// --- Multiple concurrent sessions ---

#[test]
fn multiple_concurrent_sessions() {
    let guard = start_proxy(Arc::new(AllowHost));

    let handles: Vec<_> = (0..3)
        .map(|i| {
            let sock_path = guard.runtime_dir.join(host::SOCKET_NAME);
            thread::spawn(move || {
                let stream = UnixStream::connect(&sock_path).unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .ok();
                let mut writer = stream.try_clone().unwrap();
                let mut reader = BufReader::new(stream);

                let session_id = format!("session-{i}");
                let msg = ShimMessage::OpenUrl {
                    session_id: session_id.clone(),
                    url: format!("https://example.com/{i}"),
                    callback_port: None,
                };
                let json = serde_json::to_string(&msg).unwrap();
                writer.write_all(json.as_bytes()).unwrap();
                writer.write_all(b"\n").unwrap();
                writer.flush().unwrap();

                // Read prompt result
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                let msg: HostMessage = serde_json::from_str(line.trim()).unwrap();
                match msg {
                    HostMessage::PromptResult { allowed, .. } => assert!(allowed),
                    other => panic!("session {i}: expected PromptResult, got: {other:?}"),
                }

                // Read session complete
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                let msg: HostMessage = serde_json::from_str(line.trim()).unwrap();
                match msg {
                    HostMessage::SessionComplete { session_id: sid } => {
                        assert_eq!(sid, session_id);
                    }
                    other => panic!("session {i}: expected SessionComplete, got: {other:?}"),
                }
            })
        })
        .collect();

    for h in handles {
        h.join().unwrap();
    }
}

// --- Guard cleanup ---

#[test]
fn guard_drop_cleans_up_runtime_dir() {
    let dir = tempfile::tempdir().unwrap();
    let runtime_dir = dir.keep();

    {
        let _guard = host::start_with_host(&runtime_dir, Arc::new(AllowHost)).unwrap();
        assert!(runtime_dir.join(host::SOCKET_NAME).exists());
    }
    // Guard dropped — runtime dir should be removed
    assert!(
        !runtime_dir.exists(),
        "runtime dir should be cleaned up on drop"
    );
}

// --- Socket absent test ---

#[test]
fn connection_fails_when_socket_absent() {
    let dir = tempfile::tempdir().unwrap();
    let sock_path = dir.path().join("nonexistent.sock");
    let result = UnixStream::connect(&sock_path);
    assert!(result.is_err(), "should fail when socket doesn't exist");
}

// --- Container path constants ---

#[test]
fn container_paths_are_stable() {
    assert_eq!(
        AuthProxyGuard::container_runtime_dir(),
        "/run/ags-auth-proxy"
    );
    assert_eq!(
        AuthProxyGuard::container_socket_path(),
        "/run/ags-auth-proxy/auth-proxy.sock"
    );
}

use std::io::Read;
