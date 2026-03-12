use std::path::{Path, PathBuf};
use std::process::ExitCode;

use ags::cli::{self, Agent, Command, RunOptions, SubCommand};
use ags::config::{self, ValidatedConfig};
use ags::secrets::{self, OsSecretBackend};
use ags::ssh::{self, OsSshRunner, SshKey};

fn main() -> ExitCode {
    let update_check = ags::update_check::UpdateCheck::from_default_cache();

    let code = match cli::parse_args(std::env::args()) {
        Ok(Command::Run(opts)) => run_agent(opts),
        Ok(Command::Sub(sub)) => {
            let skip_notice = matches!(sub, SubCommand::Completions(_) | SubCommand::Update);
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

fn run_subcommand(sub: SubCommand) -> ExitCode {
    match sub {
        SubCommand::Install(opts) => {
            if let Err(e) = ags::cmd::install::run(&opts) {
                eprintln!("install error: {e}");
                return ExitCode::FAILURE;
            }
            return ExitCode::SUCCESS;
        }
        SubCommand::Uninstall => {
            if let Err(e) = ags::cmd::install::uninstall() {
                eprintln!("uninstall error: {e}");
                return ExitCode::FAILURE;
            }
            return ExitCode::SUCCESS;
        }
        SubCommand::CreateAliases(opts) => {
            if let Err(e) = ags::cmd::create_aliases::run(&opts) {
                eprintln!("create-aliases error: {e}");
                return ExitCode::FAILURE;
            }
            return ExitCode::SUCCESS;
        }
        SubCommand::Completions(opts) => {
            if let Err(e) = ags::cmd::completions::run(&opts) {
                eprintln!("completions error: {e}");
                return ExitCode::FAILURE;
            }
            return ExitCode::SUCCESS;
        }
        SubCommand::Setup | SubCommand::Doctor | SubCommand::Update | SubCommand::UpdateAgents => {}
    }

    let config = match load_config(None) {
        Ok(c) => c,
        Err(code) => return code,
    };

    match sub {
        SubCommand::Setup => {
            if let Err(e) = ags::cmd::setup::run(&config) {
                eprintln!("setup error: {e}");
                return ExitCode::FAILURE;
            }
        }
        SubCommand::Doctor => {
            let ok = ags::cmd::doctor::run(&config);
            if !ok {
                return ExitCode::FAILURE;
            }
        }
        SubCommand::Update => {
            if let Err(e) = ags::assets::ensure_containerfile(&config.sandbox.containerfile) {
                eprintln!("update error: could not write Containerfile: {e}");
                return ExitCode::FAILURE;
            }
            let tmux_conf = config.sandbox.containerfile.with_file_name("tmux.conf");
            if let Err(e) = ags::assets::ensure_tmux_conf(&tmux_conf) {
                eprintln!("update error: could not write tmux config: {e}");
                return ExitCode::FAILURE;
            }
            let opts = ags::cmd::update::UpdateOptions::default();
            if let Err(e) = ags::cmd::update::run(&config, &opts) {
                eprintln!("update error: {e}");
                return ExitCode::FAILURE;
            }
        }
        SubCommand::UpdateAgents => {
            let opts = ags::cmd::update_agents::UpdateAgentsOptions::default();
            if let Err(e) = ags::cmd::update_agents::run(&config, &opts) {
                eprintln!("update-agents error: {e}");
                return ExitCode::FAILURE;
            }
        }
        SubCommand::Install(_)
        | SubCommand::Uninstall
        | SubCommand::CreateAliases(_)
        | SubCommand::Completions(_) => {
            unreachable!()
        }
    }

    ExitCode::SUCCESS
}

fn run_agent(opts: RunOptions) -> ExitCode {
    // 1. Load and validate config
    let config = match load_config(opts.config_path.as_deref()) {
        Ok(c) => c,
        Err(code) => return code,
    };

    // 2. Ensure embedded assets are on disk
    if let Err(e) = ags::assets::ensure_containerfile(&config.sandbox.containerfile) {
        eprintln!("warning: could not write Containerfile: {e}");
    }
    let tmux_conf = config.sandbox.containerfile.with_file_name("tmux.conf");
    if let Err(e) = ags::assets::ensure_tmux_conf(&tmux_conf) {
        eprintln!("warning: could not write tmux config: {e}");
    }
    if matches!(opts.agent, Agent::Pi | Agent::Shell) {
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
    if matches!(opts.agent, Agent::Claude) {
        let hooks_dir = config.sandbox.cache_dir.join("ags-hooks");
        if let Err(e) = ags::assets::ensure_claude_guard_hook(&hooks_dir) {
            eprintln!("warning: could not write Claude guard hook: {e}");
        }
        if let Err(e) = ags::assets::ensure_claude_guard_skill(&hooks_dir) {
            eprintln!("warning: could not write Claude guard skill: {e}");
        }
    }

    // 3. Resolve secrets
    let resolved_secrets = secrets::resolve_secrets(&config.secrets, &OsSecretBackend);

    // 4. Bootstrap git config
    let sign_key_container = "/home/dev/.ssh/ags-agent-signing.pub";
    if let Err(e) = ags::git::ensure_gitconfig(&config.sandbox.gitconfig_path, sign_key_container) {
        eprintln!("warning: git config bootstrap failed: {e}");
    }

    // 5. Ensure SSH agent
    let ssh_sock = match ssh::ensure_agent(
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
    };

    // 6. Browser sidecar
    let mut _browser_guard = None;
    if opts.browser {
        match ags::browser::start_if_needed(true, &config.browser) {
            Ok(sidecar) => _browser_guard = sidecar,
            Err(e) => {
                eprintln!("error: browser: {e}");
                return ExitCode::FAILURE;
            }
        }
    }

    // 6b. Auth proxy
    let _auth_proxy_guard;
    let auth_proxy_runtime_dir;
    {
        let runtime_base = ags::util::runtime_dir();
        let dir = runtime_base.join(format!("ags-auth-proxy-{}", std::process::id()));

        match ags::auth_proxy::start(&dir, config.auth_proxy.auto_allow_domains.clone()) {
            Ok(guard) => {
                // Write the shim script into the runtime dir so it can be mounted
                if let Err(e) = ags::assets::ensure_auth_proxy_shim(&guard.runtime_dir) {
                    eprintln!("warning: auth proxy shim write failed: {e}");
                }
                auth_proxy_runtime_dir = Some(guard.runtime_dir.clone());
                _auth_proxy_guard = Some(guard);
            }
            Err(e) => {
                eprintln!("warning: auth proxy: {e}");
                auth_proxy_runtime_dir = None;
                _auth_proxy_guard = None;
            }
        }
    }

    // 6c. PSP sidecar
    let _psp_guard;
    let psp_socket;
    let psp_session_id;
    if opts.psp {
        match ags::psp::start(&config.psp.binary, opts.psp_keep) {
            Ok(guard) => {
                psp_socket = Some(guard.socket_path.clone());
                psp_session_id =
                    Some(format!("ags-{}-{}", opts.agent.as_str(), std::process::id()));
                _psp_guard = Some(guard);
            }
            Err(e) => {
                eprintln!("error: {e}");
                return ExitCode::FAILURE;
            }
        }
    } else {
        psp_socket = None;
        psp_session_id = None;
        _psp_guard = None;
    }

    // 7. Discover external git mounts
    let workdir = match std::env::current_dir() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("error: cannot determine working directory: {e}");
            return ExitCode::FAILURE;
        }
    };
    let _git_mounts = ags::git::discover_external_git_mounts(&workdir);

    // 8. Build launch plan
    let plan = match ags::plan::build_launch_plan(
        &config,
        &workdir,
        opts.agent,
        ags::plan::BuildLaunchPlanOptions {
            browser_mode: opts.browser,
            tmux_mode: opts.tmux,
            ssh_auth_sock: ssh_sock.as_deref(),
            resolved_secrets: &resolved_secrets,
            auth_proxy_runtime_dir: auth_proxy_runtime_dir.as_deref(),
            psp_socket: psp_socket.as_deref(),
            psp_session_id: psp_session_id.as_deref(),
            extra_mount_dirs: &opts.add_dirs,
        },
    ) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };

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
        .unwrap_or_else(default_config_path);

    if !config_path.exists() {
        if let Err(e) = create_default_config(&config_path) {
            eprintln!("error: could not create default config: {e}");
            return Err(ExitCode::from(2));
        }
        eprintln!("Created default config: {}", config_path.display());
    }

    config::parse_and_validate(&config_path).map_err(|e| {
        eprintln!("error: {e}");
        ExitCode::from(2)
    })
}

fn create_default_config(path: &Path) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, DEFAULT_CONFIG)
}

const DEFAULT_CONFIG: &str = r#"[sandbox]
image = "localhost/agent-sandbox:latest"
containerfile = "~/.config/ags/Containerfile"
cache_dir = "~/.cache/ags"
gitconfig_path = "~/.config/ags/gitconfig-agent"
auth_key = "~/.ssh/ags-agent-auth"
sign_key = "~/.ssh/ags-agent-signing"
bootstrap_files = ["auth.json", "models.json"]
container_boot_dirs = [
  "/home/dev/.ssh",
]
passthrough_env = [
  "ANTHROPIC_API_KEY",
  "OPENAI_API_KEY",
  "GEMINI_API_KEY",
  "OPENROUTER_API_KEY",
  "AI_GATEWAY_API_KEY",
  "OPENCODE_API_KEY",
]

[[mount]]
host = "~/.ssh/known_hosts"
container = "/home/dev/.ssh/known_hosts"
mode = "ro"
kind = "file"
optional = true

[[agent_mount]]
host = "~/.claude.json"
container = "/home/dev/.claude.json"
kind = "file"

[[agent_mount]]
host = "~/.claude"
container = "/home/dev/.claude"

[[agent_mount]]
host = "~/.codex"
container = "/home/dev/.codex"

[[agent_mount]]
host = "~/.pi"
container = "/home/dev/.pi"

[[agent_mount]]
host = "~/.config/opencode"
container = "/home/dev/.config/opencode"

[[agent_mount]]
host = "~/.gemini"
container = "/home/dev/.gemini"
"#;

fn default_config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("~/.config"))
        .join("ags/config.toml")
}
