use std::collections::BTreeMap;

use serde::Deserialize;

/// Top-level config as deserialized directly from TOML.
/// Field names and shapes match the config file schema exactly.
#[derive(Debug, Deserialize)]
pub struct RawConfig {
    pub sandbox: RawSandbox,
    #[serde(default)]
    pub mount: Vec<RawMount>,
    #[serde(default)]
    pub tool: Vec<RawTool>,
    #[serde(default)]
    pub secret: Vec<RawSecret>,
    #[serde(default)]
    pub browser: RawBrowser,
    #[serde(default)]
    pub update: RawUpdate,
}

#[derive(Debug, Deserialize)]
pub struct RawSandbox {
    pub image: String,
    pub containerfile: String,
    pub sandbox_pi_dir: String,
    pub host_pi_dir: String,
    pub host_claude_dir: String,
    pub cache_dir: String,
    pub gitconfig_path: String,
    pub auth_key: String,
    pub sign_key: String,
    #[serde(default = "default_agent_sandbox_base")]
    pub agent_sandbox_base: String,
    #[serde(default)]
    pub bootstrap_files: Vec<String>,
    #[serde(default)]
    pub container_boot_dirs: Vec<String>,
    #[serde(default)]
    pub passthrough_env: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct RawMount {
    pub host: String,
    pub container: String,
    pub mode: String,
    #[serde(default = "default_kind")]
    pub kind: String,
    #[serde(default)]
    pub create: bool,
    #[serde(default)]
    pub optional: bool,
    #[serde(default = "default_when")]
    pub when: String,
    #[serde(default = "default_source")]
    pub source: String,
}

#[derive(Debug, Deserialize)]
pub struct RawTool {
    pub name: String,
    pub path: String,
    pub container_path: String,
    #[serde(default = "default_ro")]
    pub mode: String,
    #[serde(default = "default_when")]
    pub when: String,
    #[serde(default)]
    pub optional: bool,
    #[serde(default)]
    pub directory: Vec<RawMount>,
    #[serde(default)]
    pub secret: Vec<RawSecret>,
}

#[derive(Debug, Deserialize)]
pub struct RawSecret {
    pub env: String,
    pub from_env: Option<String>,
    pub secret_store: Option<BTreeMap<String, String>>,
    // Legacy form
    pub provider: Option<String>,
    pub var: Option<String>,
    pub attributes: Option<BTreeMap<String, String>>,
}

#[derive(Debug, Default, Deserialize)]
pub struct RawBrowser {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub command: String,
    #[serde(default)]
    pub profile_dir: String,
    #[serde(default)]
    pub debug_port: u16,
    #[serde(default)]
    pub pi_skill_path: String,
    #[serde(default)]
    pub command_args: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct RawUpdate {
    #[serde(default = "default_pi_spec")]
    pub pi_spec: String,
    #[serde(default = "default_release_age")]
    pub minimum_release_age: u32,
}

impl Default for RawUpdate {
    fn default() -> Self {
        Self {
            pi_spec: default_pi_spec(),
            minimum_release_age: default_release_age(),
        }
    }
}

fn default_agent_sandbox_base() -> String {
    "~/.config/ags".to_owned()
}

fn default_kind() -> String {
    "dir".to_owned()
}

fn default_when() -> String {
    "always".to_owned()
}

fn default_source() -> String {
    "config".to_owned()
}

fn default_ro() -> String {
    "ro".to_owned()
}

fn default_pi_spec() -> String {
    "@mariozechner/pi-coding-agent".to_owned()
}

fn default_release_age() -> u32 {
    1440
}
