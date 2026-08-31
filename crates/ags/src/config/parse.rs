use std::fs;
use std::path::{Path, PathBuf};

use toml::Value;

use crate::cli::Agent;
use crate::config::error::ConfigError;
use crate::config::raw::{
    RawAgentMount, RawBrowser, RawClipboard, RawConfig, RawHostUi, RawMount, RawSecret, RawTool,
};
use crate::config::types::{
    AuthProxyConfig, BrowserConfig, ClipboardConfig, ClipboardMode, DesktopPassthroughConfig,
    HostUiConfig, MountKind, MountMode, MountWhen, PspConfig, SecretSource, UpdateConfig,
    ValidatedConfig, ValidatedMount, ValidatedSandbox, ValidatedSecret, ValidatedTool,
};
use crate::config::{LockedAgentReleaseSource, LockedToolDownload};

/// Read, parse, and validate a config TOML file from disk.
pub fn parse_and_validate(path: &Path) -> Result<ValidatedConfig, ConfigError> {
    let content = fs::read_to_string(path).map_err(|e| ConfigError::Io {
        path: path.to_owned(),
        source: e,
    })?;
    parse_toml_str(&content, path)
}

/// Read, merge, and validate a base config plus an optional overlay config.
///
/// Scalar and table fields from the overlay take precedence. Repeatable top-level
/// tables (`[[mount]]`, `[[agent_mount]]`, `[[tool]]`, `[[secret]]`) are additive
/// so repository-local config can extend the base config instead of replacing it.
pub fn parse_and_validate_with_overlay(
    base_path: &Path,
    overlay_path: Option<&Path>,
) -> Result<ValidatedConfig, ConfigError> {
    let mut merged = read_toml_value(base_path)?;
    let mut tool_download_lock_config_path = base_path;
    let mut agent_release_source_lock_config_path = base_path;

    if let Some(overlay_path) = overlay_path {
        let overlay = read_toml_value(overlay_path)?;
        reject_overlay_command_secrets(&overlay, overlay_path)?;
        if overlay
            .get("sandbox")
            .and_then(Value::as_table)
            .is_some_and(|sandbox| sandbox.contains_key("tool_download_lock"))
        {
            tool_download_lock_config_path = overlay_path;
        }
        if overlay
            .get("sandbox")
            .and_then(Value::as_table)
            .is_some_and(|sandbox| sandbox.contains_key("agent_release_source_lock"))
        {
            agent_release_source_lock_config_path = overlay_path;
        }
        merge_toml_value(&mut merged, overlay, &[]);
    }

    parse_toml_value(
        merged,
        base_path,
        tool_download_lock_config_path,
        agent_release_source_lock_config_path,
    )
}

/// Parse and validate config from a TOML string (useful for testing).
pub fn parse_toml_str(content: &str, config_path: &Path) -> Result<ValidatedConfig, ConfigError> {
    let value = toml::from_str(content).map_err(|e| ConfigError::Toml {
        path: config_path.to_owned(),
        source: e,
    })?;
    parse_toml_value(value, config_path, config_path, config_path)
}

fn read_toml_value(path: &Path) -> Result<Value, ConfigError> {
    let content = fs::read_to_string(path).map_err(|e| ConfigError::Io {
        path: path.to_owned(),
        source: e,
    })?;
    toml::from_str(&content).map_err(|e| ConfigError::Toml {
        path: path.to_owned(),
        source: e,
    })
}

fn parse_toml_value(
    value: Value,
    config_path: &Path,
    tool_download_lock_config_path: &Path,
    agent_release_source_lock_config_path: &Path,
) -> Result<ValidatedConfig, ConfigError> {
    let raw: RawConfig = value.try_into().map_err(|e| ConfigError::Toml {
        path: config_path.to_owned(),
        source: e,
    })?;
    validate(
        raw,
        config_path,
        tool_download_lock_config_path,
        agent_release_source_lock_config_path,
    )
}

fn reject_overlay_command_secrets(overlay: &Value, overlay_path: &Path) -> Result<(), ConfigError> {
    let Some(root) = overlay.as_table() else {
        return Ok(());
    };

    if let Some(secrets) = root.get("secret").and_then(Value::as_array) {
        for (index, secret) in secrets.iter().enumerate() {
            if secret
                .as_table()
                .is_some_and(|table| table.contains_key("command"))
            {
                return Err(ConfigError::Validation(format!(
                    "repo-local config {} may not define [[secret]] #{index}.command; command secret sources are allowed only in the user/global config",
                    overlay_path.display()
                )));
            }
        }
    }

    if let Some(tools) = root.get("tool").and_then(Value::as_array) {
        for (tool_index, tool) in tools.iter().enumerate() {
            let Some(secrets) = tool
                .as_table()
                .and_then(|table| table.get("secret"))
                .and_then(Value::as_array)
            else {
                continue;
            };
            for (secret_index, secret) in secrets.iter().enumerate() {
                if secret
                    .as_table()
                    .is_some_and(|table| table.contains_key("command"))
                {
                    return Err(ConfigError::Validation(format!(
                        "repo-local config {} may not define [[tool]] #{tool_index}.secret[{secret_index}].command; command secret sources are allowed only in the user/global config",
                        overlay_path.display()
                    )));
                }
            }
        }
    }

    Ok(())
}

fn validate(
    raw: RawConfig,
    config_path: &Path,
    tool_download_lock_config_path: &Path,
    agent_release_source_lock_config_path: &Path,
) -> Result<ValidatedConfig, ConfigError> {
    let sandbox = validate_sandbox(
        &raw.sandbox,
        tool_download_lock_config_path,
        agent_release_source_lock_config_path,
    )?;

    let mut mounts = Vec::new();
    for (idx, m) in raw.mount.iter().enumerate() {
        mounts.push(validate_mount(m, &format!("[[mount]] #{idx}"))?);
    }
    for (idx, m) in raw.agent_mount.iter().enumerate() {
        mounts.push(validate_agent_mount(m, &format!("[[agent_mount]] #{idx}"))?);
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
    let host_ui = validate_host_ui(&raw.host_ui)?;
    let clipboard = validate_clipboard(&raw.clipboard)?;

    Ok(ValidatedConfig {
        config_file: config_path.to_owned(),
        sandbox,
        mounts,
        tools,
        secrets,
        browser,
        update: UpdateConfig {
            pi_spec: require_non_empty(&raw.update.pi_spec, "[update].pi_spec")?.to_owned(),
            minimum_release_age: raw.update.minimum_release_age,
        },
        auth_proxy: AuthProxyConfig {
            auto_allow_domains: raw.auth_proxy.auto_allow_domains,
        },
        host_ui,
        clipboard,
        desktop_passthrough: DesktopPassthroughConfig {
            wayland: raw.desktop_passthrough.wayland,
        },
        psp: PspConfig {
            binary: raw.psp.binary,
        },
    })
}

fn validate_dnf_packages(list: &[String], ctx: &str) -> Result<Vec<String>, ConfigError> {
    let packages = validate_string_list(list, ctx)?;
    for (index, package) in packages.iter().enumerate() {
        if !crate::config::is_valid_dnf_package_name(package) {
            return Err(ConfigError::Validation(format!(
                "{ctx}[{index}] must be a package name, not an option or shell expression"
            )));
        }
    }
    Ok(packages)
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

fn validate_agent_mount(raw: &RawAgentMount, ctx: &str) -> Result<ValidatedMount, ConfigError> {
    Ok(ValidatedMount {
        host: expand_path(&raw.host, &format!("{ctx}.host"))?,
        container: require_non_empty(&raw.container, &format!("{ctx}.container"))?.to_owned(),
        mode: MountMode::Rw,
        kind: parse_kind(&raw.kind, &format!("{ctx}.kind"))?,
        when: MountWhen::Always,
        create: false,
        optional: false,
        source: "agent_mount".to_owned(),
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

    if let Some(command) = &raw.command {
        let Some(executable) = command.first() else {
            return Err(ConfigError::Validation(format!(
                "{ctx}.command must include at least one argv element"
            )));
        };
        require_non_empty(executable, &format!("{ctx}.command[0]"))?;

        let mut argv = command.clone();
        argv[0] = resolve_command_executable(executable, &format!("{ctx}.command[0]"))?;
        out.push(ValidatedSecret {
            env: env.to_owned(),
            source: SecretSource::Command { argv },
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
            "{ctx} must define at least one source: from_env, secret_store, command, or provider"
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

fn resolve_binary_name(raw: &str, ctx: &str) -> Result<String, ConfigError> {
    let name = require_non_empty(raw, ctx)?;
    if name.contains('/') || name.starts_with('~') {
        Ok(expand_path(name, ctx)?.to_string_lossy().into_owned())
    } else {
        Ok(name.to_owned())
    }
}

fn resolve_command_executable(raw: &str, ctx: &str) -> Result<String, ConfigError> {
    let executable = require_non_empty(raw, ctx)?;
    let expanded = expand_env_vars(&expand_tilde(executable)?);
    let path = Path::new(&expanded);
    if !path.is_absolute() {
        return Err(ConfigError::Validation(format!(
            "{ctx} must resolve to an absolute executable path"
        )));
    }
    Ok(path.to_string_lossy().into_owned())
}

fn validate_browser(raw: &RawBrowser) -> Result<BrowserConfig, ConfigError> {
    if !raw.enabled {
        return Ok(BrowserConfig::default());
    }

    let command = resolve_binary_name(&raw.command, "[browser].command")?;

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

fn validate_clipboard(raw: &RawClipboard) -> Result<ClipboardConfig, ConfigError> {
    let mode = match raw.mode.to_lowercase().as_str() {
        "off" => ClipboardMode::Off,
        "read" => ClipboardMode::Read,
        "readwrite" | "read_write" | "rw" => ClipboardMode::ReadWrite,
        other => {
            return Err(ConfigError::Validation(format!(
                "[clipboard].mode must be 'off', 'read', or 'readwrite', got '{other}'"
            )));
        }
    };
    if raw.max_bytes == 0 {
        return Err(ConfigError::Validation(
            "[clipboard].max_bytes must be greater than zero".to_owned(),
        ));
    }
    Ok(ClipboardConfig {
        enabled: raw.enabled,
        mode,
        max_bytes: raw.max_bytes,
        approval_required: raw.approval_required,
        approval_seconds: raw.approval_seconds,
        approve_writes: raw.approve_writes,
    })
}

fn validate_host_ui(raw: &RawHostUi) -> Result<HostUiConfig, ConfigError> {
    let renderer = require_non_empty(&raw.renderer, "[host_ui].renderer")?.to_owned();
    let log_level = require_non_empty(&raw.log_level, "[host_ui].log_level")?.to_owned();
    let binary = resolve_binary_name(&raw.binary, "[host_ui].binary")?;
    let renderer_bin = if raw.renderer_bin.trim().is_empty() {
        None
    } else {
        Some(expand_path(&raw.renderer_bin, "[host_ui].renderer_bin")?)
    };

    Ok(HostUiConfig {
        enabled: raw.enabled,
        binary,
        renderer,
        renderer_bin,
        idle_timeout_ms: raw.idle_timeout_ms,
        log_level,
    })
}

// --- helpers ---

include!("parse_merge.rs");
include!("parse_expand.rs");
