fn validate_sandbox(
    raw: &crate::config::raw::RawSandbox,
    tool_download_lock_config_path: &Path,
    agent_provider_lock_config_path: &Path,
) -> Result<ValidatedSandbox, ConfigError> {
    let tool_download_lock = if raw.tool_download_lock.trim().is_empty() {
        None
    } else {
        Some(expand_path_from_config(
            &raw.tool_download_lock,
            "[sandbox].tool_download_lock",
            tool_download_lock_config_path,
        )?)
    };
    // An omitted lock preserves the reviewed built-in default tools for backward
    // compatibility. A generated lock containing [] means the user explicitly
    // selected no downloaded tools.
    let tool_downloads = match tool_download_lock.as_deref() {
        Some(path) => load_tool_download_lock(path)?,
        None => load_default_tool_downloads()?,
    };
    let configured_agent_provider_lock = if raw.agent_provider_lock.trim().is_empty() {
        None
    } else {
        Some(expand_path_from_config(
            &raw.agent_provider_lock,
            "[sandbox].agent_provider_lock",
            agent_provider_lock_config_path,
        )?)
    };
    let legacy_agent_provider_lock = if raw.agent_release_source_lock.trim().is_empty() {
        None
    } else {
        Some(expand_path_from_config(
            &raw.agent_release_source_lock,
            "[sandbox].agent_release_source_lock",
            agent_provider_lock_config_path,
        )?)
    };
    if configured_agent_provider_lock.is_some() && legacy_agent_provider_lock.is_some() {
        return Err(ConfigError::Validation(
            "[sandbox] must not define both agent_provider_lock and the legacy agent_release_source_lock"
                .to_owned(),
        ));
    }
    let (agent_provider_lock, agent_providers) = match (
        configured_agent_provider_lock,
        legacy_agent_provider_lock,
    ) {
        (Some(path), None) => {
            let providers = load_agent_provider_lock(&path)?;
            (Some(path), providers)
        }
        (None, Some(path)) => {
            let providers = load_legacy_agent_provider_lock(&path)?;
            (Some(path), providers)
        }
        (None, None) => (None, load_default_agent_providers()?),
        (Some(_), Some(_)) => unreachable!("conflicting locks were rejected above"),
    };
    let enabled_agents = validate_enabled_agents(&raw.enabled_agents)?;
    for agent in &enabled_agents {
        if !agent_providers.iter().any(|entry| entry.agent == *agent) {
            return Err(ConfigError::Validation(format!(
                "[sandbox].enabled_agents includes '{}' but the agent provider lock does not",
                agent.as_str()
            )));
        }
    }
    Ok(ValidatedSandbox {
        image: require_non_empty(&raw.image, "[sandbox].image")?.to_owned(),
        containerfile: expand_path(&raw.containerfile, "[sandbox].containerfile")?,
        cache_dir: expand_path(&raw.cache_dir, "[sandbox].cache_dir")?,
        gitconfig_path: expand_path(&raw.gitconfig_path, "[sandbox].gitconfig_path")?,
        auth_key: expand_path(&raw.auth_key, "[sandbox].auth_key")?,
        sign_key: expand_path(&raw.sign_key, "[sandbox].sign_key")?,
        bootstrap_files: validate_string_list(&raw.bootstrap_files, "[sandbox].bootstrap_files")?,
        container_boot_dirs: validate_string_list(
            &raw.container_boot_dirs,
            "[sandbox].container_boot_dirs",
        )?,
        passthrough_env: validate_string_list(&raw.passthrough_env, "[sandbox].passthrough_env")?,
        enabled_agents,
        extra_dnf_packages: validate_dnf_packages(
            &raw.extra_dnf_packages,
            "[sandbox].extra_dnf_packages",
        )?,
        tool_download_lock,
        tool_downloads,
        agent_provider_lock,
        agent_providers,
    })
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct LegacyLockedAgentReleaseSource {
    agent: Agent,
    github_release: crate::config::GitHubReleaseSource,
}

fn load_legacy_agent_provider_lock(path: &Path) -> Result<Vec<LockedAgentProvider>, ConfigError> {
    let content = std::fs::read_to_string(path).map_err(|error| {
        ConfigError::Validation(format!(
            "[sandbox].agent_release_source_lock could not read '{}': {error}",
            path.display()
        ))
    })?;
    verify_content_addressed_lock(
        path,
        "agent-release-sources",
        "[sandbox].agent_release_source_lock",
        content.as_bytes(),
    )?;
    let sources = serde_json::from_str::<Vec<LegacyLockedAgentReleaseSource>>(&content).map_err(
        |error| {
            ConfigError::Validation(format!(
                "[sandbox].agent_release_source_lock contains invalid JSON in '{}': {error}",
                path.display()
            ))
        },
    )?;
    let [source] = sources.as_slice() else {
        return Err(ConfigError::Validation(
            "[sandbox].agent_release_source_lock must contain exactly the legacy OpenCode release source; run `ags tools` to create an agent provider lock"
                .to_owned(),
        ));
    };
    if source.agent != Agent::Opencode {
        return Err(ConfigError::Validation(
            "[sandbox].agent_release_source_lock must contain the legacy OpenCode release source; run `ags tools` to create an agent provider lock"
                .to_owned(),
        ));
    }
    let mut providers = load_default_agent_providers()?;
    let opencode = providers
        .iter_mut()
        .find(|entry| entry.agent == Agent::Opencode)
        .ok_or_else(|| {
            ConfigError::Validation(
                "embedded default agent provider lock does not include OpenCode".to_owned(),
            )
        })?;
    opencode.provider = crate::config::AgentProviderPolicy::GithubRelease {
        source: source.github_release.clone(),
    };
    crate::config::validate_locked_agent_providers(&providers, "legacy agent release source lock")
        .map_err(ConfigError::Validation)?;
    Ok(providers)
}

fn load_agent_provider_lock(path: &Path) -> Result<Vec<LockedAgentProvider>, ConfigError> {
    let content = std::fs::read_to_string(path).map_err(|error| {
        ConfigError::Validation(format!(
            "[sandbox].agent_provider_lock could not read '{}': {error}",
            path.display()
        ))
    })?;
    verify_required_content_addressed_lock(
        path,
        "agent-providers",
        "[sandbox].agent_provider_lock",
        content.as_bytes(),
    )?;
    let providers = serde_json::from_str::<Vec<LockedAgentProvider>>(&content).map_err(|error| {
            ConfigError::Validation(format!(
                "[sandbox].agent_provider_lock contains invalid JSON in '{}': {error}",
                path.display()
            ))
        })?;
    crate::config::validate_locked_agent_providers(&providers, "agent provider lock")
        .map_err(ConfigError::Validation)?;
    Ok(providers)
}

fn load_default_agent_providers() -> Result<Vec<LockedAgentProvider>, ConfigError> {
    let providers = serde_json::from_str::<Vec<LockedAgentProvider>>(
        crate::assets::DEFAULT_AGENT_PROVIDERS_LOCK,
    )
    .map_err(|error| {
        ConfigError::Validation(format!(
            "embedded default agent provider lock contains invalid JSON: {error}"
        ))
    })?;
    crate::config::validate_locked_agent_providers(
        &providers,
        "embedded default agent provider lock",
    )
    .map_err(ConfigError::Validation)?;
    Ok(providers)
}

fn validate_enabled_agents(list: &[String]) -> Result<Vec<Agent>, ConfigError> {
    let mut configured = Vec::new();
    for (index, value) in list.iter().enumerate() {
        let agent = Agent::from_id(value).ok_or_else(|| {
            ConfigError::Validation(format!(
                "[sandbox].enabled_agents[{index}] must be one of: pi, claude, codex, gemini, opencode"
            ))
        })?;
        if agent == Agent::Shell {
            return Err(ConfigError::Validation(format!(
                "[sandbox].enabled_agents[{index}] must not be 'shell'; shell is always available"
            )));
        }
        if configured.contains(&agent) {
            return Err(ConfigError::Validation(format!(
                "[sandbox].enabled_agents contains duplicate agent '{value}'"
            )));
        }
        configured.push(agent);
    }

    Ok(Agent::INSTALLABLE
        .into_iter()
        .filter(|agent| configured.contains(agent))
        .collect())
}

fn load_tool_download_lock(path: &Path) -> Result<Vec<LockedToolDownload>, ConfigError> {
    let content = std::fs::read_to_string(path).map_err(|error| ConfigError::Validation(format!(
        "[sandbox].tool_download_lock could not read '{}': {error}",
        path.display()
    )))?;
    verify_content_addressed_lock(
        path,
        "tool-downloads",
        "[sandbox].tool_download_lock",
        content.as_bytes(),
    )?;
    let downloads = serde_json::from_str::<Vec<LockedToolDownload>>(&content).map_err(|error| {
        ConfigError::Validation(format!(
            "[sandbox].tool_download_lock contains invalid JSON in '{}': {error}",
            path.display()
        ))
    })?;
    crate::config::validate_locked_tool_downloads(&downloads, "tool download lock")
        .map_err(ConfigError::Validation)?;
    Ok(downloads)
}

fn load_default_tool_downloads() -> Result<Vec<LockedToolDownload>, ConfigError> {
    let downloads = serde_json::from_str::<Vec<LockedToolDownload>>(
        crate::assets::DEFAULT_TOOL_DOWNLOADS_LOCK,
    )
    .map_err(|error| {
        ConfigError::Validation(format!(
            "embedded default tool download lock contains invalid JSON: {error}"
        ))
    })?;
    crate::config::validate_locked_tool_downloads(&downloads, "embedded default tool download lock")
        .map_err(ConfigError::Validation)?;
    Ok(downloads)
}

fn verify_content_addressed_lock(
    path: &Path,
    prefix: &str,
    context: &str,
    content: &[u8],
) -> Result<(), ConfigError> {
    let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
        return Ok(());
    };
    let Some(digest) = file_name
        .strip_prefix(&format!("{prefix}."))
        .and_then(|name| name.strip_suffix(".lock.json"))
        .filter(|digest| digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit()))
    else {
        return Ok(());
    };
    let actual = format!("{:x}", <sha2::Sha256 as sha2::Digest>::digest(content));
    if !digest.eq_ignore_ascii_case(&actual) {
        return Err(ConfigError::Validation(format!(
            "{context} content digest does not match '{}': expected {digest}, got {actual}",
            path.display()
        )));
    }
    Ok(())
}

fn verify_required_content_addressed_lock(
    path: &Path,
    prefix: &str,
    context: &str,
    content: &[u8],
) -> Result<(), ConfigError> {
    let digest = path
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.strip_prefix(&format!("{prefix}.")))
        .and_then(|name| name.strip_suffix(".lock.json"))
        .filter(|digest| digest.len() == 64 && digest.bytes().all(|byte| byte.is_ascii_hexdigit()))
        .ok_or_else(|| {
            ConfigError::Validation(format!(
                "{context} must reference a content-addressed {prefix}.<sha256>.lock.json file"
            ))
        })?;
    let actual = format!("{:x}", <sha2::Sha256 as sha2::Digest>::digest(content));
    if !digest.eq_ignore_ascii_case(&actual) {
        return Err(ConfigError::Validation(format!(
            "{context} content digest does not match '{}': expected {digest}, got {actual}",
            path.display()
        )));
    }
    Ok(())
}

fn require_non_empty<'a>(s: &'a str, ctx: &str) -> Result<&'a str, ConfigError> {
    if s.trim().is_empty() {
        return Err(ConfigError::Validation(format!(
            "{ctx} must be a non-empty string"
        )));
    }
    Ok(s)
}

fn validate_string_list(list: &[String], ctx: &str) -> Result<Vec<String>, ConfigError> {
    for (i, s) in list.iter().enumerate() {
        require_non_empty(s, &format!("{ctx}[{i}]"))?;
    }
    Ok(list.to_vec())
}

fn parse_mode(s: &str, ctx: &str) -> Result<MountMode, ConfigError> {
    match s.to_lowercase().as_str() {
        "ro" => Ok(MountMode::Ro),
        "rw" => Ok(MountMode::Rw),
        _ => Err(ConfigError::Validation(format!(
            "{ctx} must be 'ro' or 'rw'"
        ))),
    }
}

fn parse_kind(s: &str, ctx: &str) -> Result<MountKind, ConfigError> {
    match s.to_lowercase().as_str() {
        "dir" => Ok(MountKind::Dir),
        "file" => Ok(MountKind::File),
        _ => Err(ConfigError::Validation(format!(
            "{ctx} must be 'dir' or 'file'"
        ))),
    }
}

fn parse_when(s: &str, ctx: &str) -> Result<MountWhen, ConfigError> {
    match s.to_lowercase().as_str() {
        "always" => Ok(MountWhen::Always),
        "browser" => Ok(MountWhen::Browser),
        _ => Err(ConfigError::Validation(format!(
            "{ctx} must be 'always' or 'browser'"
        ))),
    }
}

fn expand_path(raw: &str, ctx: &str) -> Result<PathBuf, ConfigError> {
    let after_tilde = expand_tilde(raw)?;
    let after_vars = expand_env_vars(&after_tilde);
    let path = PathBuf::from(&after_vars);
    std::path::absolute(&path)
        .map_err(|e| ConfigError::Validation(format!("{ctx}: failed to resolve path '{raw}': {e}")))
}

fn expand_path_from_config(
    raw: &str,
    ctx: &str,
    config_path: &Path,
) -> Result<PathBuf, ConfigError> {
    let after_tilde = expand_tilde(raw)?;
    let after_vars = expand_env_vars(&after_tilde);
    let path = PathBuf::from(&after_vars);
    let path = if path.is_absolute() {
        path
    } else {
        config_path.parent().unwrap_or(Path::new(".")).join(path)
    };
    std::path::absolute(&path)
        .map_err(|e| ConfigError::Validation(format!("{ctx}: failed to resolve path '{raw}': {e}")))
}

fn expand_tilde(raw: &str) -> Result<String, ConfigError> {
    if let Some(rest) = raw.strip_prefix('~') {
        if rest.is_empty() || rest.starts_with('/') {
            let home = dirs::home_dir()
                .ok_or_else(|| ConfigError::Validation("cannot determine home directory".into()))?;
            Ok(format!("{}{rest}", home.display()))
        } else {
            // ~user form not supported, pass through
            Ok(raw.to_owned())
        }
    } else {
        Ok(raw.to_owned())
    }
}

/// Expand `$VAR` and `${VAR}` references. Undefined variables are left as-is
/// (matching Python `os.path.expandvars` behavior).
fn expand_env_vars(input: &str) -> String {
    if !input.contains('$') {
        return input.to_owned();
    }

    let mut result = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch != '$' {
            result.push(ch);
            continue;
        }

        match chars.peek().copied() {
            Some('{') => {
                chars.next();
                let mut name = String::new();
                let mut closed = false;
                for c in chars.by_ref() {
                    if c == '}' {
                        closed = true;
                        break;
                    }
                    name.push(c);
                }
                if closed {
                    match std::env::var(&name) {
                        Ok(val) => result.push_str(&val),
                        Err(_) => {
                            result.push_str("${");
                            result.push_str(&name);
                            result.push('}');
                        }
                    }
                } else {
                    result.push_str("${");
                    result.push_str(&name);
                }
            }
            Some(c) if c.is_ascii_alphabetic() || c == '_' => {
                let mut name = String::new();
                while let Some(&c) = chars.peek() {
                    if c.is_ascii_alphanumeric() || c == '_' {
                        name.push(c);
                        chars.next();
                    } else {
                        break;
                    }
                }
                match std::env::var(&name) {
                    Ok(val) => result.push_str(&val),
                    Err(_) => {
                        result.push('$');
                        result.push_str(&name);
                    }
                }
            }
            _ => {
                result.push('$');
            }
        }
    }

    result
}
