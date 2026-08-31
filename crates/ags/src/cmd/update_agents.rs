use std::fmt;
use std::fs;
use std::process::Command;

use crate::cli::Agent;
use crate::config::ValidatedConfig;
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
    MissingReleaseSource(String),
    ReleaseResolveFailed(String),
    InstallFailed(String),
}

impl fmt::Display for UpdateAgentsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::HostDirCreate(msg) => write!(f, "failed to create host directory: {msg}"),
            Self::MissingReleaseSource(agent) => write!(
                f,
                "enabled agent '{agent}' has no release source; run `ags tools` to save the agent catalog"
            ),
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

    let configured_pi_spec = opts.pi_spec.as_deref().unwrap_or(&config.update.pi_spec);
    let pi_spec = resolve_pi_spec(configured_pi_spec);
    let release_age = opts
        .minimum_release_age
        .unwrap_or(config.update.minimum_release_age);
    let opencode_download = if enabled_agents.contains(&Agent::Opencode) {
        let source = config
            .sandbox
            .agent_release_sources
            .iter()
            .find(|source| source.agent == Agent::Opencode)
            .ok_or_else(|| UpdateAgentsError::MissingReleaseSource("opencode".to_owned()))?;
        Some(
            resolve_github_release_source(&source.github_release, release_age)
                .map_err(|error| UpdateAgentsError::ReleaseResolveFailed(error.to_string()))?,
        )
    } else {
        None
    };
    let install_script = build_install_script(
        pi_spec,
        release_age,
        enabled_agents,
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
    if pi_spec == configured_pi_spec {
        println!("  PI spec: {pi_spec}");
    } else {
        println!("  PI spec: {pi_spec} (migrated from legacy {configured_pi_spec})");
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

fn build_podman_run_args(
    image: &str,
    pnpm_home: &std::path::Path,
    codex_install: &std::path::Path,
    opencode_install: &std::path::Path,
    claude_install: &std::path::Path,
    npm_global: &std::path::Path,
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
