use std::path::{Path, PathBuf};

use crate::config::error::ConfigError;
use crate::config::raw::{RawBrowser, RawConfig, RawMount, RawSecret, RawTool};
use crate::config::types::{
    BrowserConfig, MountKind, MountMode, MountWhen, SecretSource, UpdateConfig, ValidatedConfig,
    ValidatedMount, ValidatedSandbox, ValidatedSecret, ValidatedTool,
};

/// Read, parse, and validate a config TOML file from disk.
pub fn parse_and_validate(path: &Path) -> Result<ValidatedConfig, ConfigError> {
    let content = std::fs::read_to_string(path).map_err(|e| ConfigError::Io {
        path: path.to_owned(),
        source: e,
    })?;
    parse_toml_str(&content, path)
}

/// Parse and validate config from a TOML string (useful for testing).
pub fn parse_toml_str(content: &str, config_path: &Path) -> Result<ValidatedConfig, ConfigError> {
    let raw: RawConfig = toml::from_str(content).map_err(|e| ConfigError::Toml {
        path: config_path.to_owned(),
        source: e,
    })?;
    validate(raw, config_path)
}

fn validate(raw: RawConfig, config_path: &Path) -> Result<ValidatedConfig, ConfigError> {
    let sandbox = validate_sandbox(&raw.sandbox)?;

    let mut mounts = Vec::new();
    for (idx, m) in raw.mount.iter().enumerate() {
        mounts.push(validate_mount(m, &format!("[[mount]] #{idx}"))?);
    }

    let mut secrets = Vec::new();
    for (idx, s) in raw.secret.iter().enumerate() {
        secrets.extend(validate_secret(s, &format!("[[secret]] #{idx}"))?);
    }

    let mut tools = Vec::new();
    for (idx, t) in raw.tool.iter().enumerate() {
        let ctx = format!("[[tool]] #{idx}");
        let (tool, extra_mounts, extra_secrets) = validate_tool(t, &ctx)?;
        tools.push(tool);
        mounts.extend(extra_mounts);
        secrets.extend(extra_secrets);
    }

    let browser = validate_browser(&raw.browser)?;

    Ok(ValidatedConfig {
        config_file: config_path.to_owned(),
        sandbox,
        mounts,
        tools,
        secrets,
        browser,
        update: UpdateConfig {
            pi_spec: raw.update.pi_spec,
            minimum_release_age: raw.update.minimum_release_age,
        },
    })
}

fn validate_sandbox(raw: &crate::config::raw::RawSandbox) -> Result<ValidatedSandbox, ConfigError> {
    Ok(ValidatedSandbox {
        image: require_non_empty(&raw.image, "[sandbox].image")?.to_owned(),
        containerfile: expand_path(&raw.containerfile, "[sandbox].containerfile")?,
        sandbox_pi_dir: expand_path(&raw.sandbox_pi_dir, "[sandbox].sandbox_pi_dir")?,
        host_pi_dir: expand_path(&raw.host_pi_dir, "[sandbox].host_pi_dir")?,
        host_claude_dir: expand_path(&raw.host_claude_dir, "[sandbox].host_claude_dir")?,
        cache_dir: expand_path(&raw.cache_dir, "[sandbox].cache_dir")?,
        gitconfig_path: expand_path(&raw.gitconfig_path, "[sandbox].gitconfig_path")?,
        auth_key: expand_path(&raw.auth_key, "[sandbox].auth_key")?,
        sign_key: expand_path(&raw.sign_key, "[sandbox].sign_key")?,
        agent_sandbox_base: expand_path(&raw.agent_sandbox_base, "[sandbox].agent_sandbox_base")?,
        bootstrap_files: validate_string_list(&raw.bootstrap_files, "[sandbox].bootstrap_files")?,
        container_boot_dirs: validate_string_list(
            &raw.container_boot_dirs,
            "[sandbox].container_boot_dirs",
        )?,
        passthrough_env: validate_string_list(&raw.passthrough_env, "[sandbox].passthrough_env")?,
    })
}

fn validate_mount(raw: &RawMount, ctx: &str) -> Result<ValidatedMount, ConfigError> {
    Ok(ValidatedMount {
        host: expand_path(&raw.host, &format!("{ctx}.host"))?,
        container: require_non_empty(&raw.container, &format!("{ctx}.container"))?.to_owned(),
        mode: parse_mode(&raw.mode, &format!("{ctx}.mode"))?,
        kind: parse_kind(&raw.kind, &format!("{ctx}.kind"))?,
        when: parse_when(&raw.when, &format!("{ctx}.when"))?,
        create: raw.create,
        optional: raw.optional,
        source: raw.source.clone(),
    })
}

fn validate_secret(raw: &RawSecret, ctx: &str) -> Result<Vec<ValidatedSecret>, ConfigError> {
    let env = require_non_empty(&raw.env, &format!("{ctx}.env"))?;
    let mut out = Vec::new();

    if let Some(from_env) = &raw.from_env {
        let from_env = require_non_empty(from_env, &format!("{ctx}.from_env"))?;
        out.push(ValidatedSecret {
            env: env.to_owned(),
            source: SecretSource::Env {
                from_env: from_env.to_owned(),
            },
            origin: ctx.to_owned(),
            tool: None,
        });
    }

    if let Some(store) = &raw.secret_store {
        if store.is_empty() {
            return Err(ConfigError::Validation(format!(
                "{ctx}.secret_store must include at least one lookup attribute"
            )));
        }
        out.push(ValidatedSecret {
            env: env.to_owned(),
            source: SecretSource::SecretTool {
                attributes: store.clone(),
            },
            origin: ctx.to_owned(),
            tool: None,
        });
    }

    // Legacy provider form
    if let Some(provider) = &raw.provider {
        match provider.to_lowercase().as_str() {
            "env" => {
                let var = raw.var.as_deref().unwrap_or(env);
                out.push(ValidatedSecret {
                    env: env.to_owned(),
                    source: SecretSource::Env {
                        from_env: var.to_owned(),
                    },
                    origin: ctx.to_owned(),
                    tool: None,
                });
            }
            "secret-tool" => {
                let attrs = raw.attributes.as_ref().ok_or_else(|| {
                    ConfigError::Validation(format!(
                        "{ctx}.attributes required for secret-tool provider"
                    ))
                })?;
                if attrs.is_empty() {
                    return Err(ConfigError::Validation(format!(
                        "{ctx}.attributes must include at least one lookup attribute"
                    )));
                }
                out.push(ValidatedSecret {
                    env: env.to_owned(),
                    source: SecretSource::SecretTool {
                        attributes: attrs.clone(),
                    },
                    origin: ctx.to_owned(),
                    tool: None,
                });
            }
            other => {
                return Err(ConfigError::Validation(format!(
                    "{ctx}.provider must be 'env' or 'secret-tool', got '{other}'"
                )));
            }
        }
    }

    if out.is_empty() {
        return Err(ConfigError::Validation(format!(
            "{ctx} must define at least one source: from_env, secret_store, or provider"
        )));
    }

    Ok(out)
}

fn validate_tool(
    raw: &RawTool,
    ctx: &str,
) -> Result<(ValidatedTool, Vec<ValidatedMount>, Vec<ValidatedSecret>), ConfigError> {
    let name = require_non_empty(&raw.name, &format!("{ctx}.name"))?;
    let path = expand_path(&raw.path, &format!("{ctx}.path"))?;
    let container_path = require_non_empty(&raw.container_path, &format!("{ctx}.container_path"))?;
    let mode = parse_mode(&raw.mode, &format!("{ctx}.mode"))?;
    let when = parse_when(&raw.when, &format!("{ctx}.when"))?;

    let tool = ValidatedTool {
        name: name.to_owned(),
        path: path.clone(),
        container_path: container_path.to_owned(),
        mode,
        when,
        optional: raw.optional,
    };

    // Tool binary mount
    let mut mounts = vec![ValidatedMount {
        host: path,
        container: container_path.to_owned(),
        mode,
        kind: MountKind::File,
        when,
        create: false,
        optional: raw.optional,
        source: format!("tool:{name}:binary"),
    }];

    for (didx, dir) in raw.directory.iter().enumerate() {
        let dctx = format!("{ctx}.directory[{didx}]");
        let mut m = validate_mount(dir, &dctx)?;
        m.source = format!("tool:{name}:directory");
        mounts.push(m);
    }

    let mut secrets = Vec::new();
    for (sidx, s) in raw.secret.iter().enumerate() {
        let sctx = format!("{ctx}.secret[{sidx}]");
        let mut entries = validate_secret(s, &sctx)?;
        for entry in &mut entries {
            entry.tool = Some(name.to_owned());
        }
        secrets.extend(entries);
    }

    Ok((tool, mounts, secrets))
}

fn validate_browser(raw: &RawBrowser) -> Result<BrowserConfig, ConfigError> {
    if !raw.enabled {
        return Ok(BrowserConfig::default());
    }

    let command_str = require_non_empty(&raw.command, "[browser].command")?;
    let command = if command_str.contains('/') || command_str.starts_with('~') {
        expand_path(command_str, "[browser].command")?
            .to_string_lossy()
            .into_owned()
    } else {
        command_str.to_owned()
    };

    require_non_empty(&raw.profile_dir, "[browser].profile_dir")?;
    let profile_dir = expand_path(&raw.profile_dir, "[browser].profile_dir")?;

    if raw.debug_port == 0 {
        return Err(ConfigError::Validation(
            "[browser].debug_port must be set when browser is enabled".into(),
        ));
    }

    Ok(BrowserConfig {
        enabled: true,
        command,
        profile_dir,
        debug_port: raw.debug_port,
        pi_skill_path: raw.pi_skill_path.clone(),
        command_args: raw.command_args.clone(),
    })
}

// --- helpers ---

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
