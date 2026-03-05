use std::fmt;
use std::fs;
use std::process::Command;

use crate::config::ValidatedConfig;

/// Options for the update-agents command.
pub struct UpdateAgentsOptions {
    pub pi_spec: Option<String>,
    pub minimum_release_age: Option<u32>,
}

impl Default for UpdateAgentsOptions {
    fn default() -> Self {
        Self {
            pi_spec: None,
            minimum_release_age: None,
        }
    }
}

#[derive(Debug)]
pub enum UpdateAgentsError {
    HostDirCreate(String),
    InstallFailed(String),
}

impl fmt::Display for UpdateAgentsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::HostDirCreate(msg) => write!(f, "failed to create host directory: {msg}"),
            Self::InstallFailed(msg) => write!(f, "agent install failed: {msg}"),
        }
    }
}

impl std::error::Error for UpdateAgentsError {}

/// Install or update all agents in persistent volumes via a throwaway container.
pub fn run(config: &ValidatedConfig, opts: &UpdateAgentsOptions) -> Result<(), UpdateAgentsError> {
    let cache_dir = &config.sandbox.cache_dir;
    let image = &config.sandbox.image;

    let pnpm_home = cache_dir.join("pnpm-home");
    let claude_install = cache_dir.join("claude-install");

    // 1. Ensure host dirs exist
    for dir in [&pnpm_home, &claude_install] {
        fs::create_dir_all(dir)
            .map_err(|e| UpdateAgentsError::HostDirCreate(format!("{}: {e}", dir.display())))?;
    }

    let pi_spec = opts.pi_spec.as_deref().unwrap_or(&config.update.pi_spec);
    let release_age = opts
        .minimum_release_age
        .unwrap_or(config.update.minimum_release_age);

    // 2. Build the install script
    let script = build_install_script(pi_spec, release_age);

    // 3. Run throwaway container
    println!("Installing/updating agents in volumes...");
    println!("  PI spec: {pi_spec}");
    println!("  pnpm minimum-release-age: {release_age}");

    let status = Command::new("podman")
        .args([
            "run",
            "--rm",
            "-it",
            "--userns=keep-id",
            "-v",
            &format!("{}:/usr/local/pnpm:rw,z", pnpm_home.display()),
            "-v",
            &format!("{}:/opt/claude-home:rw,z", claude_install.display()),
            image,
            "bash",
            "-c",
            &script,
        ])
        .status()
        .map_err(|e| UpdateAgentsError::InstallFailed(e.to_string()))?;

    if !status.success() {
        return Err(UpdateAgentsError::InstallFailed(format!(
            "exited with {status}"
        )));
    }

    println!("\nDone. Agents updated in volumes.");
    println!("Verify with: ags --agent pi -- --version");
    Ok(())
}

fn build_install_script(pi_spec: &str, release_age: u32) -> String {
    format!(
        r#"set -e && \
mkdir -p "$HOME/.config/pnpm" && \
printf 'minimum-release-age=%s\n' '{release_age}' > "$HOME/.config/pnpm/rc" && \
pnpm self-update && \
PNPM_HOME=/usr/local/pnpm PATH=/usr/local/pnpm:$PATH \
  pnpm add -g --store-dir /usr/local/pnpm/.store \
    {pi_spec} @openai/codex @google/gemini-cli opencode-ai && \
if [ -x /opt/claude-home/.local/bin/claude ]; then
  /opt/claude-home/.local/bin/claude update
else
  export HOME=/opt/claude-home && curl -fsSL https://claude.ai/install.sh | bash
fi && \
ln -sf /opt/claude-home/.local/bin/claude /usr/local/pnpm/claude"#,
        release_age = release_age,
        pi_spec = pi_spec,
    )
}
