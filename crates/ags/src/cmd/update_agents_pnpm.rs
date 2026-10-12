use crate::cli::Agent;
use crate::config::{AgentProviderPolicy, LockedAgentProvider};
use crate::util::shell_quote;

pub(super) fn package<'a>(
    agent: Agent,
    pi_spec: &'a str,
    providers: &'a [LockedAgentProvider],
) -> Result<&'a str, String> {
    let AgentProviderPolicy::Pnpm { package } = super::require_provider(agent, providers)? else {
        return Err(format!("{agent} requires a pnpm provider"));
    };
    let package = if agent == Agent::Pi {
        super::resolve_pi_spec(pi_spec)
    } else {
        package
    };
    crate::config::validate_pnpm_package(package, "pnpm agent package")?;
    Ok(package)
}

pub(super) fn actions(
    pi_spec: &str,
    enabled: &[Agent],
    providers: &[LockedAgentProvider],
) -> Result<(String, String, String), String> {
    let agents = [Agent::Pi, Agent::Gemini, Agent::T3];
    let selected = agents
        .iter()
        .filter(|agent| enabled.contains(agent))
        .map(|agent| Ok((*agent, package(*agent, pi_spec, providers)?)))
        .collect::<Result<Vec<_>, String>>()?;
    let protected = selected
        .iter()
        .map(|(_, package)| format!(" {}", shell_quote(package)))
        .collect::<String>();
    let install = selected
        .iter()
        .map(|(agent, package)| format!("install_pnpm_candidate {agent} {}", shell_quote(package)))
        .collect::<Vec<_>>()
        .join("\n");
    let mut cleanup = Vec::new();
    for agent in agents {
        if let Some((_, package)) = selected.iter().find(|(id, _)| *id == agent) {
            cleanup.push(format!(
                "commit_pnpm_agent {agent} {}{protected}",
                shell_quote(package)
            ));
        } else {
            cleanup.push(format!(
                "remove_pnpm_agents_for_bin_except {agent}{protected}"
            ));
            cleanup.push(format!(
                "rm -f /usr/local/pnpm/{agent} /usr/local/pnpm/bin/{agent}"
            ));
        }
    }
    let verify = selected
        .iter()
        .map(|(agent, package)| format!("verify_pnpm_agent {} {agent}", shell_quote(package)))
        .collect::<Vec<_>>()
        .join("\n");
    Ok((install, cleanup.join("\n"), verify))
}
