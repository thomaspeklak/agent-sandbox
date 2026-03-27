use std::io::{self, BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;

use serde_json::{Value, json};

/// Connection to the host-UI socket, used during the handshake phase.
///
/// After handshake, call [`into_parts`] to split into independent reader/writer
/// halves for the bridge threads.
pub struct SocketConn {
    reader: BufReader<UnixStream>,
    writer: UnixStream,
    next_id: u64,
}

/// Write-half of the socket, usable from the stdin-to-socket bridge thread.
pub struct SocketWriter {
    writer: UnixStream,
    next_id: u64,
}

/// Read-half of the socket.
pub type SocketReader = BufReader<UnixStream>;

impl SocketConn {
    pub fn connect(path: &str) -> io::Result<Self> {
        let stream = UnixStream::connect(path)?;
        let read_clone = stream.try_clone()?;
        Ok(Self {
            reader: BufReader::new(read_clone),
            writer: stream,
            next_id: 1,
        })
    }

    pub fn hello(&mut self) -> io::Result<Value> {
        self.request(
            "hello",
            json!({
                "client_name": "glimpse-shim",
                "client_version": env!("CARGO_PKG_VERSION"),
                "protocol_min": 1,
                "protocol_max": 1
            }),
        )
    }

    pub fn open(&mut self, source: Value, options: Value) -> io::Result<String> {
        let result = self.request("open", json!({ "source": source, "options": options }))?;
        result["window_id"]
            .as_str()
            .map(String::from)
            .ok_or_else(|| io::Error::other("missing window_id in open response"))
    }

    /// Split into independent reader/writer halves for the bridge phase.
    pub fn into_parts(self) -> (SocketReader, SocketWriter) {
        (
            self.reader,
            SocketWriter {
                writer: self.writer,
                next_id: self.next_id,
            },
        )
    }

    /// Send a request and block until the matching response arrives.
    fn request(&mut self, method: &str, params: Value) -> io::Result<Value> {
        let id = self.send_request(method, params)?;
        loop {
            let msg = read_message(&mut self.reader)?;
            if msg["kind"].as_str() == Some("response") && msg["id"].as_str() == Some(id.as_str()) {
                return if msg["ok"].as_bool() == Some(true) {
                    Ok(msg["result"].clone())
                } else {
                    let err = msg["error"]["message"].as_str().unwrap_or("unknown error");
                    Err(io::Error::other(err.to_owned()))
                };
            }
            // skip stray events that arrive before the response
        }
    }

    fn send_request(&mut self, method: &str, params: Value) -> io::Result<String> {
        let id = format!("req_{}", self.next_id);
        self.next_id += 1;
        write_request(&mut self.writer, &id, method, params)?;
        Ok(id)
    }
}

impl SocketWriter {
    pub fn send_request(&mut self, method: &str, params: Value) -> io::Result<()> {
        let id = format!("req_{}", self.next_id);
        self.next_id += 1;
        write_request(&mut self.writer, &id, method, params)
    }
}

pub fn read_message(reader: &mut SocketReader) -> io::Result<Value> {
    let mut line = String::new();
    let n = reader.read_line(&mut line)?;
    if n == 0 {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "socket closed",
        ));
    }
    serde_json::from_str(line.trim()).map_err(io::Error::other)
}

fn write_request(w: &mut UnixStream, id: &str, method: &str, params: Value) -> io::Result<()> {
    let req = json!({
        "v": 1,
        "kind": "request",
        "id": id,
        "method": method,
        "params": params
    });
    let line = serde_json::to_string(&req).map_err(io::Error::other)?;
    w.write_all(line.as_bytes())?;
    w.write_all(b"\n")?;
    w.flush()
}
