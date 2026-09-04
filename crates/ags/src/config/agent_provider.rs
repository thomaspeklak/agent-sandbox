use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::cli::Agent;

use super::{ArchiveMemberMatch, GitHubReleaseSource, ToolArchiveFormat};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BuiltinAgentInstaller {
    Claude,
    Codex,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum AgentProviderPolicy {
    Pnpm { package: String },
    BuiltinInstaller { installer: BuiltinAgentInstaller },
    GithubRelease { source: GitHubReleaseSource },
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LockedAgentProvider {
    pub agent: Agent,
    pub provider: AgentProviderPolicy,
}

pub(crate) fn validate_locked_agent_providers(
    providers: &[LockedAgentProvider],
    context: &str,
) -> Result<(), String> {
    let mut agents = BTreeSet::new();
    for (index, entry) in providers.iter().enumerate() {
        let item_context = format!("{context}[{index}]");
        if entry.agent == Agent::Shell {
            return Err(format!("{item_context}.agent must not be 'shell'"));
        }
        if !agents.insert(entry.agent) {
            return Err(format!(
                "{context} repeats agent '{}'",
                entry.agent.as_str()
            ));
        }
        validate_agent_provider(entry.agent, &entry.provider, &item_context)?;
    }
    Ok(())
}

pub(crate) fn validate_agent_provider(
    agent: Agent,
    provider: &AgentProviderPolicy,
    context: &str,
) -> Result<(), String> {
    match (agent, provider) {
        (Agent::Pi | Agent::Gemini, AgentProviderPolicy::Pnpm { package }) => {
            validate_pnpm_package(package, &format!("{context}.provider.package"))
        }
        (
            Agent::Claude,
            AgentProviderPolicy::BuiltinInstaller {
                installer: BuiltinAgentInstaller::Claude,
            },
        )
        | (
            Agent::Codex,
            AgentProviderPolicy::BuiltinInstaller {
                installer: BuiltinAgentInstaller::Codex,
            },
        ) => Ok(()),
        (Agent::Opencode, AgentProviderPolicy::GithubRelease { source }) => {
            super::validate_github_release_source(source)
                .map_err(|error| format!("{context}.provider.{error}"))?;
            if source.archive != ToolArchiveFormat::TarGz
                || source.member != "opencode"
                || source.member_match != ArchiveMemberMatch::Exact
                || source.install_as != "opencode"
            {
                return Err(format!(
                    "{context}.provider must install the exact member 'opencode' from tar.gz as 'opencode'"
                ));
            }
            Ok(())
        }
        (Agent::Shell, _) => Err(format!("{context}.agent must not be 'shell'")),
        _ => Err(format!(
            "{context}.provider is incompatible with agent '{}'",
            agent.as_str()
        )),
    }
}

pub(crate) fn validate_pnpm_package(package: &str, context: &str) -> Result<(), String> {
    let unscoped = package.strip_prefix('@').unwrap_or(package);
    let segments = unscoped.split('/').collect::<Vec<_>>();
    let valid_segment = |segment: &str| {
        !segment.is_empty()
            && !segment.starts_with(['.', '-'])
            && segment
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
    };
    let valid = if package.starts_with('@') {
        segments.len() == 2 && segments.iter().all(|segment| valid_segment(segment))
    } else {
        segments.len() == 1 && valid_segment(segments[0])
    };
    if !valid {
        return Err(format!("{context} must be an unversioned npm package name"));
    }
    Ok(())
}
