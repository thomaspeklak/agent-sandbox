use std::fmt;
use std::fs;
use std::path::Path;
use std::process::Command;

use crate::cli::Agent;
use crate::config::{
    AgentProviderPolicy, LockedAgentProvider, ToolDownloadSource, ValidatedConfig,
};
use crate::github_release::resolve_github_release_source;
use crate::util::shell_quote;

#[path = "update_agents_identity.rs"]
mod identity;
#[path = "update_agents_script.rs"]
mod script;
use script::{build_install_script, resolve_pi_spec};

/// Options for the update-agents command.
#[derive(Default)]
pub struct UpdateAgentsOptions {
    pub pi_spec: Option<String>,
    pub minimum_release_age: Option<u32>,
}

#[derive(Debug)]
pub enum UpdateAgentsError {
    HostDirCreate(String),
    MissingProvider(String),
    RecoveryFailed(String),
    ReleaseResolveFailed(String),
    InstallFailed(String),
}

impl fmt::Display for UpdateAgentsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::HostDirCreate(msg) => write!(f, "failed to create host directory: {msg}"),
            Self::MissingProvider(agent) => write!(
                f,
                "enabled agent '{agent}' has no provider; run `ags tools` to save the agent catalog"
            ),
            Self::RecoveryFailed(msg) => write!(f, "failed to recover OpenCode update: {msg}"),
            Self::ReleaseResolveFailed(msg) => {
                write!(f, "failed to resolve agent release: {msg}")
            }
            Self::InstallFailed(msg) => write!(f, "agent install failed: {msg}"),
        }
    }
}

impl std::error::Error for UpdateAgentsError {}

/// Reconcile selected agents in persistent volumes via a throwaway container.
pub fn run(config: &ValidatedConfig, opts: &UpdateAgentsOptions) -> Result<(), UpdateAgentsError> {
    let image =
        identity::image_id(&config.sandbox.image).map_err(UpdateAgentsError::InstallFailed)?;
    let generation = crate::agent_runtime::Update::begin(&config.sandbox.cache_dir)
        .map_err(|error| UpdateAgentsError::InstallFailed(error.to_string()))?;
    let result = run_candidate(config, opts, &image, &generation);
    if result.is_err() {
        match generation.cleanup_after_failure() {
            Ok(report) => print_cleanup_report(report),
            Err(error) => eprintln!(
                "warning: agent update failed and incomplete-runtime cleanup did not complete: {error}"
            ),
        }
    }
    result
}

fn run_candidate(
    config: &ValidatedConfig,
    opts: &UpdateAgentsOptions,
    image: &str,
    generation: &crate::agent_runtime::Update,
) -> Result<(), UpdateAgentsError> {
    let cache_dir = &config.sandbox.cache_dir;
    let enabled_agents = &config.sandbox.enabled_agents;
    let selected = crate::agent_runtime::selected(cache_dir)
        .map_err(|error| UpdateAgentsError::InstallFailed(error.to_string()))?;
    println!("Selected runtime: {}", selected.display());
    let uses = crate::agent_runtime::inspect_usage(cache_dir).map_err(|error| {
        UpdateAgentsError::InstallFailed(format!("cannot inspect runtime usage: {error}"))
    })?;
    if uses.is_empty() {
        println!("No existing containers reference runtime directories in this cache.");
    }
    for (container, roots) in &uses {
        for root in roots {
            println!(
                "  retained: {} — {} ({})",
                root.display(),
                container.name,
                container.state.status
            );
        }
    }
    println!("Building runtime candidate: {}", generation.path.display());
    let pnpm_home = generation.path.join("pnpm-home");
    let codex_install = generation.path.join("codex-install");
    let opencode_install = generation.path.join("opencode-install");
    let claude_install = generation.path.join("claude-install");
    // Isolate legacy-shim cleanup too; never mutate the shared npm user cache.
    let npm_global = generation.path.join("npm-global");

    for dir in [
        &pnpm_home,
        &codex_install,
        &opencode_install,
        &claude_install,
        &npm_global,
    ] {
        fs::create_dir_all(dir).map_err(|error| {
            UpdateAgentsError::HostDirCreate(format!("{}: {error}", dir.display()))
        })?;
    }

    let release_age = opts
        .minimum_release_age
        .unwrap_or(config.update.minimum_release_age);
    let opencode_download = resolve_opencode_with_recovery(
        &opencode_install,
        enabled_agents,
        &config.sandbox.agent_providers,
        release_age,
        resolve_github_release_source,
    )?;
    let configured_pi_spec = opts.pi_spec.as_deref().unwrap_or(&config.update.pi_spec);
    let configured_pi_spec = resolve_pi_spec(configured_pi_spec);
    let pi_spec = if configured_pi_spec == crate::config::DEFAULT_PI_SPEC {
        provider_for(Agent::Pi, &config.sandbox.agent_providers)
            .and_then(|provider| match provider {
                AgentProviderPolicy::Pnpm { package } => Some(package.as_str()),
                _ => None,
            })
            .unwrap_or(configured_pi_spec)
    } else {
        configured_pi_spec
    };
    let install_script = build_install_script(
        pi_spec,
        release_age,
        enabled_agents,
        &config.sandbox.agent_providers,
        opencode_download.as_ref(),
    )
    .map_err(UpdateAgentsError::InstallFailed)?;

    let mut verification_script =
        String::from("set -e\nexport DISABLE_AUTOUPDATER=1 OPENCODE_DISABLE_AUTOUPDATE=true\n");
    for agent in enabled_agents {
        let launcher = match agent {
            Agent::Pi => "/usr/local/pnpm/bin/pi",
            Agent::Gemini => "/usr/local/pnpm/bin/gemini",
            Agent::Codex => "/usr/local/pnpm/codex",
            Agent::Claude => "/opt/claude-home/.local/bin/claude",
            Agent::Opencode => "/opt/opencode-home/.opencode/bin/opencode",
            Agent::Shell => continue,
        };
        verification_script.push_str(&format!("timeout 60 {} --version\n", shell_quote(launcher)));
    }

    println!("Checking agent updates in an isolated runtime candidate...");
    println!(
        "  enabled: {}",
        agent_list(enabled_agents).unwrap_or_else(|| "none (shell only)".to_owned())
    );
    let disabled_agents = Agent::INSTALLABLE
        .into_iter()
        .filter(|agent| !enabled_agents.contains(agent))
        .collect::<Vec<_>>();
    if let Some(disabled) = agent_list(&disabled_agents) {
        println!("  omitted from new generation: {disabled}");
    }
    if pi_spec == opts.pi_spec.as_deref().unwrap_or(&config.update.pi_spec) {
        println!("  PI spec: {pi_spec}");
    } else {
        println!(
            "  PI spec: {pi_spec} (resolved from {})",
            opts.pi_spec.as_deref().unwrap_or(&config.update.pi_spec)
        );
    }
    println!("  minimum release age: {release_age} minutes");
    if let Some(download) = &opencode_download {
        println!("  OpenCode release: v{}", download.version);
    }

    let mut run_args = build_podman_run_args(
        image,
        &pnpm_home,
        &codex_install,
        &opencode_install,
        &claude_install,
        &npm_global,
        &install_script,
    );
    // Cache downloads across checks, but import via reflinks/copies: never link a
    // live runtime to the package manager's writable content store.
    let store = cache_dir.join("agent-downloads/pnpm-store");
    let pnpm_cache = cache_dir.join("agent-downloads/pnpm-cache");
    fs::create_dir_all(&store)
        .and_then(|_| fs::create_dir_all(&pnpm_cache))
        .map_err(|error| UpdateAgentsError::HostDirCreate(error.to_string()))?;
    let mut install_args = run_args.clone();
    let image_index = install_args.len() - 4;
    install_args.splice(
        image_index..image_index,
        [
            "-v".to_owned(),
            format!("{}:/var/cache/ags/agent-pnpm-store:rw", store.display()),
            "-v".to_owned(),
            format!(
                "{}:/var/cache/ags/agent-pnpm-cache:rw",
                pnpm_cache.display()
            ),
        ],
    );
    let status = Command::new("podman")
        .args(&install_args)
        .status()
        .map_err(|error| UpdateAgentsError::InstallFailed(error.to_string()))?;

    if !status.success() {
        return Err(UpdateAgentsError::InstallFailed(format!(
            "exited with {status}"
        )));
    }

    // Verify from a fresh container with read-only mounts, matching normal launches.
    // A CLI that requires writes to its install directory must not be published.
    for index in 1..run_args.len() {
        if run_args[index - 1] == "-v" {
            run_args[index] = format!("{}:ro", run_args[index].strip_suffix(":rw").unwrap());
        }
    }
    // Only the inventory output is writable during verification, never runtime files.
    let inventory_dir =
        tempfile::tempdir().map_err(|error| UpdateAgentsError::InstallFailed(error.to_string()))?;
    let image_index = run_args.len() - 4;
    run_args.splice(
        image_index..image_index,
        [
            "--network=none".to_owned(),
            "-v".to_owned(),
            format!("{}:/run/ags-update:rw", inventory_dir.path().display()),
        ],
    );
    verification_script.push_str(&identity::inventory_script(enabled_agents));
    *run_args.last_mut().unwrap() = verification_script;
    let status = Command::new("podman")
        .args(&run_args)
        .status()
        .map_err(|error| UpdateAgentsError::InstallFailed(error.to_string()))?;
    if !status.success() {
        return Err(UpdateAgentsError::InstallFailed(format!(
            "runtime verification exited with {status}"
        )));
    }
    let inventory = fs::read(inventory_dir.path().join("inventory.json")).map_err(|error| {
        UpdateAgentsError::InstallFailed(format!("cannot read runtime inventory: {error}"))
    })?;
    let manifest = crate::agent_runtime::RuntimeManifest::from_inventory(
        image.to_owned(),
        identity::request_identity(config, pi_spec),
        &inventory,
    )
    .map_err(|error| {
        UpdateAgentsError::InstallFailed(format!("invalid runtime inventory: {error}"))
    })?;
    match generation.finish(manifest).map_err(|error| {
        UpdateAgentsError::InstallFailed(format!("cannot publish runtime: {error}"))
    })? {
        crate::agent_runtime::Publication::Unchanged => {
            println!("\nAlready up to date. Current and previous generations are unchanged.");
        }
        crate::agent_runtime::Publication::Published(shared) => {
            println!(
                "\nDone. New sandboxes will use {}.",
                generation.path.display()
            );
            println!(
                "Shared {} unchanged files ({} bytes of file content).",
                shared.files, shared.bytes
            );
        }
    }
    println!(
        "Existing sandboxes keep their runtimes; latest, previous, and in-use generations are retained."
    );
    match generation.cleanup() {
        Ok(report) => print_cleanup_report(report),
        Err(error) => {
            eprintln!("warning: runtime update succeeded but cleanup did not complete: {error}")
        }
    }
    if let Some(agent) = enabled_agents.first() {
        println!(
            "Verify with: {}",
            verification_command(*agent, &config.config_file)
        );
    } else {
        println!("No agent CLIs are enabled; `ags --agent shell` remains available.");
    }
    Ok(())
}

fn print_cleanup_report(report: crate::agent_runtime::CleanupReport) {
    for path in report.removed {
        println!("  cleaned: {}", path.display());
    }
    for path in report.retained {
        println!("  kept: {}", path.display());
    }
}

fn verification_command(agent: Agent, config_file: &std::path::Path) -> String {
    format!(
        "ags --agent {} --config {} -- --version",
        agent.as_str(),
        shell_quote(&config_file.display().to_string())
    )
}

fn agent_list(agents: &[Agent]) -> Option<String> {
    (!agents.is_empty()).then(|| {
        agents
            .iter()
            .map(|agent| agent.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    })
}

fn provider_for(agent: Agent, providers: &[LockedAgentProvider]) -> Option<&AgentProviderPolicy> {
    providers
        .iter()
        .find(|entry| entry.agent == agent)
        .map(|entry| &entry.provider)
}

fn resolve_opencode_with_recovery<F>(
    opencode_install: &Path,
    enabled_agents: &[Agent],
    providers: &[LockedAgentProvider],
    release_age: u32,
    resolver: F,
) -> Result<Option<ToolDownloadSource>, UpdateAgentsError>
where
    F: FnOnce(
        &crate::config::GitHubReleaseSource,
        u32,
    ) -> Result<ToolDownloadSource, crate::github_release::GitHubReleaseError>,
{
    recover_opencode_transaction(opencode_install)?;
    if !enabled_agents.contains(&Agent::Opencode) {
        return Ok(None);
    }
    let provider = provider_for(Agent::Opencode, providers)
        .ok_or_else(|| UpdateAgentsError::MissingProvider("opencode".to_owned()))?;
    let AgentProviderPolicy::GithubRelease { source } = provider else {
        return Err(UpdateAgentsError::MissingProvider("opencode".to_owned()));
    };
    resolver(source, release_age)
        .map(Some)
        .map_err(|error| UpdateAgentsError::ReleaseResolveFailed(error.to_string()))
}

fn recover_opencode_transaction(root: &Path) -> Result<(), UpdateAgentsError> {
    let active = root.join(".opencode");
    let stage = root.join(".opencode.stage");
    let backup = root.join(".opencode.previous");
    let transaction = root.join(".opencode.transaction");
    let has_transaction = path_exists(&transaction)?;
    let has_active = path_exists(&active)?;
    let has_backup = path_exists(&backup)?;

    if has_transaction {
        remove_entry(&active)?;
        if has_backup {
            restore_backup(&backup, &active)?;
        }
        remove_entry(&transaction)?;
    } else if !has_active && has_backup {
        restore_backup(&backup, &active)?;
    } else if has_active && has_backup {
        remove_entry(&backup)?;
    }
    remove_entry(&stage)?;
    Ok(())
}

fn path_exists(path: &Path) -> Result<bool, UpdateAgentsError> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(recovery_error(path, error)),
    }
}

fn remove_entry(path: &Path) -> Result<(), UpdateAgentsError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(recovery_error(path, error)),
    };
    let result = if metadata.is_dir() && !metadata.file_type().is_symlink() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    };
    result.map_err(|error| recovery_error(path, error))
}

fn restore_backup(backup: &Path, active: &Path) -> Result<(), UpdateAgentsError> {
    let metadata = fs::symlink_metadata(backup).map_err(|error| recovery_error(backup, error))?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(UpdateAgentsError::RecoveryFailed(format!(
            "{} is not a regular directory",
            backup.display()
        )));
    }
    fs::rename(backup, active).map_err(|error| recovery_error(backup, error))
}

fn recovery_error(path: &Path, error: std::io::Error) -> UpdateAgentsError {
    UpdateAgentsError::RecoveryFailed(format!("{}: {error}", path.display()))
}

fn build_podman_run_args(
    image: &str,
    pnpm_home: &Path,
    codex_install: &Path,
    opencode_install: &Path,
    claude_install: &Path,
    npm_global: &Path,
    script: &str,
) -> Vec<String> {
    let generation = pnpm_home
        .parent()
        .expect("pnpm runtime directory has a generation parent");
    let leased_script = format!(
        "exec 9</run/ags-update-candidate/.installing\nflock --shared 9 || exit 1\n{script}"
    );
    vec![
        "run".to_owned(),
        "--rm".to_owned(),
        "-it".to_owned(),
        "--userns=keep-id".to_owned(),
        "--security-opt=label=disable".to_owned(),
        "-v".to_owned(),
        format!("{}:/usr/local/pnpm:rw", pnpm_home.display()),
        "-v".to_owned(),
        format!("{}:/opt/codex-home:rw", codex_install.display()),
        "-v".to_owned(),
        format!("{}:/opt/opencode-home:rw", opencode_install.display()),
        "-v".to_owned(),
        format!("{}:/opt/claude-home:rw", claude_install.display()),
        "-v".to_owned(),
        format!("{}:/home/dev/.npm-global:rw", npm_global.display()),
        "-v".to_owned(),
        format!(
            "{}:/run/ags-update-candidate/.installing:rw",
            generation.join(".installing").display()
        ),
        image.to_owned(),
        "bash".to_owned(),
        "-c".to_owned(),
        leased_script,
    ]
}

#[cfg(test)]
#[path = "update_agents_tests.rs"]
mod tests;
