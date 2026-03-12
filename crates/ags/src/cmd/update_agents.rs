use std::fmt;
use std::fs;
use std::process::Command;

use crate::config::ValidatedConfig;

/// Options for the update-agents command.
#[derive(Default)]
pub struct UpdateAgentsOptions {
    pub pi_spec: Option<String>,
    pub minimum_release_age: Option<u32>,
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
printf 'minimum-release-age=%s\nignore-scripts=true\n' '{release_age}' > "$HOME/.config/pnpm/rc" && \
(pnpm self-update || echo '[ags] pnpm self-update skipped (release too new?); using existing version' >&2) && \
(PNPM_HOME=/usr/local/pnpm PATH=/usr/local/pnpm:$PATH \
  pnpm add -g --store-dir /usr/local/pnpm/.store \
    {pi_spec} @openai/codex @google/gemini-cli opencode-ai || \
  (echo '[ags] pnpm add -g failed (release too new?); using existing installs' >&2 && \
   PNPM_HOME=/usr/local/pnpm PATH=/usr/local/pnpm:$PATH command -v pi >/dev/null 2>&1)) && \
CLAUDE_HOME=/opt/claude-home && \
CLAUDE_BIN="$CLAUDE_HOME/.local/bin/claude" && \
if [ -x "$CLAUDE_BIN" ]; then \
  HOME="$CLAUDE_HOME" PATH="$CLAUDE_HOME/.local/bin:$PATH" "$CLAUDE_BIN" update || \
  (echo 'claude update failed; reinstalling via install.sh' >&2 && \
   export HOME="$CLAUDE_HOME" PATH="$CLAUDE_HOME/.local/bin:$PATH" && \
   curl -fsSL https://claude.ai/install.sh | bash); \
else \
  export HOME="$CLAUDE_HOME" PATH="$CLAUDE_HOME/.local/bin:$PATH" && \
  curl -fsSL https://claude.ai/install.sh | bash; \
fi && \
[ -x "$CLAUDE_BIN" ] && \
rm -f /usr/local/pnpm/claude && \
printf '%s\n' '#!/usr/bin/env bash' 'export PATH=/opt/claude-home/.local/bin:$PATH' 'exec /opt/claude-home/.local/bin/claude "$@"' > /usr/local/pnpm/claude && \
chmod +x /usr/local/pnpm/claude"#,
        release_age = release_age,
        pi_spec = pi_spec,
    )
}

#[cfg(test)]
mod tests {
    use super::build_install_script;

    #[test]
    fn claude_update_still_uses_persistent_install_home() {
        let script = build_install_script("@mariozechner/pi-coding-agent", 1440);

        assert!(
            script.contains(
                "HOME=\"$CLAUDE_HOME\" PATH=\"$CLAUDE_HOME/.local/bin:$PATH\" \"$CLAUDE_BIN\" update"
            ),
            "claude update should run with persistent CLAUDE_HOME"
        );
    }

    #[test]
    fn claude_wrapper_does_not_override_runtime_home() {
        let script = build_install_script("@mariozechner/pi-coding-agent", 1440);

        assert!(
            script.contains("exec /opt/claude-home/.local/bin/claude \"$@\""),
            "wrapper should execute claude from persistent install path"
        );
        assert!(
            script.contains("export PATH=/opt/claude-home/.local/bin:$PATH"),
            "wrapper should keep claude bin on PATH"
        );
        assert!(
            !script.contains("export HOME=/opt/claude-home"),
            "wrapper must not override HOME at runtime"
        );
    }
}
