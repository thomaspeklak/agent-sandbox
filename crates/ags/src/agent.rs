use std::path::PathBuf;

use crate::cli::Agent;
use crate::config::{MountMode, ValidatedConfig};
use crate::plan::PlanMount;

/// Agent-specific launch profile: command, args, env, mounts, and boot dirs.
pub struct AgentProfile {
    pub command: String,
    pub command_args: Vec<String>,
    pub extra_env: Vec<(String, String)>,
    pub extra_mounts: Vec<PlanMount>,
    /// File mounts that are skipped when the host path doesn't exist.
    pub optional_file_mounts: Vec<PlanMount>,
    /// Host directories to create before launch (e.g. sandbox subdirs).
    pub host_setup_dirs: Vec<PathBuf>,
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
        Agent::Claude => claude_profile(config),
        Agent::Codex => codex_profile(config),
        Agent::Gemini => gemini_profile(config),
        Agent::Opencode => opencode_profile(config),
    }
}

fn pi_profile(config: &ValidatedConfig) -> AgentProfile {
    let sandbox = config.sandbox.sandbox_dir_for(Agent::Pi);
    AgentProfile {
        command: "pi".to_owned(),
        command_args: vec![
            "--no-extensions".to_owned(),
            "-e".to_owned(),
            "/home/dev/.pi/extensions/guard.ts".to_owned(),
        ],
        extra_env: vec![("PI_CODING_AGENT_DIR".to_owned(), "/home/dev/.pi".to_owned())],
        extra_mounts: vec![PlanMount {
            host: sandbox.clone(),
            container: "/home/dev/.pi".to_owned(),
            mode: MountMode::Rw,
        }],
        optional_file_mounts: vec![],
        host_setup_dirs: vec![sandbox.join("extensions")],
        extra_boot_dirs: vec![],
        entrypoint_setup: String::new(),
        browser_skill_flag: Some("--skill".to_owned()),
        browser_skill_path: config.browser.pi_skill_path.clone(),
    }
}

fn claude_profile(config: &ValidatedConfig) -> AgentProfile {
    let sandbox = config.sandbox.sandbox_dir_for(Agent::Claude);
    AgentProfile {
        command: "claude".to_owned(),
        command_args: vec![],
        extra_env: vec![(
            "CLAUDE_CONFIG_DIR".to_owned(),
            "/home/dev/.claude".to_owned(),
        )],
        extra_mounts: vec![PlanMount {
            host: sandbox.clone(),
            container: "/home/dev/.claude".to_owned(),
            mode: MountMode::Rw,
        }],
        optional_file_mounts: vec![PlanMount {
            host: config.sandbox.agent_sandbox_base.join(".claude.json"),
            container: "/home/dev/.claude.json".to_owned(),
            mode: MountMode::Rw,
        }],
        host_setup_dirs: vec![sandbox],
        extra_boot_dirs: vec![],
        entrypoint_setup: String::new(),
        browser_skill_flag: None,
        browser_skill_path: String::new(),
    }
}

fn codex_profile(config: &ValidatedConfig) -> AgentProfile {
    let sandbox = config.sandbox.sandbox_dir_for(Agent::Codex);
    AgentProfile {
        command: "codex".to_owned(),
        command_args: vec![],
        extra_env: vec![],
        extra_mounts: vec![PlanMount {
            host: sandbox.clone(),
            container: "/home/dev/.codex".to_owned(),
            mode: MountMode::Rw,
        }],
        optional_file_mounts: vec![],
        host_setup_dirs: vec![sandbox],
        extra_boot_dirs: vec![],
        entrypoint_setup: String::new(),
        browser_skill_flag: None,
        browser_skill_path: String::new(),
    }
}

fn gemini_profile(config: &ValidatedConfig) -> AgentProfile {
    let sandbox = config.sandbox.sandbox_dir_for(Agent::Gemini);
    AgentProfile {
        command: "gemini".to_owned(),
        command_args: vec![],
        extra_env: vec![],
        extra_mounts: vec![PlanMount {
            host: sandbox.clone(),
            container: "/home/dev/.gemini".to_owned(),
            mode: MountMode::Rw,
        }],
        optional_file_mounts: vec![],
        host_setup_dirs: vec![sandbox],
        extra_boot_dirs: vec![],
        entrypoint_setup: String::new(),
        browser_skill_flag: None,
        browser_skill_path: String::new(),
    }
}

fn opencode_profile(config: &ValidatedConfig) -> AgentProfile {
    let sandbox = config.sandbox.sandbox_dir_for(Agent::Opencode);
    AgentProfile {
        command: "opencode".to_owned(),
        command_args: vec![],
        extra_env: vec![],
        extra_mounts: vec![PlanMount {
            host: sandbox.clone(),
            container: "/home/dev/.config/opencode".to_owned(),
            mode: MountMode::Rw,
        }],
        optional_file_mounts: vec![],
        host_setup_dirs: vec![sandbox],
        extra_boot_dirs: vec!["/home/dev/.local/share/opencode".to_owned()],
        entrypoint_setup: String::new(),
        browser_skill_flag: None,
        browser_skill_path: String::new(),
    }
}
