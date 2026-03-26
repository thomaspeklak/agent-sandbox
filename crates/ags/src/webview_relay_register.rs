impl RegisterResponse {
    fn success(host_port: u16, base_path: String, url: String) -> Self {
        Self {
            ok: true,
            error: None,
            host_port: Some(host_port),
            base_path: Some(base_path),
            url: Some(url),
        }
    }

    fn error(msg: impl Into<String>) -> Self {
        Self {
            ok: false,
            error: Some(msg.into()),
            host_port: None,
            base_path: None,
            url: None,
        }
    }
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
    let response = process_register_request(line.trim(), runtime_dir, listeners);
    let body = serde_json::to_string(&response)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    writeln!(stream, "{body}")?;
    stream.flush()
}

fn process_register_request(
    raw: &str,
    runtime_dir: &Path,
    listeners: &Arc<Mutex<Vec<AppListenerGuard>>>,
) -> RegisterResponse {
    let (port, base_path) = match serde_json::from_str::<RegisterRequest>(raw) {
        Ok(RegisterRequest::Register { port, base_path }) => (port, base_path),
        Err(err) => return RegisterResponse::error(format!("invalid request: {err}")),
    };
    if port == 0 {
        return RegisterResponse::error("invalid port");
    }
    let registration = Registration {
        sandbox_port: port,
        base_path: normalize_base_path(&base_path),
    };
    let listener = match start_app_listener(runtime_dir, registration.clone()) {
        Ok(l) => l,
        Err(err) => {
            return RegisterResponse::error(format!("failed to allocate host listener: {err}"));
        }
    };
    let host_port = listener.port;
    if let Ok(mut guards) = listeners.lock() {
        guards.push(listener);
    }
    let url = format!("http://127.0.0.1:{host_port}{}", registration.base_path);
    RegisterResponse::success(host_port, registration.base_path, url)
}

fn normalize_base_path(value: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed == "/" {
        return "/".to_owned();
    }
    let with_slash = if trimmed.starts_with('/') {
        trimmed
    } else {
        return format!("/{}", trimmed.trim_end_matches('/'));
    };
    let stripped = with_slash.trim_end_matches('/');
    if stripped.is_empty() {
        "/".to_owned()
    } else {
        stripped.to_owned()
    }
}
