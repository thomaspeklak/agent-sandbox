fn is_auto_allowed(url: &str, domains: &[String]) -> bool {
    let Some(host) = parsed_http_host(url) else {
        return false;
    };

    domains
        .iter()
        .filter_map(|domain| normalize_allowed_domain(domain))
        .any(|domain| host_matches_allowed_domain(&host, &domain))
}

fn parsed_http_host(url: &str) -> Option<String> {
    let parsed = url::Url::parse(url).ok()?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return None;
    }
    parsed.host_str().map(|host| host.to_ascii_lowercase())
}

fn normalize_allowed_domain(domain: &str) -> Option<String> {
    let trimmed = domain.trim().trim_end_matches('.');
    if trimmed.is_empty()
        || trimmed.contains(['/', ':'])
        || trimmed.chars().any(char::is_whitespace)
    {
        return None;
    }
    Some(trimmed.to_ascii_lowercase())
}

fn host_matches_allowed_domain(host: &str, domain: &str) -> bool {
    host == domain
        || (host.len() > domain.len()
            && host.ends_with(domain)
            && host.as_bytes()[host.len() - domain.len() - 1] == b'.')
}

/// Try the shared AGS dialog renderer, then deny if no renderer is available.
fn prompt_with_dialog(
    url: &str,
    has_callback: bool,
    can_proxy: bool,
    host_ui_socket: Option<&Path>,
) -> OpenDecision {
    let request = auth_dialog_request(url, has_callback, can_proxy);
    match crate::host_dialog::prompt_choice(&request, host_ui_socket) {
        crate::host_dialog::DialogOutcome::Choice(choice) => {
            parse_named_decision(&choice, can_proxy).unwrap_or(OpenDecision::Cancel)
        }
        crate::host_dialog::DialogOutcome::Cancelled => OpenDecision::Cancel,
        crate::host_dialog::DialogOutcome::Unavailable => {
            eprintln!("[ags auth-proxy] no dialog tool available (enable [host_ui] or install zenity/kdialog)");
            eprintln!("[ags auth-proxy] denying URL open: {url}");
            OpenDecision::Cancel
        }
    }
}

/// Produce a display-safe URL: strip query string and escape Pango/XML markup characters.
fn display_url(url: &str) -> String {
    let base = match url.find('?') {
        Some(i) => format!("{}?...", &url[..i]),
        None => url.to_owned(),
    };
    base.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(test)]
fn prompt_text(url: &str, has_callback: bool, can_proxy: bool) -> String {
    let display = display_url(url);
    let host = parsed_http_host(url).unwrap_or_else(|| "(unable to parse host)".to_owned());
    let action = if has_callback {
        "open this URL and relay a localhost callback back into the sandbox"
    } else {
        "open this URL"
    };

    let mut details = vec![format!("Requested host: {host}"), display];
    if has_callback {
        details.push(
            "This flow includes a localhost callback that AGS will capture and relay to the sandbox."
                .to_owned(),
        );
    }
    if can_proxy {
        details.push(
            "Proxy is available for this localhost app: Open uses the original URL, Proxy routes it through AGS."
                .to_owned(),
        );
    }

    let choices = if has_callback {
        "Choose Open to open it in the host browser or Cancel to deny."
    } else if can_proxy {
        "Choose Open to open the original URL, Proxy to route sandbox localhost through AGS, or Cancel to deny."
    } else {
        "Choose Open to open it or Cancel to deny."
    };

    format!(
        "A sandbox tool wants to {action}:\n\n{}\n\n{choices}",
        details.join("\n\n")
    )
}

fn auth_dialog_request(
    url: &str,
    has_callback: bool,
    can_proxy: bool,
) -> crate::host_dialog::DialogRequest {
    let display = display_url(url)
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">");
    let host = parsed_http_host(url).unwrap_or_else(|| "(unable to parse host)".to_owned());
    let action = if has_callback {
        "A sandbox tool wants to open a URL and relay a localhost callback back into the sandbox."
    } else {
        "A sandbox tool wants to open a URL on the host."
    };
    let mut details = vec![
        crate::host_dialog::DialogDetail::new("Requested host", host),
        crate::host_dialog::DialogDetail::new("URL", display),
    ];
    if has_callback {
        details.push(crate::host_dialog::DialogDetail::new(
            "Callback relay",
            "AGS will capture the localhost callback and relay it back into the sandbox.",
        ));
    }
    if can_proxy {
        details.push(crate::host_dialog::DialogDetail::new(
            "Proxy option",
            "Open uses the original URL. Proxy routes this localhost app through AGS.",
        ));
    }

    let mut choices = vec![crate::host_dialog::DialogChoice::new(
        "open",
        "Open",
        crate::host_dialog::DialogChoiceRole::Primary,
    )];
    if can_proxy {
        choices.push(crate::host_dialog::DialogChoice::new(
            "proxy",
            "Proxy",
            crate::host_dialog::DialogChoiceRole::Secondary,
        ));
    }
    choices.push(crate::host_dialog::DialogChoice::new(
        "cancel",
        "Cancel",
        crate::host_dialog::DialogChoiceRole::Cancel,
    ));

    crate::host_dialog::DialogRequest {
        title: "AGS Auth Proxy".to_owned(),
        heading: "Allow sandbox URL open?".to_owned(),
        message: action.to_owned(),
        details,
        note: Some("Only approve URLs you expected this sandbox session to open.".to_owned()),
        choices,
        width: 560,
        height: if can_proxy || has_callback { 420 } else { 360 },
    }
}

fn parse_named_decision(label: &str, can_proxy: bool) -> Option<OpenDecision> {
    match label.trim().to_ascii_lowercase().as_str() {
        "open" | "ok" => Some(OpenDecision::OpenOriginal),
        "proxy" if can_proxy => Some(OpenDecision::Proxy),
        "cancel" => Some(OpenDecision::Cancel),
        _ => None,
    }
}

#[cfg(test)]
fn parse_zenity_decision(output: &std::process::Output, can_proxy: bool) -> OpenDecision {
    let stdout = String::from_utf8_lossy(&output.stdout);
    if let Some(decision) = parse_named_decision(&stdout, can_proxy) {
        return decision;
    }
    if output.status.success() {
        OpenDecision::OpenOriginal
    } else {
        OpenDecision::Cancel
    }
}

// --- Localhost proxy rewriting ---

fn is_proxyable_localhost_url(url: &str) -> bool {
    let Ok(parsed) = url::Url::parse(url) else {
        return false;
    };
    parsed.scheme() == "http"
        && parsed.port().is_some()
        && matches!(parsed.host_str(), Some("localhost") | Some("127.0.0.1"))
}

fn rewrite_localhost_url_via_relay(url: &str, socket_path: &Path) -> Result<String, String> {
    let parsed = url::Url::parse(url).map_err(|e| format!("invalid URL: {e}"))?;
    if parsed.scheme() != "http" {
        return Err("only http://localhost:<port> URLs can be proxied".to_owned());
    }
    let Some(port) = parsed.port() else {
        return Err("localhost proxy requires an explicit port".to_owned());
    };
    if !matches!(parsed.host_str(), Some("localhost") | Some("127.0.0.1")) {
        return Err("proxy option is only available for localhost/127.0.0.1 URLs".to_owned());
    }

    let base_url = webview_relay::register_local_app(socket_path, port, "/")
        .map_err(|e| format!("relay registration failed: {e}"))?;
    let mut rewritten =
        url::Url::parse(&base_url).map_err(|e| format!("relay returned an invalid URL: {e}"))?;
    rewritten.set_path(parsed.path());
    rewritten.set_query(parsed.query());
    rewritten.set_fragment(parsed.fragment());
    Ok(rewritten.to_string())
}

// --- Host UI URL open ---

fn open_url_in_host_ui<F>(
    socket_path: &Path,
    url: &str,
    mut next_request_id: F,
) -> Result<HostUiWindowLease, String>
where
    F: FnMut() -> String,
{
    let mut writer = UnixStream::connect(socket_path).map_err(|e| {
        format!(
            "failed to connect to host UI socket {}: {e}",
            socket_path.display()
        )
    })?;
    writer
        .set_read_timeout(Some(Duration::from_secs(5)))
        .map_err(|e| format!("failed to configure host UI socket: {e}"))?;
    let reader_stream = writer
        .try_clone()
        .map_err(|e| format!("failed to clone host UI socket: {e}"))?;
    let mut reader = BufReader::new(reader_stream);

    let hello_id = next_request_id();
    let _ = host_ui_request(
        &mut writer,
        &mut reader,
        &hello_id,
        "hello",
        json!({
            "client_name": "ags-auth-proxy",
            "client_version": env!("CARGO_PKG_VERSION"),
            "protocol_min": 1,
            "protocol_max": 1,
            "session_id": null,
        }),
    )?;

    let open_id = next_request_id();
    let _ = host_ui_request(
        &mut writer,
        &mut reader,
        &open_id,
        "open",
        json!({
            "source": { "kind": "url", "url": url },
            "options": {
                "width": 1100,
                "height": 800,
                "title": "Sandbox App"
            }
        }),
    )?;

    // Drain incoming messages so the host UI doesn't block on a full send buffer.
    thread::spawn(move || {
        let _ = io::copy(&mut reader, &mut io::sink());
    });

    Ok(HostUiWindowLease { _writer: writer })
}

fn host_ui_request(
    writer: &mut UnixStream,
    reader: &mut BufReader<UnixStream>,
    id: &str,
    method: &str,
    params: serde_json::Value,
) -> Result<serde_json::Value, String> {
    let payload = json!({
        "v": 1,
        "kind": "request",
        "id": id,
        "method": method,
        "params": params,
    });
    writeln!(writer, "{payload}").map_err(|e| format!("failed to send host UI request: {e}"))?;
    writer
        .flush()
        .map_err(|e| format!("failed to flush host UI request: {e}"))?;

    let mut line = String::new();
    loop {
        line.clear();
        let read = reader
            .read_line(&mut line)
            .map_err(|e| format!("failed to read host UI response: {e}"))?;
        if read == 0 {
            return Err("host UI closed the connection".to_owned());
        }
        let value: serde_json::Value = serde_json::from_str(line.trim())
            .map_err(|e| format!("invalid host UI response: {e}"))?;

        if value.get("kind").and_then(|v| v.as_str()) != Some("response")
            || value.get("id").and_then(|v| v.as_str()) != Some(id)
        {
            continue;
        }

        return if value.get("ok").and_then(|v| v.as_bool()) == Some(true) {
            Ok(value
                .get("result")
                .cloned()
                .unwrap_or(serde_json::Value::Null))
        } else {
            let message = value
                .get("error")
                .and_then(|e| e.get("message"))
                .and_then(|v| v.as_str())
                .unwrap_or("host UI request failed");
            Err(message.to_owned())
        };
    }
}

// --- Browser open ---

fn open_url_on_host(url: &str) -> Result<(), String> {
    let status = std::process::Command::new("xdg-open")
        .arg(url)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map_err(|e| format!("xdg-open failed to start: {e}"))?;

    if status.success() {
        Ok(())
    } else {
        Err(format!("xdg-open exited with {status}"))
    }
}
