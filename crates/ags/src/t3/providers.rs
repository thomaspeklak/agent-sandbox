use super::registration::{Registration, write_json};
use crate::cli::Agent;
use crate::config::ValidatedConfig;
use crate::util::shell_quote;
use serde_json::{Value, json};
use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;

pub fn prepare(registration: &Registration, config: &ValidatedConfig) -> io::Result<PathBuf> {
    let assets = registration.private_dir()?.join("assets");
    crate::util::ensure_private_dir(&assets)?;
    let settings_path = registration.home.join(".t3/userdata/settings.json");
    let mut settings = if settings_path.exists() {
        serde_json::from_slice::<Value>(&fs::read(&settings_path)?)?
    } else {
        json!({})
    };
    if !settings.is_object() {
        return Err(io::Error::other("T3 settings must be a JSON object"));
    }
    if settings.get("providers").is_none() {
        settings["providers"] = json!({});
    }
    if !settings["providers"].is_object() {
        return Err(io::Error::other("invalid T3 provider settings"));
    }
    settings["enableProviderUpdateChecks"] = json!(false);
    for (kind, agent, binary, home) in [
        (
            "codex",
            Agent::Codex,
            "/usr/local/pnpm/codex",
            "/home/dev/.codex",
        ),
        (
            "claudeAgent",
            Agent::Claude,
            "/opt/claude-home/.local/bin/claude",
            "/home/dev/.claude",
        ),
        (
            "opencode",
            Agent::Opencode,
            "/opt/opencode-home/.opencode/bin/opencode",
            "/home/dev/.config/opencode",
        ),
    ] {
        if settings["providers"].get(kind).is_none() {
            settings["providers"][kind] = json!({});
        }
        let provider = &mut settings["providers"][kind];
        if !provider.is_object() {
            return Err(io::Error::other("invalid T3 provider entry"));
        }
        provider["enabled"] = json!(config.sandbox.is_agent_enabled(agent));
        provider["binaryPath"] = json!(format!("/run/ags-t3/{}", agent.as_str()));
        if agent != Agent::Opencode {
            provider["homePath"] = json!(home);
        }
        if agent == Agent::Codex {
            provider["setupMode"] = json!("existing");
        }
        let profile = crate::agent::profile_for_with_guards(
            agent,
            config,
            !registration.settings.yolo,
            registration.settings.root,
            false,
        );
        let extra_args = profile
            .command_args
            .iter()
            .filter(|arg| arg.as_str() != "--dangerously-skip-permissions")
            .map(|arg| shell_quote(arg))
            .collect::<Vec<_>>()
            .join(" ");
        let mut wrapper = String::from(
            "#!/usr/bin/env bash\nexport HOME=/home/dev\nunset XDG_CONFIG_HOME XDG_CACHE_HOME XDG_DATA_HOME\n",
        );
        if agent == Agent::Codex {
            wrapper.push_str("export CODEX_HOME=/home/dev/.codex\n");
        }
        if agent == Agent::Claude {
            wrapper.push_str("unset CLAUDE_CONFIG_DIR\n");
        }
        for (name, value) in &profile.extra_env {
            wrapper.push_str(&format!("export {name}={}\n", shell_quote(value)));
        }
        if !profile.extra_boot_dirs.is_empty() {
            wrapper.push_str(&format!(
                "mkdir -p {}\n",
                profile
                    .extra_boot_dirs
                    .iter()
                    .map(|dir| shell_quote(dir))
                    .collect::<Vec<_>>()
                    .join(" ")
            ));
        }
        if !profile.entrypoint_setup.is_empty() {
            wrapper.push_str(&format!("{}\n", profile.entrypoint_setup));
        }
        wrapper.push_str(&format!(
            "exec {} {extra_args} \"$@\"\n",
            shell_quote(binary)
        ));
        fs::write(assets.join(agent.as_str()), wrapper)?;
        fs::set_permissions(
            assets.join(agent.as_str()),
            fs::Permissions::from_mode(0o755),
        )?;
    }
    for kind in ["cursor", "grok", "antigravity"] {
        if settings["providers"].get(kind).is_none() {
            settings["providers"][kind] = json!({});
        }
        if !settings["providers"][kind].is_object() {
            return Err(io::Error::other("invalid T3 provider entry"));
        }
        settings["providers"][kind]["enabled"] = json!(false);
    }
    write_json(&settings_path, &settings)?;
    for (name, contents) in [
        (
            "environment-bootstrap",
            include_str!("environment-bootstrap.py"),
        ),
        ("stop-server", include_str!("stop-server.py")),
        ("forward-tcp.js", include_str!("forward-tcp.js")),
        (
            "onepassword-bootstrap",
            include_str!("../../../../agent/onepassword-bootstrap"),
        ),
    ] {
        fs::write(assets.join(name), contents)?;
        fs::set_permissions(assets.join(name), fs::Permissions::from_mode(0o755))?;
    }
    if config.sandbox.is_agent_enabled(Agent::Claude) && !registration.settings.yolo {
        let hooks = config.sandbox.cache_dir.join("ags-hooks");
        crate::assets::ensure_claude_guard_hook(&hooks)?;
        crate::assets::ensure_claude_guard_skill(&hooks)?;
    }
    Ok(assets)
}
