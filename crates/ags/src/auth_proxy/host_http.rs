fn send_message(writer: &mut dyn Write, msg: &HostMessage) -> io::Result<()> {
    let json = serde_json::to_string(msg).map_err(io::Error::other)?;
    writer.write_all(json.as_bytes())?;
    writer.write_all(b"\n")?;
    writer.flush()
}

fn send_error(writer: &mut dyn Write, session_id: &str, message: String) -> io::Result<()> {
    send_message(
        writer,
        &HostMessage::Error {
            session_id: session_id.to_owned(),
            message,
        },
    )
}

fn send_session_complete(writer: &mut dyn Write, session_id: &str) -> io::Result<()> {
    send_message(
        writer,
        &HostMessage::SessionComplete {
            session_id: session_id.to_owned(),
        },
    )
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

        let mut addr: libc::sockaddr_in = std::mem::zeroed();
        #[cfg(any(
            target_os = "macos",
            target_os = "ios",
            target_os = "freebsd",
            target_os = "netbsd",
            target_os = "openbsd",
            target_os = "dragonfly",
        ))]
        {
            addr.sin_len = std::mem::size_of::<libc::sockaddr_in>() as u8;
        }
        addr.sin_family = libc::AF_INET as libc::sa_family_t;
        addr.sin_port = port.to_be();
        addr.sin_addr = libc::in_addr {
            s_addr: u32::from_ne_bytes([127, 0, 0, 1]),
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
fn read_http_request(stream: &mut dyn Read) -> Result<HttpRequest, Box<dyn std::error::Error>> {
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

    for (key, value) in headers {
        write!(stream, "{key}: {value}\r\n")?;
    }
    if !headers
        .iter()
        .any(|(k, _)| k.eq_ignore_ascii_case("content-length"))
    {
        write!(stream, "Content-Length: {}\r\n", body.len())?;
    }
    if !headers
        .iter()
        .any(|(k, _)| k.eq_ignore_ascii_case("connection"))
    {
        write!(stream, "Connection: close\r\n")?;
    }
    write!(stream, "\r\n")?;
    stream.write_all(body.as_bytes())?;
    stream.flush()
}

