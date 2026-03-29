use crate::cli::Agent;
use crate::config::ValidatedConfig;

const HOST_SERVICE_PROMPT_HINT: &str =
    "Sandbox: use host.containers.internal (localhost is container-local).";

/// Agent-specific launch profile: command, args, env, and boot behavior.
#[derive(Default)]
pub struct AgentProfile {
    pub command: String,
    pub command_args: Vec<String>,
    pub extra_env: Vec<(String, String)>,
    /// Container directories to `mkdir -p` in the entrypoint script.
    pub extra_boot_dirs: Vec<String>,
    /// Shell commands to run in the entrypoint before `exec`.
    pub entrypoint_setup: String,
    /// CLI flag for browser skill injection (e.g. "--skill" for pi).
    pub browser_skill_flag: Option<String>,
    /// Path argument for the browser skill flag.
    pub browser_skill_path: String,
}

/// Build the launch profile for the given agent.
pub fn profile_for(agent: Agent, config: &ValidatedConfig) -> AgentProfile {
    profile_for_with_guards(agent, config, true, false, false)
}

/// Build the launch profile for the given agent with AGS guard integrations
/// either enabled or disabled for the current run.
pub fn profile_for_with_guards(
    agent: Agent,
    config: &ValidatedConfig,
    guard_enabled: bool,
    root_mode: bool,
    lockdown: bool,
) -> AgentProfile {
    let mut profile = match agent {
        Agent::Pi => pi_profile(config, guard_enabled),
        Agent::Claude => claude_profile(guard_enabled, lockdown),
        Agent::Codex => codex_profile(),
        Agent::Gemini => gemini_profile(),
        Agent::Opencode => opencode_profile(),
        Agent::Shell => shell_profile(),
    };
    if root_mode && matches!(agent, Agent::Pi | Agent::Claude) {
        profile
            .command_args
            .push("--append-system-prompt".to_owned());
        profile
            .command_args
            .push("You have root access. Install any packages you need with dnf/pip.".to_owned());
    }
    profile
}

fn pi_profile(config: &ValidatedConfig, guard_enabled: bool) -> AgentProfile {
    let mut command_args = Vec::new();
    if guard_enabled {
        command_args.push("-e".to_owned());
        command_args.push("/home/dev/.pi/agent/extensions/guard.ts".to_owned());
    }
    command_args.push("--append-system-prompt".to_owned());
    command_args.push(HOST_SERVICE_PROMPT_HINT.to_owned());

    AgentProfile {
        command: "pi".to_owned(),
        command_args,
        browser_skill_flag: Some("--skill".to_owned()),
        browser_skill_path: config.browser.pi_skill_path.clone(),
        ..AgentProfile::default()
    }
}

fn claude_profile(guard_enabled: bool, lockdown: bool) -> AgentProfile {
    let (guard_hook_path, guard_plugin_dir) = if lockdown {
        ("/run/ags-claude-hooks/guard.sh", "/run/ags-claude-hooks")
    } else {
        (
            "/home/dev/.config/ags/hooks/guard.sh",
            "/home/dev/.config/ags/hooks",
        )
    };
    let settings_json = format!(
        r#"{{"sandbox":{{"enabled":false}},"hooks":{{"PreToolUse":[{{"matcher":"Bash|Read|Write|Edit|Grep|Glob","hooks":[{{"type":"command","command":"{guard_hook_path}","timeout":5}}]}}]}}}}"#,
    );

    let mut command_args = vec!["--dangerously-skip-permissions".to_owned()];
    if guard_enabled {
        command_args.push("--settings".to_owned());
        command_args.push(settings_json);
        command_args.push("--plugin-dir".to_owned());
        command_args.push(guard_plugin_dir.to_owned());
    }
    command_args.push("--append-system-prompt".to_owned());
    command_args.push(HOST_SERVICE_PROMPT_HINT.to_owned());

    AgentProfile {
        command: "claude".to_owned(),
        command_args,
        entrypoint_setup: "ln -sf /opt/claude-home/.local/bin/claude /home/dev/.local/bin/claude"
            .to_owned(),
        ..AgentProfile::default()
    }
}

fn codex_profile() -> AgentProfile {
    AgentProfile {
        command: "codex".to_owned(),
        command_args: vec![
            "-c".to_owned(),
            format!(
                "developer_instructions={}",
                toml_basic_string(HOST_SERVICE_PROMPT_HINT)
            ),
        ],
        ..AgentProfile::default()
    }
}

fn toml_basic_string(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

const OPENCODE_BOOT_DIRS: &[&str] = &[
    "/home/dev/.local/share/opencode",
    "/home/dev/.cache/opencode",
];

fn opencode_boot_dirs() -> Vec<String> {
    OPENCODE_BOOT_DIRS.iter().map(|s| (*s).to_owned()).collect()
}

fn gemini_profile() -> AgentProfile {
    AgentProfile {
        command: "gemini".to_owned(),
        ..AgentProfile::default()
    }
}

fn shell_profile() -> AgentProfile {
    AgentProfile {
        command: "bash".to_owned(),
        extra_boot_dirs: opencode_boot_dirs(),
        ..AgentProfile::default()
    }
}

fn opencode_profile() -> AgentProfile {
    AgentProfile {
        command: "opencode".to_owned(),
        extra_boot_dirs: opencode_boot_dirs(),
        ..AgentProfile::default()
    }
}
