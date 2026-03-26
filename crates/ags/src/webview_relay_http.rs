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
    accept_loop(listener.incoming(), shutdown, "http", |stream| {
        let runtime_dir = runtime_dir.to_owned();
        let registration = registration.clone();
        thread::spawn(move || {
            if let Err(err) = handle_http_client(stream, &runtime_dir, &registration) {
                eprintln!("[ags webview-relay] client error: {err}");
            }
        });
    });
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
    write_http_response(&mut stream, status, &reason, &headers, &body)
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

fn write_http_response<N: AsRef<str>, V: AsRef<str>>(
    stream: &mut TcpStream,
    status: u16,
    reason: &str,
    headers: &[(N, V)],
    body: &[u8],
) -> io::Result<()> {
    write!(stream, "HTTP/1.1 {status} {reason}\r\n")?;
    write!(stream, "Content-Length: {}\r\n", body.len())?;
    write!(stream, "Connection: close\r\n")?;
    for (name, value) in headers {
        let name = name.as_ref();
        let value = value.as_ref();
        if is_hop_by_hop_header(name) || name.eq_ignore_ascii_case("content-length") {
            continue;
        }
        write!(stream, "{name}: {value}\r\n")?;
    }
    write!(stream, "\r\n")?;
    stream.write_all(body)
}

const HOP_BY_HOP_HEADERS: &[&str] = &[
    "connection",
    "keep-alive",
    "proxy-authenticate",
    "proxy-authorization",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
];

fn is_hop_by_hop_header(name: &str) -> bool {
    HOP_BY_HOP_HEADERS
        .iter()
        .any(|h| name.eq_ignore_ascii_case(h))
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
    writeln!(stream, "{line}")?;
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
