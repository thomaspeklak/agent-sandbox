use crate::cli::Agent;
use crate::config::ValidatedConfig;

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
    match agent {
        Agent::Pi => pi_profile(config),
        Agent::Claude => claude_profile(),
        Agent::Codex => codex_profile(),
        Agent::Gemini => gemini_profile(),
        Agent::Opencode => opencode_profile(),
        Agent::Shell => shell_profile(),
    }
}

fn pi_profile(config: &ValidatedConfig) -> AgentProfile {
    AgentProfile {
        command: "pi".to_owned(),
        command_args: vec![
            "--no-extensions".to_owned(),
            "-e".to_owned(),
            "/home/dev/.pi/agent/extensions/guard.ts".to_owned(),
        ],
        extra_env: vec![],
        extra_boot_dirs: vec![],
        entrypoint_setup: String::new(),
        browser_skill_flag: Some("--skill".to_owned()),
        browser_skill_path: config.browser.pi_skill_path.clone(),
    }
}

fn claude_profile() -> AgentProfile {
    AgentProfile {
        command: "claude".to_owned(),
        command_args: vec![
            "--dangerously-skip-permissions".to_owned(),
            "--settings".to_owned(),
            "{\"sandbox\":{\"enabled\":false}}".to_owned(),
        ],
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
        command_args: vec![],
        extra_env: vec![],
        extra_boot_dirs: vec![],
        entrypoint_setup: String::new(),
        browser_skill_flag: None,
        browser_skill_path: String::new(),
    }
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
