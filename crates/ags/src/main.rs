use std::path::{Path, PathBuf};
use std::process::ExitCode;

use ags::cli::{self, Command, RunOptions, SubCommand};
use ags::config::{self, ValidatedConfig};
use ags::secrets::{self, OsSecretBackend};
use ags::ssh::{self, OsSshRunner, SshKey};

fn main() -> ExitCode {
    match cli::parse_args(std::env::args()) {
        Ok(Command::Run(opts)) => run_agent(opts),
        Ok(Command::Sub(sub)) => run_subcommand(sub),
        Err(cli::CliError::HelpRequested) => {
            println!("{}", cli::help_text());
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("error: {error}");
            eprintln!("\n{}", cli::help_text());
            ExitCode::from(2)
        }
    }
}

fn run_subcommand(sub: SubCommand) -> ExitCode {
    match sub {
        SubCommand::Install => {
            let root = project_root();
            if let Err(e) = ags::cmd::install::run(&root) {
                eprintln!("install error: {e}");
                return ExitCode::FAILURE;
            }
        }
        SubCommand::Uninstall => {
            let root = project_root();
            if let Err(e) = ags::cmd::install::uninstall(&root) {
                eprintln!("uninstall error: {e}");
                return ExitCode::FAILURE;
            }
        }
        _ => {
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
                    let opts = ags::cmd::update::UpdateOptions::default();
                    if let Err(e) = ags::cmd::update::run(&config, &opts) {
                        eprintln!("update error: {e}");
                        return ExitCode::FAILURE;
                    }
                }
                SubCommand::Install | SubCommand::Uninstall => unreachable!(),
            }
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

    // 2. Resolve secrets
    let resolved_secrets = secrets::resolve_secrets(&config.secrets, &OsSecretBackend);

    // 3. Bootstrap git config
    let sign_key_container = "/home/dev/.ssh/pi-agent-signing.pub";
    if let Err(e) = ags::git::ensure_gitconfig(&config.sandbox.gitconfig_path, sign_key_container) {
        eprintln!("warning: git config bootstrap failed: {e}");
    }

    // 4. Ensure SSH agent
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

    // 5. Browser sidecar
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

    // 6. Discover external git mounts
    let workdir = match std::env::current_dir() {
        Ok(d) => d,
        Err(e) => {
            eprintln!("error: cannot determine working directory: {e}");
            return ExitCode::FAILURE;
        }
    };
    let _git_mounts = ags::git::discover_external_git_mounts(&workdir);

    // 7. Build launch plan
    let plan = match ags::plan::build_launch_plan(
        &config,
        &workdir,
        opts.browser,
        ssh_sock.as_deref(),
        &resolved_secrets,
    ) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("error: {e}");
            return ExitCode::FAILURE;
        }
    };

    // 8. Execute via podman
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

    config::parse_and_validate(&config_path).map_err(|e| {
        eprintln!("error: {e}");
        ExitCode::from(2)
    })
}

/// Resolve the pi-sandbox project root.
///
/// Uses `PI_SBOX_PROJECT_ROOT` if set, otherwise walks up from the current
/// executable looking for a directory that contains `config/config.toml`.
fn project_root() -> PathBuf {
    if let Ok(root) = std::env::var("PI_SBOX_PROJECT_ROOT") {
        return PathBuf::from(root);
    }

    // Walk up from executable location
    if let Ok(exe) = std::env::current_exe() {
        let mut dir = exe.as_path();
        while let Some(parent) = dir.parent() {
            if parent.join("config/config.toml").exists() {
                return parent.to_owned();
            }
            dir = parent;
        }
    }

    // Fallback: current directory
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

fn default_config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("~/.config"))
        .join("pi-sandbox/config.toml")
}
