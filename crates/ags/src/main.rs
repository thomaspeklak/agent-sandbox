use std::path::{Path, PathBuf};
use std::process::ExitCode;

use ags::cli::{self, Agent, Command, RunOptions, SubCommand};
use ags::config::{self, ValidatedConfig};
use ags::secrets::{self, OsSecretBackend};
use ags::ssh::{self, OsSshRunner, SshKey};
use ags::trust::StdioRepoConfigPrompter;

fn main() -> ExitCode {
    let update_check = ags::update_check::UpdateCheck::from_default_cache();

    let code = match cli::parse_args(std::env::args()) {
        Ok(Command::Run(opts)) => run_agent(opts),
        Ok(Command::Sub(sub)) => {
            let skip_notice = matches!(
                sub,
                SubCommand::Completions(_)
                    | SubCommand::UpdateImage(_)
                    | SubCommand::UpdateDeprecated(_)
            );
            let code = run_subcommand(sub);
            if skip_notice {
                return code;
            }
            code
        }
        Err(cli::CliError::HelpRequested) => {
            println!("{}", cli::help_text());
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("error: {error}");
            eprintln!("\n{}", cli::help_text());
            ExitCode::from(2)
        }
    };

    update_check.notify_if_available();
    code
}

/// Run a fallible subcommand, printing `"{label} error: …"` on failure.
fn try_sub(label: &str, result: Result<(), impl std::fmt::Display>) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{label} error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run_update_image(config: &ValidatedConfig, opts: ags::cli::UpdateImageOptions) -> ExitCode {
    if let Err(e) = ags::assets::ensure_image_build_context(&config.sandbox.containerfile) {
        eprintln!("update-image error: could not prepare image build context: {e}");
        return ExitCode::FAILURE;
    }
    try_sub(
        "update-image",
        ags::cmd::update::run(
            config,
            &ags::cmd::update::UpdateOptions {
                keep_existing: opts.keep_existing,
                ..Default::default()
            },
        ),
    )
}

fn run_subcommand(sub: SubCommand) -> ExitCode {
    // Subcommands that don't need a config file.
    match sub {
        SubCommand::Install(ref opts) => return try_sub("install", ags::cmd::install::run(opts)),
        SubCommand::Uninstall => return try_sub("uninstall", ags::cmd::install::uninstall()),
        SubCommand::CreateAliases(ref opts) => {
            return try_sub("create-aliases", ags::cmd::create_aliases::run(opts));
        }
        SubCommand::Completions(ref opts) => {
            return try_sub("completions", ags::cmd::completions::run(opts));
        }
        SubCommand::Config => {
            let config_path = ags::config::default_config_path();
            return try_sub("config", ags::cmd::config_editor::run(&config_path));
        }
        SubCommand::Setup
        | SubCommand::Doctor
        | SubCommand::UpdateImage(_)
        | SubCommand::UpdateDeprecated(_)
        | SubCommand::UpdateAgents => {}
    }

    let config = match load_config(None) {
        Ok(c) => c,
        Err(code) => return code,
    };

    match sub {
        SubCommand::Setup => try_sub("setup", ags::cmd::setup::run(&config)),
        SubCommand::Doctor => {
            if ags::cmd::doctor::run(&config) {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        SubCommand::UpdateImage(opts) => run_update_image(&config, opts),
        SubCommand::UpdateDeprecated(opts) => {
            eprintln!("warning: `ags update` is deprecated; use `ags update-image` instead.");
            run_update_image(&config, opts)
        }
        SubCommand::UpdateAgents => try_sub(
            "update-agents",
            ags::cmd::update_agents::run(
                &config,
                &ags::cmd::update_agents::UpdateAgentsOptions::default(),
            ),
        ),
        SubCommand::Install(_)
        | SubCommand::Uninstall
        | SubCommand::CreateAliases(_)
        | SubCommand::Completions(_)
        | SubCommand::Config => {
            unreachable!()
        }
    }
}

fn run_agent(opts: RunOptions) -> ExitCode {
    // 1. Load and validate config
    let config = match load_config(opts.config_path.as_deref()) {
        Ok(c) => c,
        Err(code) => return code,
    };
    if let Err(e) = ags::lockdown::validate(&opts) {
        eprintln!("error: {e}");
        return ExitCode::from(2);
    }
    if let Err(e) = ags::psp::validate_options(&opts) {
        eprintln!("error: {e}");
        return ExitCode::from(2);
    }

    // 2. Ensure embedded assets are on disk
    if let Err(e) = ags::assets::ensure_image_build_context(&config.sandbox.containerfile) {
        eprintln!("warning: could not prepare image build context: {e}");
    }
    if !opts.lockdown && matches!(opts.agent, Agent::Pi | Agent::Shell) {
        if let Some(pi_host) = config.mount_host_for_container("/home/dev/.pi") {
            let pi_agent_dir = pi_host.join("agent");
            if let Err(e) = ags::assets::ensure_guard_extension(&pi_agent_dir) {
                eprintln!("warning: could not write guard extension: {e}");
            }
            if let Err(e) = ags::assets::ensure_settings_template(&pi_agent_dir) {
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
        if let Err(e) = ags::assets::ensure_claude_guard_hook(&hooks_dir) {
            eprintln!("warning: could not write Claude guard hook: {e}");
        }
        if let Err(e) = ags::assets::ensure_claude_guard_skill(&hooks_dir) {
            eprintln!("warning: could not write Claude guard skill: {e}");
        }
    }

    let _lockdown_session = if opts.lockdown {
        match ags::lockdown::prepare(opts.agent, &config, !opts.yolo) {
            Ok(session) => Some(session),
            Err(e) => {
                eprintln!("error: {e}");
                return ExitCode::FAILURE;
            }
        }
    } else {
        None
    };

    let resolved_secrets = if opts.lockdown {
        std::collections::HashMap::new()
    } else {
        secrets::resolve_secrets(&config.secrets, &OsSecretBackend)
    };

    if !opts.lockdown {
        let sign_key_container = "/home/dev/.ssh/ags-agent-signing.pub";
        if let Err(e) =
            ags::git::ensure_gitconfig(&config.sandbox.gitconfig_path, sign_key_container)
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
    let runtime_base = match ags::util::runtime_dir() {
        Ok(dir) => dir,
        Err(e) => {
            eprintln!("error: failed to prepare AGS runtime dir: {e}");
            return ExitCode::FAILURE;
        }
    };
    let pid = std::process::id();

    let mut _browser_guard = None;
    if !opts.lockdown && opts.browser {
        match ags::browser::start_if_needed(true, &config.browser) {
            Ok(sidecar) => _browser_guard = sidecar,
            Err(e) => {
                eprintln!("error: browser: {e}");
                return ExitCode::FAILURE;
            }
        }
    }

    let _host_ui_guard: Option<ags::host_ui::HostUiGuard> =
        if !opts.lockdown && config.host_ui.enabled {
            let dir = runtime_base.join(format!("ags-host-ui-{pid}"));
            let session_id = format!("ags-{}-{pid}", opts.agent.as_str());
            match ags::host_ui::start(&dir, session_id, &config.host_ui) {
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
        match ags::clipboard::start(&dir, clipboard_mode, config.clipboard.max_bytes) {
            Ok(guard) => {
                if let Err(e) = ags::assets::ensure_clipboard_shim(&guard.runtime_dir) {
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
        match ags::webview_relay::start(&dir) {
            Ok(guard) => {
                if let Err(e) = ags::assets::ensure_webview_relay_assets(&guard.runtime_dir) {
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
            .map(|g| g.runtime_dir.join(ags::webview_relay::SOCKET_NAME));
        let host_ui_socket = _host_ui_guard
            .as_ref()
            .map(|g| g.runtime_dir.join("host-ui.sock"));
        match ags::auth_proxy::start(
            &dir,
            config.auth_proxy.auto_allow_domains.clone(),
            relay_socket,
            host_ui_socket,
        ) {
            Ok(guard) => {
                if let Err(e) = ags::assets::ensure_auth_proxy_shim(&guard.runtime_dir) {
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
        for warning in ags::psp::operator_warnings(opts.psp_keep) {
            eprintln!("warning: {warning}");
        }
        match ags::psp::start(&config.psp.binary, opts.psp_keep) {
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

    // 8. Build launch plan
    let plan = match ags::plan::build_launch_plan(
        &config,
        &workdir,
        opts.agent,
        ags::plan::BuildLaunchPlanOptions {
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
            if let Err(e) = ags::podman::ensure_image(&plan.image, &plan.containerfile) {
                eprintln!("error: {e}");
                return ExitCode::FAILURE;
            }
            match ags::podman::image_has_binary(&plan.image, "dcg") {
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

    // 9. Execute via podman
    match ags::podman::execute(&plan, &opts.passthrough_args) {
        Ok(code) => ExitCode::from(code),
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn load_config(override_path: Option<&Path>) -> Result<ValidatedConfig, ExitCode> {
    let config_path = override_path
        .map(PathBuf::from)
        .unwrap_or_else(ags::config::default_config_path);

    if !config_path.exists() {
        if let Err(e) = ags::config::create_default_config(&config_path) {
            eprintln!("error: could not create default config: {e}");
            return Err(ExitCode::from(2));
        }
        eprintln!("Created default config: {}", config_path.display());
    }

    let repo_local_config = std::env::current_dir().ok().and_then(|cwd| {
        match ags::trust::resolve_repo_local_overlay(
            &cwd,
            &config_path,
            &ags::trust::default_trust_store_path(),
            &StdioRepoConfigPrompter,
        ) {
            Ok(path) => path,
            Err(err) => {
                eprintln!("warning: could not load repo trust state: {err}");
                None
            }
        }
    });

    config::parse_and_validate_with_overlay(&config_path, repo_local_config.as_deref()).map_err(
        |e| {
            eprintln!("error: {e}");
            ExitCode::from(2)
        },
    )
}
