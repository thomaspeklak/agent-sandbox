use std::io::{self, BufRead, BufReader};
use std::process;
use std::thread;

use base64::Engine;
use serde_json::{Value, json};

use crate::emit_stdout;
use crate::socket::{self, SocketReader, SocketWriter};

/// Read the first `html`/`url` source from stdin, open the window on the
/// socket, then run the bidirectional bridge until the window closes.
pub fn run(mut conn: socket::SocketConn, options: Value) -> Result<(), Box<dyn std::error::Error>> {
    // Single BufReader for the lifetime of stdin — avoids data loss from
    // dropping a partially-buffered reader between read_first_source and
    // the bridge loop.
    let mut stdin_reader = BufReader::new(io::stdin());

    let source = read_first_source(&mut stdin_reader)?;
    let window_id = conn.open(source, options)?;

    let (mut sock_reader, sock_writer) = conn.into_parts();

    thread::spawn(move || {
        let _ = stdin_to_socket(stdin_reader, sock_writer, &window_id);
    });

    socket_to_stdout(&mut sock_reader);
    process::exit(0);
}

// ── socket → stdout ──────────────────────────────────────────────────────

fn socket_to_stdout(reader: &mut SocketReader) {
    while let Ok(msg) = socket::read_message(reader) {
        if msg["kind"].as_str() != Some("event") {
            continue; // ignore response acks
        }

        let data = &msg["data"];
        match msg["event"].as_str().unwrap_or("") {
            "window.ready" => {
                emit_stdout(&json!({
                    "type": "ready",
                    "screen": data.get("screen").unwrap_or(&json!({})),
                    "screens": data.get("screens").unwrap_or(&json!([])),
                    "appearance": data.get("appearance").unwrap_or(&json!({})),
                    "cursor": data.get("cursor").unwrap_or(&json!({"x":0,"y":0})),
                    "cursorTip": data.get("cursor_tip").unwrap_or(&Value::Null)
                }));
            }
            "window.message" => {
                emit_stdout(&json!({ "type": "message", "data": data }));
            }
            "window.closed" => {
                emit_stdout(&json!({ "type": "closed" }));
                return;
            }
            _ => {}
        }
    }
}

// ── stdin → socket ───────────────────────────────────────────────────────

fn send_update(writer: &mut SocketWriter, window_id: &str, patch: Value) -> io::Result<()> {
    writer.send_request("update", json!({ "window_id": window_id, "patch": patch }))
}

fn stdin_to_socket(
    reader: BufReader<io::Stdin>,
    mut writer: SocketWriter,
    window_id: &str,
) -> io::Result<()> {
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let msg: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };

        match msg.get("type").and_then(Value::as_str) {
            Some("html") => {
                let raw = decode_b64_html(msg["html"].as_str().unwrap_or(""));
                send_update(&mut writer, window_id, json!({ "html": raw }))?;
            }
            Some("url") => {
                send_update(
                    &mut writer,
                    window_id,
                    json!({ "url": msg["url"].as_str().unwrap_or("") }),
                )?;
            }
            Some("eval") => {
                send_update(
                    &mut writer,
                    window_id,
                    json!({ "js": msg["js"].as_str().unwrap_or("") }),
                )?;
            }
            Some("show") => {
                let mut show = json!({});
                if let Some(title) = msg.get("title") {
                    show["title"] = title.clone();
                }
                send_update(&mut writer, window_id, json!({ "show": show }))?;
            }
            Some("follow-cursor") => {
                send_update(
                    &mut writer,
                    window_id,
                    json!({
                        "follow_cursor": {
                            "enabled": msg.get("enabled").and_then(Value::as_bool).unwrap_or(true),
                            "anchor": msg.get("anchor").cloned().unwrap_or(Value::Null),
                            "mode": msg.get("mode").cloned().unwrap_or(Value::Null)
                        }
                    }),
                )?;
            }
            Some("close") => {
                writer.send_request("close", json!({ "window_id": window_id }))?;
                return Ok(());
            }
            Some("get-info") => {} // no socket equivalent; real info arrives via ready events
            Some("file") => {
                eprintln!("[glimpse-shim] file transport not supported over socket");
            }
            _ => {}
        }
    }

    // stdin EOF → close the window
    if let Err(e) = writer.send_request("close", json!({ "window_id": window_id })) {
        eprintln!("[glimpse-shim] failed to send close on stdin EOF: {e}");
    }
    Ok(())
}

// ── helpers ──────────────────────────────────────────────────────────────

fn read_first_source(
    reader: &mut BufReader<io::Stdin>,
) -> Result<Value, Box<dyn std::error::Error>> {
    let mut line = String::new();
    loop {
        line.clear();
        let n = reader.read_line(&mut line)?;
        if n == 0 {
            return Err("stdin closed before source was received".into());
        }
        if line.trim().is_empty() {
            continue;
        }
        let msg: Value = serde_json::from_str(&line)?;
        match msg.get("type").and_then(Value::as_str) {
            Some("html") => {
                let raw = decode_b64_html(msg["html"].as_str().unwrap_or(""));
                return Ok(json!({ "kind": "html", "html": raw }));
            }
            Some("url") => {
                let url = msg["url"].as_str().unwrap_or("").to_owned();
                return Ok(json!({ "kind": "url", "url": url }));
            }
            _ => continue,
        }
    }
}

fn decode_b64_html(encoded: &str) -> String {
    base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .unwrap_or_default()
}
