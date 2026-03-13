use crate::cli::Agent;
use crate::config::ValidatedConfig;

const HOST_SERVICE_PROMPT_HINT: &str =
    "Sandbox: use host.containers.internal (localhost is container-local).";

/// Agent-specific launch profile: command, args, env, and boot behavior.
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
    profile_for_with_guards(agent, config, true)
}

/// Build the launch profile for the given agent with AGS guard integrations
/// either enabled or disabled for the current run.
pub fn profile_for_with_guards(
    agent: Agent,
    config: &ValidatedConfig,
    guard_enabled: bool,
) -> AgentProfile {
    match agent {
        Agent::Pi => pi_profile(config, guard_enabled),
        Agent::Claude => claude_profile(guard_enabled),
        Agent::Codex => codex_profile(),
        Agent::Gemini => gemini_profile(),
        Agent::Opencode => opencode_profile(),
        Agent::Shell => shell_profile(),
    }
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
        extra_env: vec![],
        extra_boot_dirs: vec![],
        entrypoint_setup: String::new(),
        browser_skill_flag: Some("--skill".to_owned()),
        browser_skill_path: config.browser.pi_skill_path.clone(),
    }
}

fn claude_profile(guard_enabled: bool) -> AgentProfile {
    const GUARD_HOOK_PATH: &str = "/home/dev/.config/ags/hooks/guard.sh";
    const GUARD_PLUGIN_DIR: &str = "/home/dev/.config/ags/hooks";
    let settings_json = format!(
        r#"{{"sandbox":{{"enabled":false}},"hooks":{{"PreToolUse":[{{"matcher":"Bash|Read|Write|Edit|Grep|Glob","hooks":[{{"type":"command","command":"{GUARD_HOOK_PATH}","timeout":5}}]}}]}}}}"#,
    );

    let mut command_args = vec!["--dangerously-skip-permissions".to_owned()];
    if guard_enabled {
        command_args.push("--settings".to_owned());
        command_args.push(settings_json);
        command_args.push("--plugin-dir".to_owned());
        command_args.push(GUARD_PLUGIN_DIR.to_owned());
    }
    command_args.push("--append-system-prompt".to_owned());
    command_args.push(HOST_SERVICE_PROMPT_HINT.to_owned());

    AgentProfile {
        command: "claude".to_owned(),
        command_args,
        extra_env: vec![],
        extra_boot_dirs: vec![],
        entrypoint_setup: String::new(),
        browser_skill_flag: None,
        browser_skill_path: String::new(),
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
        extra_env: vec![],
        extra_boot_dirs: vec![],
        entrypoint_setup: String::new(),
        browser_skill_flag: None,
        browser_skill_path: String::new(),
    }
}

fn toml_basic_string(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

fn gemini_profile() -> AgentProfile {
    AgentProfile {
        command: "gemini".to_owned(),
        command_args: vec![],
        extra_env: vec![],
        extra_boot_dirs: vec![],
        entrypoint_setup: String::new(),
        browser_skill_flag: None,
        browser_skill_path: String::new(),
    }
}

fn shell_profile() -> AgentProfile {
    AgentProfile {
        command: "bash".to_owned(),
        command_args: vec![],
        extra_env: vec![],
        extra_boot_dirs: vec![
            "/home/dev/.local/share/opencode".to_owned(),
            "/home/dev/.cache/opencode".to_owned(),
        ],
        entrypoint_setup: String::new(),
        browser_skill_flag: None,
        browser_skill_path: String::new(),
    }
}

fn opencode_profile() -> AgentProfile {
    AgentProfile {
        command: "opencode".to_owned(),
        command_args: vec![],
        extra_env: vec![],
        extra_boot_dirs: vec![
            "/home/dev/.local/share/opencode".to_owned(),
            "/home/dev/.cache/opencode".to_owned(),
        ],
        entrypoint_setup: String::new(),
        browser_skill_flag: None,
        browser_skill_path: String::new(),
    }
}
