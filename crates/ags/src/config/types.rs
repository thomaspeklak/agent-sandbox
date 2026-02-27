use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;

use crate::cli::Agent;

/// Validated, path-resolved configuration ready for use by the launch pipeline.
#[derive(Debug, Clone)]
pub struct ValidatedConfig {
    pub config_file: PathBuf,
    pub sandbox: ValidatedSandbox,
    pub mounts: Vec<ValidatedMount>,
    pub tools: Vec<ValidatedTool>,
    pub secrets: Vec<ValidatedSecret>,
    pub browser: BrowserConfig,
    pub update: UpdateConfig,
}

#[derive(Debug, Clone)]
pub struct ValidatedSandbox {
    pub image: String,
    pub containerfile: PathBuf,
    pub sandbox_pi_dir: PathBuf,
    pub host_pi_dir: PathBuf,
    pub host_claude_dir: PathBuf,
    pub cache_dir: PathBuf,
    pub gitconfig_path: PathBuf,
    pub auth_key: PathBuf,
    pub sign_key: PathBuf,
    pub agent_sandbox_base: PathBuf,
    pub bootstrap_files: Vec<String>,
    pub container_boot_dirs: Vec<String>,
    pub passthrough_env: Vec<String>,
}

impl ValidatedSandbox {
    /// Per-agent sandbox directory under `agent_sandbox_base/<name>`.
    pub fn sandbox_dir_for(&self, agent: Agent) -> PathBuf {
        self.agent_sandbox_base.join(agent.as_str())
    }
}

#[derive(Debug, Clone)]
pub struct ValidatedMount {
    pub host: PathBuf,
    pub container: String,
    pub mode: MountMode,
    pub kind: MountKind,
    pub when: MountWhen,
    pub create: bool,
    pub optional: bool,
    pub source: String,
}

#[derive(Debug, Clone)]
pub struct ValidatedTool {
    pub name: String,
    pub path: PathBuf,
    pub container_path: String,
    pub mode: MountMode,
    pub when: MountWhen,
    pub optional: bool,
}

#[derive(Debug, Clone)]
pub struct ValidatedSecret {
    pub env: String,
    pub source: SecretSource,
    pub origin: String,
    pub tool: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MountMode {
    Ro,
    Rw,
}

impl fmt::Display for MountMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ro => f.write_str("ro"),
            Self::Rw => f.write_str("rw"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MountKind {
    Dir,
    File,
}

impl fmt::Display for MountKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Dir => f.write_str("dir"),
            Self::File => f.write_str("file"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MountWhen {
    Always,
    Browser,
}

impl fmt::Display for MountWhen {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Always => f.write_str("always"),
            Self::Browser => f.write_str("browser"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecretSource {
    Env {
        from_env: String,
    },
    SecretTool {
        attributes: BTreeMap<String, String>,
    },
}

#[derive(Debug, Clone)]
pub struct BrowserConfig {
    pub enabled: bool,
    pub command: String,
    pub profile_dir: PathBuf,
    pub debug_port: u16,
    pub pi_skill_path: String,
    pub command_args: Vec<String>,
}

impl Default for BrowserConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            command: String::new(),
            profile_dir: PathBuf::new(),
            debug_port: 0,
            pi_skill_path: String::new(),
            command_args: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct UpdateConfig {
    pub pi_spec: String,
    pub minimum_release_age: u32,
}

impl Default for UpdateConfig {
    fn default() -> Self {
        Self {
            pi_spec: "@mariozechner/pi-coding-agent".to_owned(),
            minimum_release_age: 1440,
        }
    }
}
