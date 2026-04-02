use std::path::{Path, PathBuf};

pub fn create_default_config(path: &Path) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, DEFAULT_CONFIG)
}

pub fn default_config_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("~/.config"))
        .join("ags/config.toml")
}

pub const DEFAULT_CONFIG: &str = r#"[sandbox]
image = "localhost/agent-sandbox:latest"
containerfile = "~/.config/ags/Containerfile"
cache_dir = "~/.cache/ags"
gitconfig_path = "~/.config/ags/gitconfig-agent"
auth_key = "~/.ssh/ags-agent-auth"
sign_key = "~/.ssh/ags-agent-signing"
bootstrap_files = ["auth.json", "models.json"]
container_boot_dirs = [
  "/home/dev/.ssh",
]
passthrough_env = [
  "ANTHROPIC_API_KEY",
  "OPENAI_API_KEY",
  "GEMINI_API_KEY",
  "OPENROUTER_API_KEY",
  "AI_GATEWAY_API_KEY",
  "OPENCODE_API_KEY",
]

[[mount]]
host = "~/.ssh/known_hosts"
container = "/home/dev/.ssh/known_hosts"
mode = "ro"
kind = "file"
optional = true

[[agent_mount]]
host = "~/.claude.json"
container = "/home/dev/.claude.json"
kind = "file"

[[agent_mount]]
host = "~/.claude"
container = "/home/dev/.claude"

[[agent_mount]]
host = "~/.codex"
container = "/home/dev/.codex"

[[agent_mount]]
host = "~/.pi"
container = "/home/dev/.pi"

[[agent_mount]]
host = "~/.config/opencode"
container = "/home/dev/.config/opencode"

[[agent_mount]]
host = "~/.gemini"
container = "/home/dev/.gemini"

[host_ui]
enabled = false
binary = "glimpse-host-ui"
renderer = "stub"
idle_timeout_ms = 0
log_level = "info"
"#;
