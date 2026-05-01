fn build_env(
    config: &ValidatedConfig,
    profile: &AgentProfile,
    ctx: BuildEnvContext<'_>,
) -> PlanEnv {
    let BuildEnvContext {
        wayland,
        read_roots,
        write_roots,
        resolved_secrets,
        auth_proxy_runtime_dir,
        host_ui_runtime_dir,
        host_ui_session_id,
        webview_relay_runtime_dir,
        psp_socket,
        psp_session_id,
        guard_enabled,
        lockdown,
    } = ctx;
    let mut inline = vec![
        ("HOME".to_owned(), CONTAINER_HOME.to_owned()),
        ("RUSTUP_HOME".to_owned(), "/usr/local/rustup".to_owned()),
        ("AGS_SANDBOX".to_owned(), "1".to_owned()),
    ];
    if lockdown {
        inline.push(("AGS_LOCKDOWN".to_owned(), "1".to_owned()));
    } else {
        inline.push(("GIT_CONFIG_GLOBAL".to_owned(), CONTAINER_GITCONFIG.to_owned()));
        inline.push(("SSH_AUTH_SOCK".to_owned(), CONTAINER_SSH_SOCK.to_owned()));
        inline.push((
            "AGS_HOST_SERVICES_HOST".to_owned(),
            HOST_SERVICES_HOST.to_owned(),
        ));
        inline.push((
            "AGS_HOST_SERVICES_HINT".to_owned(),
            HOST_SERVICES_HINT.to_owned(),
        ));
    }

    inline.extend(profile.extra_env.iter().cloned());

    if !lockdown {
        inline.extend(
            CACHE_MOUNTS
                .iter()
                .filter(|(_, _, env_var)| !env_var.is_empty())
                .map(|(_, container_path, env_var)| {
                    (env_var.to_string(), container_path.to_string())
                }),
        );
    }

    if !guard_enabled {
        inline.push(("AGS_GUARD_YOLO".to_owned(), "1".to_owned()));
    }

    if let Some(w) = wayland {
        inline.push(("WAYLAND_DISPLAY".to_owned(), w.display_name.clone()));
        inline.push(("XDG_RUNTIME_DIR".to_owned(), "/tmp".to_owned()));
    }

    if !lockdown && auth_proxy_runtime_dir.is_some() {
        inline.push((
            "AGS_AUTH_PROXY_SOCK".to_owned(),
            AuthProxyGuard::container_socket_path().to_owned(),
        ));
        inline.push((
            "BROWSER".to_owned(),
            format!("{CONTAINER_HOME}/.local/bin/auth-proxy-shim"),
        ));
    }

    if !lockdown && host_ui_runtime_dir.is_some() {
        inline.push((
            "AGS_HOST_UI_SOCK".to_owned(),
            HostUiGuard::container_socket_path().to_owned(),
        ));
        inline.push((
            "GLIMPSE_BINARY_PATH".to_owned(),
            "/opt/ags/glimpse-shim".to_owned(),
        ));
        inline.push(("AGS_HOST_UI_PROTOCOL".to_owned(), "1".to_owned()));
        inline.push(("AGS_HOST_UI_TRANSPORT".to_owned(), "socket".to_owned()));
        inline.push((
            "AGS_HOST_UI_HINT".to_owned(),
            "[ags] Host UI available through mounted socket; host owns Glimpse windows".to_owned(),
        ));
        if let Some(session_id) = host_ui_session_id {
            inline.push(("AGS_HOST_UI_SESSION_ID".to_owned(), session_id.to_owned()));
        }
    }

    if !lockdown && webview_relay_runtime_dir.is_some() {
        inline.push((
            "AGS_WEBVIEW_RELAY_SOCKET".to_owned(),
            WebviewRelayGuard::container_socket_path().to_owned(),
        ));
        inline.push((
            "AGS_WEBVIEW_RELAY_UPSTREAM_SOCKET".to_owned(),
            WebviewRelayGuard::container_upstream_socket_path().to_owned(),
        ));
        inline.push((
            "AGS_WEBVIEW_URL_HELPER".to_owned(),
            format!("{CONTAINER_HOME}/.local/bin/ags-webview-url"),
        ));
    }

    if !lockdown && psp_socket.is_some() {
        inline.push((
            "DOCKER_HOST".to_owned(),
            format!("unix://{}", crate::psp::PspGuard::container_socket_path()),
        ));
        inline.push((
            "TESTCONTAINERS_HOST_OVERRIDE".to_owned(),
            HOST_SERVICES_HOST.to_owned(),
        ));
        if let Some(session_id) = psp_session_id {
            inline.push(("PSP_SESSION_ID".to_owned(), session_id.to_owned()));
        }
    }

    let passthrough_names = if lockdown {
        Vec::new()
    } else {
        ["TERM", "COLORTERM", "EDITOR", "VISUAL"]
            .into_iter()
            .map(String::from)
            .collect()
    };

    let mut env_file_entries = if lockdown {
        Vec::new()
    } else {
        resolved_secrets
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    };
    if !lockdown {
        for env_name in &config.sandbox.passthrough_env {
            if resolved_secrets.contains_key(env_name) {
                continue;
            }
            if let Ok(val) = std::env::var(env_name)
                && !val.is_empty()
            {
                env_file_entries.push((env_name.clone(), val));
            }
        }
    }

    PlanEnv {
        inline,
        passthrough_names,
        env_file_entries,
        read_roots_json: json_string_array(read_roots),
        write_roots_json: json_string_array(write_roots),
    }
}

// --- entrypoint ---

struct EntryPointContext<'a> {
    boot_dirs: &'a [String],
    profile: &'a AgentProfile,
    browser: &'a BrowserConfig,
    browser_mode: bool,
    tmux_mode: bool,
    webview_relay_enabled: bool,
    show_host_services_hint: bool,
    stop_when_done: bool,
}

fn build_entrypoint(ctx: EntryPointContext<'_>) -> String {
    let EntryPointContext {
        boot_dirs,
        profile,
        browser,
        browser_mode,
        tmux_mode,
        webview_relay_enabled,
        show_host_services_hint,
        stop_when_done,
    } = ctx;
    let mut script = String::new();

    let all_dirs: Vec<String> = boot_dirs
        .iter()
        .chain(profile.extra_boot_dirs.iter())
        .map(|d| shell_quote(d))
        .collect();

    if !all_dirs.is_empty() {
        script.push_str(&format!("mkdir -p {}; ", all_dirs.join(" ")));
    }

    if !profile.entrypoint_setup.is_empty() {
        script.push_str(&profile.entrypoint_setup);
        script.push_str("; ");
    }

    if browser_mode && browser.enabled {
        script.push_str(&format!(
            "socat TCP-LISTEN:{port},fork,reuseaddr,bind=127.0.0.1 \
             TCP:10.0.2.2:{port} >/tmp/ags-socat.log 2>&1 & ",
            port = browser.debug_port
        ));
    }

    if webview_relay_enabled {
        script.push_str(concat!(
            "if [ -n \"${AGS_WEBVIEW_RELAY_UPSTREAM_SOCKET:-}\" ]; then ",
            "if command -v python3 >/dev/null 2>&1; then ",
            "python3 /run/ags-webview-relay/webview-relay-shim ",
            ">/tmp/ags-webview-relay.log 2>&1 & ",
            "else ",
            "echo '[ags] warning: python3 is missing; ",
            "sandbox webview relay will not work in this sandbox.' >&2; ",
            "fi; fi; ",
        ));
    }

    if show_host_services_hint {
        script.push_str(&format!(
            "if [ -t 1 ]; then echo {} >&2; fi; ",
            shell_quote(HOST_SERVICES_HINT)
        ));
    }

    let agent_exec = build_agent_exec(profile, browser, browser_mode);

    if tmux_mode {
        script.push_str(
            "if ! command -v tmux >/dev/null 2>&1; then echo '[ags] tmux is not available in the sandbox image. Run `ags update-image` to rebuild the image with tmux support.' >&2; exit 127; fi; ",
        );
        script.push_str("cat > /tmp/ags-run-in-tmux.sh <<'EOF'\n#!/usr/bin/env bash\n");
        if stop_when_done {
            script.push_str(&agent_exec);
        } else {
            // Strip leading "exec " so the agent runs as a child process and the
            // script continues after it exits.
            let child_cmd = agent_exec.strip_prefix("exec ").unwrap_or(&agent_exec);
            script.push_str(child_cmd);
            script.push_str("\nAGS_EXIT=$?");
            script.push_str("\necho \"[ags] Agent exited (code $AGS_EXIT). Shell is ready — type 'exit' to stop the container.\"");
            script.push_str("\nexec bash");
        }
        script.push_str("\nEOF\n");
        script.push_str("chmod +x /tmp/ags-run-in-tmux.sh; ");
        script.push_str("exec tmux new-session -A -s ags /tmp/ags-run-in-tmux.sh \"$@\"");
    } else {
        script.push_str(&agent_exec);
    }

    script
}

fn build_agent_exec(profile: &AgentProfile, browser: &BrowserConfig, browser_mode: bool) -> String {
    let mut command = format!("exec {}", profile.command);
    for arg in &profile.command_args {
        command.push_str(&format!(" {}", shell_quote(arg)));
    }

    if browser_mode
        && browser.enabled
        && let Some(ref flag) = profile.browser_skill_flag
        && !profile.browser_skill_path.is_empty()
    {
        command.push_str(&format!(
            " {} {}",
            flag,
            shell_quote(&profile.browser_skill_path)
        ));
    }

    command.push_str(" \"$@\"");
    command
}

// --- JSON helpers ---

fn json_string_array(items: &[String]) -> String {
    let unique: BTreeSet<&str> = items
        .iter()
        .map(|s| s.as_str())
        .filter(|s| !s.is_empty())
        .collect();
    serde_json::to_string(&unique).expect("string array serialization cannot fail")
}
