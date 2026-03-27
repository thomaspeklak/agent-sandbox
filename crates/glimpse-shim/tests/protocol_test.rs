use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixListener;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use base64::Engine;
use serde_json::{Value, json};

fn shim_bin() -> String {
    env!("CARGO_BIN_EXE_glimpse-shim").to_owned()
}

/// Spawn a mock host-UI socket server that records all inbound requests.
///
/// When it receives an `open` request it sends back a `window.ready` event.
/// When it receives a `close` request it sends `window.closed` and stops.
/// Optionally injects extra events via the `inject_rx` channel.
fn mock_server(
    socket_path: &std::path::Path,
    inject_rx: mpsc::Receiver<Value>,
) -> thread::JoinHandle<Vec<Value>> {
    let listener = UnixListener::bind(socket_path).unwrap();
    thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        let read_half = stream.try_clone().unwrap();
        let mut reader = BufReader::new(read_half);
        let mut writer = stream;
        let mut requests: Vec<Value> = vec![];
        let window_id = "test_win_1";

        let mut line = String::new();

        // ── hello ────────────────────────────────────────────────────
        reader.read_line(&mut line).unwrap();
        let req: Value = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(req["method"], "hello");
        requests.push(req.clone());
        writeln!(
            writer,
            "{}",
            json!({
                "v": 1, "kind": "response", "id": req["id"], "ok": true,
                "result": {
                    "protocol_version": 1,
                    "server_name": "mock",
                    "server_version": "0.0.1",
                    "capabilities": { "prompt": false, "follow_cursor": false, "platform": "linux" }
                }
            })
        )
        .unwrap();
        writer.flush().unwrap();

        // ── open ─────────────────────────────────────────────────────
        line.clear();
        reader.read_line(&mut line).unwrap();
        let req: Value = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(req["method"], "open");
        requests.push(req.clone());
        writeln!(
            writer,
            "{}",
            json!({
                "v": 1, "kind": "response", "id": req["id"], "ok": true,
                "result": { "window_id": window_id }
            })
        )
        .unwrap();

        // Send window.ready event
        writeln!(
            writer,
            "{}",
            json!({
                "v": 1, "kind": "event", "event": "window.ready",
                "window_id": window_id,
                "data": {
                    "screen": { "width": 1920, "height": 1080 },
                    "screens": [],
                    "appearance": { "dark_mode": false },
                    "cursor": { "x": 100, "y": 200 },
                    "cursor_tip": null
                }
            })
        )
        .unwrap();
        writer.flush().unwrap();

        // ── subsequent requests ──────────────────────────────────────
        loop {
            // Inject events from test if any
            while let Ok(event) = inject_rx.try_recv() {
                writeln!(writer, "{}", event).unwrap();
                writer.flush().unwrap();
            }

            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) => break,
                Err(_) => break,
                Ok(_) => {}
            }
            let req: Value = match serde_json::from_str(line.trim()) {
                Ok(v) => v,
                Err(_) => continue,
            };
            requests.push(req.clone());

            // Ack the request
            writeln!(
                writer,
                "{}",
                json!({
                    "v": 1, "kind": "response", "id": req["id"], "ok": true,
                    "result": { "accepted": true }
                })
            )
            .unwrap();
            writer.flush().unwrap();

            if req["method"] == "close" {
                writeln!(
                    writer,
                    "{}",
                    json!({
                        "v": 1, "kind": "event", "event": "window.closed",
                        "window_id": window_id, "data": {}
                    })
                )
                .unwrap();
                writer.flush().unwrap();
                break;
            }
        }

        requests
    })
}

fn encode_html(html: &str) -> String {
    base64::engine::general_purpose::STANDARD.encode(html.as_bytes())
}

#[test]
fn open_update_close_round_trip() {
    let dir = tempfile::tempdir().unwrap();
    let sock = dir.path().join("host-ui.sock");
    let (inject_tx, inject_rx) = mpsc::channel::<Value>();

    let server = mock_server(&sock, inject_rx);

    let mut child = Command::new(shim_bin())
        .env("AGS_HOST_UI_SOCK", &sock)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .expect("failed to spawn glimpse-shim");

    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();

    // 1. Read synthetic ready from shim
    reader.read_line(&mut line).unwrap();
    let msg: Value = serde_json::from_str(line.trim()).unwrap();
    assert_eq!(msg["type"], "ready");
    // Synthetic ready has empty screen
    assert_eq!(msg["screen"], json!({}));

    // 2. Send HTML source to shim
    let html = "<html><body>test</body></html>";
    writeln!(
        stdin,
        "{}",
        json!({"type": "html", "html": encode_html(html)})
    )
    .unwrap();

    // 3. Read real ready from server
    line.clear();
    reader.read_line(&mut line).unwrap();
    let msg: Value = serde_json::from_str(line.trim()).unwrap();
    assert_eq!(msg["type"], "ready");
    assert_eq!(msg["screen"]["width"], 1920);

    // 4. Send eval update
    writeln!(
        stdin,
        "{}",
        json!({"type": "eval", "js": "console.log('hi')"})
    )
    .unwrap();

    // 5. Send a message event from server
    inject_tx
        .send(json!({
            "v": 1, "kind": "event", "event": "window.message",
            "window_id": "test_win_1",
            "data": { "payload": 42 }
        }))
        .unwrap();

    // Give the server a moment to process
    thread::sleep(Duration::from_millis(50));

    // 6. Send show update
    writeln!(stdin, "{}", json!({"type": "show", "title": "New Title"})).unwrap();

    // Small delay to let message event propagate
    thread::sleep(Duration::from_millis(50));

    // 7. Read message event from shim stdout
    line.clear();
    reader.read_line(&mut line).unwrap();
    let msg: Value = serde_json::from_str(line.trim()).unwrap();
    assert_eq!(msg["type"], "message");
    assert_eq!(msg["data"]["payload"], 42);

    // 8. Send close
    writeln!(stdin, "{}", json!({"type": "close"})).unwrap();

    // 9. Read closed from shim stdout
    line.clear();
    reader.read_line(&mut line).unwrap();
    let msg: Value = serde_json::from_str(line.trim()).unwrap();
    assert_eq!(msg["type"], "closed");

    // 10. Wait for child to exit
    let status = child.wait().unwrap();
    assert!(status.success() || status.code() == Some(0));

    // 11. Verify server received the right requests
    let requests = server.join().unwrap();

    // hello, open, update(eval), update(show), close
    assert_eq!(requests.len(), 5, "expected 5 requests, got: {requests:?}");
    assert_eq!(requests[0]["method"], "hello");
    assert_eq!(requests[1]["method"], "open");

    // Verify the open request has decoded HTML (not base64)
    let open_html = requests[1]["params"]["source"]["html"].as_str().unwrap();
    assert_eq!(open_html, html);

    assert_eq!(requests[2]["method"], "update");
    assert_eq!(requests[2]["params"]["patch"]["js"], "console.log('hi')");

    assert_eq!(requests[3]["method"], "update");
    assert_eq!(requests[3]["params"]["patch"]["show"]["title"], "New Title");

    assert_eq!(requests[4]["method"], "close");
}

#[test]
fn stdin_eof_sends_close() {
    let dir = tempfile::tempdir().unwrap();
    let sock = dir.path().join("host-ui.sock");
    let (_inject_tx, inject_rx) = mpsc::channel::<Value>();

    let server = mock_server(&sock, inject_rx);

    let mut child = Command::new(shim_bin())
        .env("AGS_HOST_UI_SOCK", &sock)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();

    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();

    // Read synthetic ready
    reader.read_line(&mut line).unwrap();

    // Send HTML source
    writeln!(
        stdin,
        "{}",
        json!({"type": "html", "html": encode_html("<html></html>")})
    )
    .unwrap();

    // Read real ready
    line.clear();
    reader.read_line(&mut line).unwrap();

    // Close stdin (EOF)
    drop(stdin);

    // Read closed event
    line.clear();
    reader.read_line(&mut line).unwrap();
    let msg: Value = serde_json::from_str(line.trim()).unwrap();
    assert_eq!(msg["type"], "closed");

    child.wait().unwrap();

    // Server should have received close request
    let requests = server.join().unwrap();
    assert_eq!(
        requests.last().unwrap()["method"],
        "close",
        "last request should be close"
    );
}

#[test]
fn url_source_opens_with_url() {
    let dir = tempfile::tempdir().unwrap();
    let sock = dir.path().join("host-ui.sock");
    let (_inject_tx, inject_rx) = mpsc::channel::<Value>();

    let server = mock_server(&sock, inject_rx);

    let mut child = Command::new(shim_bin())
        .env("AGS_HOST_UI_SOCK", &sock)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();

    let mut stdin = child.stdin.take().unwrap();
    let stdout = child.stdout.take().unwrap();
    let mut reader = BufReader::new(stdout);
    let mut line = String::new();

    // Read synthetic ready
    reader.read_line(&mut line).unwrap();

    // Send URL source
    writeln!(
        stdin,
        "{}",
        json!({"type": "url", "url": "https://example.com"})
    )
    .unwrap();

    // Read real ready
    line.clear();
    reader.read_line(&mut line).unwrap();

    // Close
    writeln!(stdin, "{}", json!({"type": "close"})).unwrap();

    line.clear();
    reader.read_line(&mut line).unwrap();
    let msg: Value = serde_json::from_str(line.trim()).unwrap();
    assert_eq!(msg["type"], "closed");

    child.wait().unwrap();

    let requests = server.join().unwrap();
    let open = &requests[1];
    assert_eq!(open["method"], "open");
    assert_eq!(open["params"]["source"]["kind"], "url");
    assert_eq!(open["params"]["source"]["url"], "https://example.com");
}
