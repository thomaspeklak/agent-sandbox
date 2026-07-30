//! Top-level agent launch lifecycle.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use crate::cli::{Agent, RunOptions};
use crate::config::{self, ValidatedConfig};
use crate::onepassword::{BootstrapAssetGuard, SourceRef};
use crate::secrets::{self, OsHostCommandRunner, OsSecretBackend};
use crate::ssh::{self, OsSshRunner, SshKey};
use crate::trust::StdioRepoConfigPrompter;

pub fn run_agent(opts: RunOptions) -> ExitCode {
    // Parse sources before any host lookup. They remain metadata until Podman
    // is ready to launch, so no 1Password payload is available during preflight.
    let sources = match opts
        .op_secret_sets
        .iter()
        .map(|source| SourceRef::parse(source))
        .collect::<Result<Vec<_>, _>>()
    {
        Ok(sources) => sources,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::from(2);
        }
    };
    if let Err(e) = crate::lockdown::validate(&opts) {
        eprintln!("error: {e}");
        return ExitCode::from(2);
    }
    if let Err(e) = crate::psp::validate_options(&opts) {
        eprintln!("error: {e}");
        return ExitCode::from(2);
    }
    // Config loading is intentionally after source/lockdown validation but
    // before all other preflight work. It never invokes `op`.
    let config = match load_config(opts.config_path.as_deref()) {
        Ok(c) => c,
        Err(code) => return code,
    };

    // 2. Ensure embedded assets are on disk
    if let Err(e) = crate::assets::ensure_image_build_context(&config.sandbox.containerfile) {
        eprintln!("warning: could not prepare image build context: {e}");
    }
    if !opts.lockdown && matches!(opts.agent, Agent::Pi | Agent::Shell) {
        if let Some(pi_host) = config.mount_host_for_container("/home/dev/.pi") {
            let pi_agent_dir = pi_host.join("agent");
            if let Err(e) = crate::assets::ensure_guard_extension(&pi_agent_dir) {
                eprintln!("warning: could not write guard extension: {e}");
            }
            if let Err(e) = crate::assets::ensure_settings_template(&pi_agent_dir) {
                eprintln!("warning: could not write settings template: {e}");
            }
        } else {
            eprintln!(
                "warning: no mount found for /home/dev/.pi; cannot ensure Pi guard/settings assets"
            );
        }
    }
    if !opts.lockdown && matches!(opts.agent, Agent::Claude) {
        let hooks_dir = config.sandbox.cache_dir.join("ags-hooks");
        if let Err(e) = crate::assets::ensure_claude_guard_hook(&hooks_dir) {
            eprintln!("warning: could not write Claude guard hook: {e}");
        }
        if let Err(e) = crate::assets::ensure_claude_guard_skill(&hooks_dir) {
            eprintln!("warning: could not write Claude guard skill: {e}");
        }
    }

    let _lockdown_session = if opts.lockdown {
        match crate::lockdown::prepare(opts.agent, &config, !opts.yolo) {
            Ok(session) => Some(session),
            Err(e) => {
                eprintln!("error: {e}");
                return ExitCode::FAILURE;
            }
        }
    } else {
        None
    };

    let resolved_secrets = secrets::resolve_secrets_for_run(
        &config.secrets,
        &OsSecretBackend,
        &OsHostCommandRunner,
        opts.lockdown,
    );

    if !opts.lockdown {
        let sign_key_container = "/home/dev/.ssh/ags-agent-signing.pub";
        if let Err(e) =
            crate::git::ensure_gitconfig(&config.sandbox.gitconfig_path, sign_key_container)
        {
            eprintln!("warning: git config bootstrap failed: {e}");
        }
    }

    let ssh_sock = if opts.lockdown {
        None
    } else {
        match ssh::ensure_agent(
            &config.sandbox.cache_dir,
            &[
                SshKey {
                    private_path: config.sandbox.auth_key.clone(),
                    label: "auth".into(),
                },
                SshKey {
                    private_path: config.sandbox.sign_key.clone(),
                    label: "signing".into(),
                },
            ],
            &OsSshRunner,
        ) {
            Ok(ready) => {
                for w in &ready.warnings {
                    eprintln!("warning: {w}");
                }
                Some(ready.auth_sock)
            }
            Err(e) => {
                eprintln!("warning: SSH agent setup failed: {e}");
                None
            }
        }
    };

    // 6. Sidecars
    let runtime_base = match crate::util::runtime_dir() {
        Ok(dir) => dir,
        Err(e) => {
            eprintln!("error: failed to prepare AGS runtime dir: {e}");
            return ExitCode::FAILURE;
        }
    };
    let pid = std::process::id();

    let mut _browser_guard = None;
    if !opts.lockdown && opts.browser {
        match crate::browser::start_if_needed(true, &config.browser) {
            Ok(sidecar) => _browser_guard = sidecar,
            Err(e) => {
                eprintln!("error: browser: {e}");
                return ExitCode::FAILURE;
            }
        }
    }

    let _host_ui_guard: Option<crate::host_ui::HostUiGuard> =
        if !opts.lockdown && config.host_ui.enabled {
            let dir = runtime_base.join(format!("ags-host-ui-{pid}"));
            let session_id = format!("ags-{}-{pid}", opts.agent.as_str());
            match crate::host_ui::start(&dir, session_id, &config.host_ui) {
                Ok(guard) => Some(guard),
                Err(e) => {
                    eprintln!("warning: host UI: {e}");
                    None
                }
            }
        } else {
            None
        };

    let clipboard_mode = config.clipboard.effective_mode();
    let _clipboard_guard = if !opts.lockdown && clipboard_mode.can_read() {
        let dir = runtime_base.join(format!("ags-clipboard-{pid}"));
        let clipboard_approval = crate::clipboard::ClipboardApprovalConfig {
            required: config.clipboard.approval_required,
            window_seconds: config.clipboard.approval_seconds,
            approve_writes: config.clipboard.approve_writes,
        };
        match crate::clipboard::start(
            &dir,
            clipboard_mode,
            config.clipboard.max_bytes,
            clipboard_approval,
            _host_ui_guard.as_ref().map(|g| g.socket_path.as_path()),
        ) {
            Ok(guard) => {
                if let Err(e) = crate::assets::ensure_clipboard_shim(&guard.runtime_dir) {
                    eprintln!("warning: clipboard shim write failed: {e}");
                }
                Some(guard)
            }
            Err(e) => {
                eprintln!("warning: clipboard bridge: {e}");
                None
            }
        }
    } else {
        None
    };

    let _webview_relay_guard = if opts.lockdown {
        None
    } else {
        let dir = runtime_base.join(format!("ags-webview-relay-{pid}"));
        match crate::webview_relay::start(&dir) {
            Ok(guard) => {
                if let Err(e) = crate::assets::ensure_webview_relay_assets(&guard.runtime_dir) {
                    eprintln!("warning: webview relay assets write failed: {e}");
                }
                Some(guard)
            }
            Err(e) => {
                eprintln!("warning: webview relay: {e}");
                None
            }
        }
    };

    let _auth_proxy_guard = if opts.lockdown {
        None
    } else {
        let dir = runtime_base.join(format!("ags-auth-proxy-{pid}"));
        let relay_socket = _webview_relay_guard
            .as_ref()
            .map(|g| g.runtime_dir.join(crate::webview_relay::SOCKET_NAME));
        let host_ui_socket = _host_ui_guard
            .as_ref()
            .map(|g| g.runtime_dir.join("host-ui.sock"));
        match crate::auth_proxy::start(
            &dir,
            config.auth_proxy.auto_allow_domains.clone(),
            relay_socket,
            host_ui_socket,
        ) {
            Ok(guard) => {
                if let Err(e) = crate::assets::ensure_auth_proxy_shim(&guard.runtime_dir) {
                    eprintln!("warning: auth proxy shim write failed: {e}");
                }
                Some(guard)
            }
            Err(e) => {
                eprintln!("warning: auth proxy: {e}");
                None
            }
        }
    };

    let _psp_guard = if !opts.lockdown && opts.psp {
        for warning in crate::psp::operator_warnings(opts.psp_keep) {
            eprintln!("warning: {warning}");
        }
        match crate::psp::start(&config.psp.binary, opts.psp_keep) {
            Ok(guard) => Some(guard),
            Err(e) => {
                eprintln!("error: {e}");
                return ExitCode::FAILURE;
            }
        }
    } else {
        None
    };
    let psp_session_id = _psp_guard
        .as_ref()
        .map(|_| format!("ags-{}-{pid}", opts.agent.as_str()));

    // 7. Working directory
    let workdir = match std::env::current_dir() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("error: cannot determine working directory: {e}");
            return ExitCode::FAILURE;
        }
    };

    let bootstrap_asset = if sources.is_empty() {
        None
    } else {
        match BootstrapAssetGuard::prepare(&runtime_base) {
            Ok(asset) => Some(asset),
            Err(e) => {
                eprintln!("error: failed to prepare 1Password bootstrap asset: {e}");
                return ExitCode::FAILURE;
            }
        }
    };
    let bootstrap_host_path = bootstrap_asset.as_ref().map(BootstrapAssetGuard::path);

    // 8. Build launch plan
    let plan = match crate::plan::build_launch_plan(
        &config,
        &workdir,
        opts.agent,
        crate::plan::BuildLaunchPlanOptions {
            browser_mode: opts.browser,
            tmux_mode: opts.tmux,
            guard_enabled: !opts.yolo,
            lockdown: opts.lockdown,
            ssh_auth_sock: ssh_sock.as_deref(),
            resolved_secrets: &resolved_secrets,
            auth_proxy_runtime_dir: _auth_proxy_guard.as_ref().map(|g| g.runtime_dir.as_path()),
            clipboard_runtime_dir: _clipboard_guard.as_ref().map(|g| g.runtime_dir.as_path()),
            clipboard_mode,
            host_ui_runtime_dir: _host_ui_guard.as_ref().map(|g| g.runtime_dir.as_path()),
            host_ui_session_id: _host_ui_guard.as_ref().map(|g| g.session_id.as_str()),
            webview_relay_runtime_dir: _webview_relay_guard
                .as_ref()
                .map(|g| g.runtime_dir.as_path()),
            psp_socket: _psp_guard.as_ref().map(|g| g.socket_path.as_path()),
            psp_session_id: psp_session_id.as_deref(),
            extra_mounts: _lockdown_session
                .as_ref()
                .map(|s| s.extra_mounts.as_slice())
                .unwrap_or(&[]),
            extra_mount_dirs: &opts.add_dirs,
            stop_when_done: opts.stop_when_done,
            root_mode: opts.root,
            wayland_passthrough: opts.wayland_compositor_passthrough
                || config.desktop_passthrough.wayland,
            payload_fd_count: sources.len(),
            bootstrap_path: (!sources.is_empty())
                .then_some(crate::plan::ONEPASSWORD_BOOTSTRAP_CONTAINER_PATH),
            bootstrap_host_path: bootstrap_host_path.as_deref(),
        },
    ) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };

    if matches!(opts.agent, Agent::Pi | Agent::Claude) {
        if opts.root {
            eprintln!("warning: --root grants root access inside the sandbox for this run");
        }
        if opts.yolo {
            eprintln!(
                "warning: --yolo disables AGS {} guards for this run",
                opts.agent.as_str()
            );
        } else {
            if let Err(e) = crate::podman::ensure_image(&plan.image, &plan.containerfile) {
                eprintln!("error: {e}");
                return ExitCode::FAILURE;
            }
            match crate::podman::image_has_binary(&plan.image, "dcg") {
                Ok(true) => {}
                Ok(false) => eprintln!(
                    "warning: destructive_command_guard (dcg) is missing in the sandbox image; AGS {} Bash guards will fail open. Run `ags doctor` or `ags update-image`.",
                    opts.agent.as_str()
                ),
                Err(e) => eprintln!(
                    "warning: could not verify destructive_command_guard (dcg) availability in the sandbox image: {e}"
                ),
            }
        }
    }

    // 9. Resolve 1Password payloads only at the final Podman handoff.
    let result = if sources.is_empty() {
        crate::podman::execute(&plan, &opts.passthrough_args)
    } else {
        crate::podman::execute_with_payload_sources(&plan, &opts.passthrough_args, &sources)
    };
    match result {
        Ok(code) => ExitCode::from(code),
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

pub fn load_config(override_path: Option<&Path>) -> Result<ValidatedConfig, ExitCode> {
    let config_path = override_path
        .map(PathBuf::from)
        .unwrap_or_else(crate::config::default_config_path);

    if !config_path.exists() {
        if let Err(e) = crate::config::create_default_config(&config_path) {
            eprintln!("error: could not create default config: {e}");
            return Err(ExitCode::from(2));
        }
        eprintln!("Created default config: {}", config_path.display());
    }

    let repo_local_config =
        std::env::current_dir().ok().and_then(
            |cwd| match crate::trust::resolve_repo_local_overlay(
                &cwd,
                &config_path,
                &crate::trust::default_trust_store_path(),
                &StdioRepoConfigPrompter,
            ) {
                Ok(path) => path,
                Err(err) => {
                    eprintln!("warning: could not load repo trust state: {err}");
                    None
                }
            },
        );

    config::parse_and_validate_with_overlay(&config_path, repo_local_config.as_deref()).map_err(
        |e| {
            eprintln!("error: {e}");
            ExitCode::from(2)
        },
    )
}
