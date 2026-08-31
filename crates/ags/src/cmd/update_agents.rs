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
    let cache_dir = &config.sandbox.cache_dir;
    let image = &config.sandbox.image;
    let enabled_agents = &config.sandbox.enabled_agents;

    let pnpm_home = cache_dir.join("pnpm-home");
    let codex_install = cache_dir.join("codex-install");
    let opencode_install = cache_dir.join("opencode-install");
    let claude_install = cache_dir.join("claude-install");
    let npm_global = cache_dir.join("npm-global");

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

    println!("Reconciling agent CLIs in persistent volumes...");
    println!(
        "  enabled: {}",
        agent_list(enabled_agents).unwrap_or_else(|| "none (shell only)".to_owned())
    );
    let disabled_agents = Agent::INSTALLABLE
        .into_iter()
        .filter(|agent| !enabled_agents.contains(agent))
        .collect::<Vec<_>>();
    if let Some(disabled) = agent_list(&disabled_agents) {
        println!("  removing: {disabled}");
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

    let status = Command::new("podman")
        .args(build_podman_run_args(
            image,
            &pnpm_home,
            &codex_install,
            &opencode_install,
            &claude_install,
            &npm_global,
            &install_script,
        ))
        .status()
        .map_err(|error| UpdateAgentsError::InstallFailed(error.to_string()))?;

    if !status.success() {
        return Err(UpdateAgentsError::InstallFailed(format!(
            "exited with {status}"
        )));
    }

    println!("\nDone. Agent CLI volumes reconciled.");
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
        image.to_owned(),
        "bash".to_owned(),
        "-c".to_owned(),
        script.to_owned(),
    ]
}

#[cfg(test)]
#[path = "update_agents_tests.rs"]
mod tests;
