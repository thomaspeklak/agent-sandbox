pub fn start(
    runtime_dir: &Path,
    auto_allow_domains: Vec<String>,
    webview_relay_socket_path: Option<PathBuf>,
    host_ui_socket_path: Option<PathBuf>,
) -> Result<AuthProxyGuard, AuthProxyError> {
    start_with_host(
        runtime_dir,
        Arc::new(OsAuthProxyHost::new(
            auto_allow_domains,
            webview_relay_socket_path,
            host_ui_socket_path,
        )),
    )
}

/// Start the auth proxy with a custom host implementation (for testing).
pub fn start_with_host(
    runtime_dir: &Path,
    host: Arc<dyn AuthProxyHost + Send + Sync>,
) -> Result<AuthProxyGuard, AuthProxyError> {
    crate::util::ensure_private_dir(runtime_dir).map_err(AuthProxyError::RuntimeDirCreate)?;

    let sock_path = runtime_dir.join(SOCKET_NAME);
    // Remove stale socket if present
    let _ = fs::remove_file(&sock_path);

    let listener = UnixListener::bind(&sock_path).map_err(AuthProxyError::SocketBind)?;

    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_clone = shutdown.clone();
    let runtime_dir_owned = runtime_dir.to_owned();

    let thread = thread::spawn(move || {
        accept_loop(&listener, &shutdown_clone, &host);
    });

    Ok(AuthProxyGuard {
        runtime_dir: runtime_dir_owned,
        shutdown,
        thread: Some(thread),
    })
}

fn accept_loop(
    listener: &UnixListener,
    shutdown: &AtomicBool,
    host: &Arc<dyn AuthProxyHost + Send + Sync>,
) {
    for stream in listener.incoming() {
        if shutdown.load(Ordering::Relaxed) {
            break;
        }
        match stream {
            Ok(stream) => {
                let host = Arc::clone(host);
                thread::spawn(move || {
                    if let Err(e) = handle_session(stream, host.as_ref()) {
                        eprintln!("[ags auth-proxy] session error: {e}");
                    }
                });
            }
            Err(e) => {
                if shutdown.load(Ordering::Relaxed) {
                    break;
                }
                eprintln!("[ags auth-proxy] accept error: {e}");
            }
        }
    }
}

fn handle_session(
    stream: UnixStream,
    host: &dyn AuthProxyHost,
) -> Result<(), Box<dyn std::error::Error>> {
    stream.set_read_timeout(Some(SESSION_TIMEOUT)).ok();

    let reader_stream = stream.try_clone()?;
    let mut reader = BufReader::new(reader_stream);
    let mut writer = stream;

    let mut line = String::new();
    reader.read_line(&mut line)?;

    if line.is_empty() {
        return Ok(()); // shutdown wake-up connection
    }

    let msg: ShimMessage = serde_json::from_str(line.trim())?;

    match msg {
        ShimMessage::OpenUrl {
            session_id,
            url,
            callback_port,
        } => handle_open_url(
            &session_id,
            &url,
            callback_port,
            &mut reader,
            &mut writer,
            host,
        ),
        _ => {
            send_error(
                &mut writer,
                "unknown",
                "expected open_url as first message".to_owned(),
            )?;
            Ok(())
        }
    }
}

fn handle_open_url(
    session_id: &str,
    url: &str,
    callback_port: Option<u16>,
    reader: &mut BufReader<UnixStream>,
    writer: &mut UnixStream,
    host: &dyn AuthProxyHost,
) -> Result<(), Box<dyn std::error::Error>> {
    let has_callback = callback_port.is_some();
    let can_proxy = host.can_proxy(url);

    // Prompt user
    let decision = host.prompt_user(url, has_callback, can_proxy);
    let allowed = !matches!(decision, OpenDecision::Cancel);

    send_message(
        writer,
        &HostMessage::PromptResult {
            session_id: session_id.to_owned(),
            allowed,
        },
    )?;

    if !allowed {
        send_session_complete(writer, session_id)?;
        return Ok(());
    }

    let target_url = match decision {
        OpenDecision::OpenOriginal | OpenDecision::Cancel => url.to_owned(),
        OpenDecision::Proxy => {
            if !can_proxy {
                send_error(
                    writer,
                    session_id,
                    "proxy option is unavailable for this URL".to_owned(),
                )?;
                return Ok(());
            }
            match host.resolve_proxy_url(url) {
                Ok(resolved) => resolved,
                Err(e) => {
                    send_error(
                        writer,
                        session_id,
                        format!("failed to resolve proxied URL: {e}"),
                    )?;
                    return Ok(());
                }
            }
        }
    };

    if let Some(port) = callback_port {
        handle_callback_flow(session_id, &target_url, port, reader, writer, host)?;
    } else {
        let open_result = if matches!(decision, OpenDecision::Proxy) {
            host.open_proxy_target(&target_url)
        } else {
            host.open_browser(&target_url)
        };
        if let Err(e) = open_result {
            send_error(
                writer,
                session_id,
                format!("failed to open target URL: {e}"),
            )?;
            return Ok(());
        }
        send_session_complete(writer, session_id)?;
    }

    Ok(())
}

fn handle_callback_flow(
    session_id: &str,
    url: &str,
    callback_port: u16,
    reader: &mut BufReader<UnixStream>,
    writer: &mut UnixStream,
    host: &dyn AuthProxyHost,
) -> Result<(), Box<dyn std::error::Error>> {
    // Bind the callback listener on the host loopback BEFORE opening the browser,
    // so the callback port is ready when the browser redirects.
    // Use SO_REUSEADDR so rapid retry (deny → allow, or successive OAuth flows)
    // doesn't fail with EADDRINUSE from TIME_WAIT sockets.
    let callback_listener = bind_callback_listener(callback_port)?;

    // Open the browser
    if let Err(e) = host.open_browser(url) {
        drop(callback_listener);
        send_error(writer, session_id, format!("failed to open browser: {e}"))?;
        return Ok(());
    }

    // Wait for the callback HTTP request from the browser, then drop the
    // listener immediately so the port is released.
    let (mut tcp_stream, _addr) = callback_listener.accept()?;
    drop(callback_listener);
    tcp_stream.set_read_timeout(Some(SESSION_TIMEOUT)).ok();

    // Read the raw HTTP request
    let (method, path, headers, body) = read_http_request(&mut tcp_stream)?;

    let request_id = format!("{session_id}-cb");

    // Relay the callback to the container shim
    send_message(
        writer,
        &HostMessage::CallbackRequest {
            session_id: session_id.to_owned(),
            request_id,
            method,
            path,
            headers,
            body,
        },
    )?;

    // Shorten the read timeout for the callback relay phase
    reader
        .get_ref()
        .set_read_timeout(Some(CALLBACK_RELAY_TIMEOUT))
        .ok();

    let mut line = String::new();
    reader.read_line(&mut line)?;
    let response: ShimMessage = serde_json::from_str(line.trim())?;

    // Send the HTTP response back to the browser
    match response {
        ShimMessage::CallbackResponse {
            status,
            headers,
            body,
            ..
        } => {
            write_http_response(&mut tcp_stream, status, &headers, &body)?;
        }
        _ => {
            write_http_response(
                &mut tcp_stream,
                502,
                &[("Content-Type".to_owned(), "text/plain".to_owned())],
                "auth proxy: unexpected response from container",
            )?;
        }
    }

    send_session_complete(writer, session_id)?;

    Ok(())
}

// --- JSON messaging ---
