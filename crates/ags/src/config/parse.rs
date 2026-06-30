use std::fs;
use std::path::{Path, PathBuf};

use toml::Value;

use crate::config::error::ConfigError;
use crate::config::raw::{
    RawAgentMount, RawBrowser, RawClipboard, RawConfig, RawHostUi, RawMount, RawSecret, RawTool,
};
use crate::config::types::{
    AuthProxyConfig, BrowserConfig, ClipboardConfig, ClipboardMode, DesktopPassthroughConfig,
    HostUiConfig, MountKind, MountMode, MountWhen, PspConfig, SecretSource, UpdateConfig,
    ValidatedConfig, ValidatedMount, ValidatedSandbox, ValidatedSecret, ValidatedTool,
};
use crate::network::PodmanNetwork;

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

    if let Some(overlay_path) = overlay_path {
        let overlay = read_toml_value(overlay_path)?;
        merge_toml_value(&mut merged, overlay, &[]);
    }

    parse_toml_value(merged, base_path)
}

/// Parse and validate config from a TOML string (useful for testing).
pub fn parse_toml_str(content: &str, config_path: &Path) -> Result<ValidatedConfig, ConfigError> {
    let value = toml::from_str(content).map_err(|e| ConfigError::Toml {
        path: config_path.to_owned(),
        source: e,
    })?;
    parse_toml_value(value, config_path)
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

fn parse_toml_value(value: Value, config_path: &Path) -> Result<ValidatedConfig, ConfigError> {
    let raw: RawConfig = value.try_into().map_err(|e| ConfigError::Toml {
        path: config_path.to_owned(),
        source: e,
    })?;
    validate(raw, config_path)
}

fn merge_toml_value(base: &mut Value, overlay: Value, path: &[&str]) {
    match (base, overlay) {
        (Value::Table(base_table), Value::Table(overlay_table)) => {
            for (key, overlay_value) in overlay_table {
                if is_additive_array_key(path, &key) {
                    match (base_table.get_mut(&key), overlay_value) {
                        (Some(Value::Array(base_array)), Value::Array(mut overlay_array)) => {
                            base_array.append(&mut overlay_array);
                        }
                        (_, overlay_value) => {
                            base_table.insert(key, overlay_value);
                        }
                    }
                    continue;
                }

                match base_table.get_mut(&key) {
                    Some(base_value) => {
                        let mut child_path = path.to_vec();
                        child_path.push(key.as_str());
                        merge_toml_value(base_value, overlay_value, &child_path);
                    }
                    None => {
                        base_table.insert(key, overlay_value);
                    }
                }
            }
        }
        (base_slot, overlay_value) => {
            *base_slot = overlay_value;
        }
    }
}

fn is_additive_array_key(path: &[&str], key: &str) -> bool {
    path.is_empty() && super::ADDITIVE_ARRAY_KEYS.contains(&key)
}

fn validate(raw: RawConfig, config_path: &Path) -> Result<ValidatedConfig, ConfigError> {
    let sandbox = validate_sandbox(&raw.sandbox)?;

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

fn validate_sandbox(raw: &crate::config::raw::RawSandbox) -> Result<ValidatedSandbox, ConfigError> {
    Ok(ValidatedSandbox {
        image: require_non_empty(&raw.image, "[sandbox].image")?.to_owned(),
        containerfile: expand_path(&raw.containerfile, "[sandbox].containerfile")?,
        podman_network: validate_podman_network(&raw.podman_network)?,
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
    })
}

fn validate_podman_network(value: &str) -> Result<PodmanNetwork, ConfigError> {
    if value.is_empty() {
        return Ok(PodmanNetwork::default());
    }
    PodmanNetwork::parse(value).map_err(ConfigError::Validation)
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

/// Resolve a binary name: expand paths containing '/' or '~', otherwise keep bare name.
fn resolve_binary_name(raw: &str, ctx: &str) -> Result<String, ConfigError> {
    let name = require_non_empty(raw, ctx)?;
    if name.contains('/') || name.starts_with('~') {
        Ok(expand_path(name, ctx)?.to_string_lossy().into_owned())
    } else {
        Ok(name.to_owned())
    }
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

include!("parse_expand.rs");
