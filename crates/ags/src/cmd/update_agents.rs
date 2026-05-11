use std::fmt;
use std::fs;
use std::process::Command;

use crate::config::{DEFAULT_PI_SPEC, LEGACY_PI_SPECS, ValidatedConfig};
use crate::util::shell_quote;

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
    let npm_global = cache_dir.join("npm-global");

    // 1. Ensure host dirs exist
    for dir in [&pnpm_home, &claude_install, &npm_global] {
        fs::create_dir_all(dir)
            .map_err(|e| UpdateAgentsError::HostDirCreate(format!("{}: {e}", dir.display())))?;
    }

    let configured_pi_spec = opts.pi_spec.as_deref().unwrap_or(&config.update.pi_spec);
    let pi_spec = resolve_pi_spec(configured_pi_spec);
    let release_age = opts
        .minimum_release_age
        .unwrap_or(config.update.minimum_release_age);

    // 2. Build the install script
    let script = build_install_script(pi_spec, release_age);

    // 3. Run throwaway container
    println!("Installing/updating agents in volumes...");
    if pi_spec == configured_pi_spec {
        println!("  PI spec: {pi_spec}");
    } else {
        println!("  PI spec: {pi_spec} (migrated from legacy {configured_pi_spec})");
    }
    println!("  pnpm minimum-release-age: {release_age}");

    let status = Command::new("podman")
        .args(build_podman_run_args(
            image,
            &pnpm_home,
            &claude_install,
            &npm_global,
            &script,
        ))
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

fn build_podman_run_args(
    image: &str,
    pnpm_home: &std::path::Path,
    claude_install: &std::path::Path,
    npm_global: &std::path::Path,
    script: &str,
) -> Vec<String> {
    vec![
        "run".to_owned(),
        "--rm".to_owned(),
        "-it".to_owned(),
        "--userns=keep-id".to_owned(),
        "--security-opt=label=disable".to_owned(),
        "-v".to_owned(),
        format!("{}:/usr/local/pnpm:rw", pnpm_home.display()),
        "-v".to_owned(),
        format!("{}:/opt/claude-home:rw", claude_install.display()),
        "-v".to_owned(),
        format!("{}:/home/dev/.npm-global:rw", npm_global.display()),
        image.to_owned(),
        "bash".to_owned(),
        "-c".to_owned(),
        script.to_owned(),
    ]
}

fn resolve_pi_spec(spec: &str) -> &str {
    if LEGACY_PI_SPECS.contains(&spec) {
        DEFAULT_PI_SPEC
    } else {
        spec
    }
}

fn legacy_pi_cleanup_script() -> String {
    LEGACY_PI_SPECS
        .iter()
        .map(|spec| format!("remove_pnpm_agent {} && \\\n", shell_quote(spec)))
        .collect()
}

fn build_install_script(pi_spec: &str, release_age: u32) -> String {
    let pi_spec = shell_quote(pi_spec);
    let legacy_pi_cleanup = legacy_pi_cleanup_script();

    // Always use the pnpm packaged in the sandbox image. `pnpm self-update` writes
    // pnpm's own shims into PNPM_HOME; those shims can shadow `/usr/bin/pnpm`
    // and drift to a different store layout than the global agent installs.
    format!(
        r#"set -e && \
mkdir -p "$HOME/.config/pnpm" /usr/local/pnpm && \
printf 'minimum-release-age=%s\nignore-scripts=true\nstore-dir=/usr/local/pnpm/.store\nglobal-bin-dir=/usr/local/pnpm\n' '{release_age}' > "$HOME/.config/pnpm/rc" && \
export PNPM_HOME=/usr/local/pnpm NPM_CONFIG_STORE_DIR=/usr/local/pnpm/.store NPM_CONFIG_GLOBAL_BIN_DIR=/usr/local/pnpm PATH=/usr/local/bin:/usr/bin:/bin:/usr/local/pnpm:/usr/local/pnpm/bin:$PATH && \
rm -f /usr/local/pnpm/pnpm /usr/local/pnpm/pn /usr/local/pnpm/pnpx /usr/local/pnpm/pnx /usr/local/pnpm/bin/pnpm /usr/local/pnpm/bin/pn /usr/local/pnpm/bin/pnpx /usr/local/pnpm/bin/pnx && \
rm -f /home/dev/.npm-global/bin/pi /home/dev/.npm-global/bin/codex /home/dev/.npm-global/bin/gemini /home/dev/.npm-global/bin/opencode && \
rm -rf /home/dev/.npm-global/lib/node_modules/@mariozechner/pi-coding-agent /home/dev/.npm-global/lib/node_modules/@earendil-works/pi-coding-agent /home/dev/.npm-global/lib/node_modules/@openai/codex /home/dev/.npm-global/lib/node_modules/@google/gemini-cli /home/dev/.npm-global/lib/node_modules/opencode-ai && \
PNPM_BIN=/usr/bin/pnpm && \
[ -x "$PNPM_BIN" ] && \
install_pnpm_agent() {{ \
  name="$1"; shift; \
  echo "[ags] updating $name..." >&2; \
  "$PNPM_BIN" add -g "$@" || return; \
  command -v "$name" >/dev/null 2>&1 || return; \
}} && \
remove_pnpm_agent() {{ \
  package="$1"; \
  echo "[ags] removing legacy $package..." >&2; \
  "$PNPM_BIN" remove -g "$package" >/dev/null 2>&1 || true; \
}} && \
{legacy_pi_cleanup}install_pnpm_agent pi {pi_spec} && \
install_pnpm_agent codex @openai/codex && \
install_pnpm_agent gemini @google/gemini-cli && \
install_pnpm_agent opencode opencode-ai && \
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
        legacy_pi_cleanup = legacy_pi_cleanup,
        pi_spec = pi_spec,
    )
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::config::{DEFAULT_PI_SPEC, LEGACY_PI_SPECS};

    use super::{build_install_script, build_podman_run_args, resolve_pi_spec};

    #[test]
    fn podman_run_args_disable_selinux_relabeling() {
        let args = build_podman_run_args(
            "localhost/agent-sandbox:latest",
            Path::new("/tmp/pnpm-home"),
            Path::new("/tmp/claude-home"),
            Path::new("/tmp/npm-global"),
            "echo ok",
        );

        assert!(args.contains(&"--security-opt=label=disable".to_owned()));
        assert!(
            args.windows(2)
                .any(|w| w[0] == "-v" && w[1] == "/tmp/pnpm-home:/usr/local/pnpm:rw")
        );
        assert!(
            args.windows(2)
                .any(|w| w[0] == "-v" && w[1] == "/tmp/claude-home:/opt/claude-home:rw")
        );
        assert!(
            args.windows(2)
                .any(|w| w[0] == "-v" && w[1] == "/tmp/npm-global:/home/dev/.npm-global:rw")
        );
        assert!(
            !args.iter().any(|arg| arg.contains(":rw,z")),
            "update-agents should not relabel mounted cache dirs"
        );
    }

    #[test]
    fn pnpm_agent_updates_do_not_fall_back_to_stale_pi() {
        let script = build_install_script(DEFAULT_PI_SPEC, 1440);

        let cleanup_pos = script
            .find("remove_pnpm_agent '@mariozechner/pi-coding-agent'")
            .expect("legacy Pi package should be removed before install");
        let install_pos = script
            .find("install_pnpm_agent pi '@earendil-works/pi-coding-agent'")
            .expect("current Pi package should be installed");
        assert!(cleanup_pos < install_pos);
        assert!(script.contains("install_pnpm_agent codex @openai/codex"));
        assert!(script.contains("install_pnpm_agent gemini @google/gemini-cli"));
        assert!(script.contains("install_pnpm_agent opencode opencode-ai"));
        assert!(script.contains("\"$PNPM_BIN\" add -g \"$@\" || return"));
        assert!(script.contains("PNPM_BIN=/usr/bin/pnpm"));
        assert!(
            !script.contains("using existing installs"),
            "pnpm update failures must not be masked by an existing stale pi binary"
        );
    }

    #[test]
    fn pnpm_update_uses_stable_store_and_ignores_stale_self_update_shims() {
        let script = build_install_script(DEFAULT_PI_SPEC, 1440);

        assert!(script.contains("store-dir=/usr/local/pnpm/.store"));
        assert!(script.contains("global-bin-dir=/usr/local/pnpm"));
        assert!(script.contains("NPM_CONFIG_STORE_DIR=/usr/local/pnpm/.store"));
        assert!(script.contains("NPM_CONFIG_GLOBAL_BIN_DIR=/usr/local/pnpm"));
        assert!(script.contains("rm -f /usr/local/pnpm/pnpm"));
        assert!(script.contains("/usr/local/pnpm/bin/pnpm"));
        assert!(script.contains("rm -f /home/dev/.npm-global/bin/pi"));
        assert!(
            script.contains("/home/dev/.npm-global/lib/node_modules/@mariozechner/pi-coding-agent"),
            "legacy npm-global Pi package should be cleaned up"
        );
        assert!(
            script.contains("install_pnpm_agent pi '@earendil-works/pi-coding-agent'"),
            "current Pi package should still be installed"
        );
        assert!(
            !script.contains("pnpm self-update"),
            "update-agents should not install pnpm into the agent runtime volume"
        );
    }

    #[test]
    fn legacy_pi_spec_resolves_to_current_default() {
        assert_eq!(resolve_pi_spec(LEGACY_PI_SPECS[0]), DEFAULT_PI_SPEC);
        assert_eq!(resolve_pi_spec("@custom/pi"), "@custom/pi");
    }

    #[test]
    fn pi_spec_is_shell_quoted_in_install_script() {
        let script = build_install_script("@scope/pkg; echo bad", 1440);

        assert!(script.contains("install_pnpm_agent pi '@scope/pkg; echo bad'"));
    }

    #[test]
    fn claude_update_still_uses_persistent_install_home() {
        let script = build_install_script(DEFAULT_PI_SPEC, 1440);

        assert!(
            script.contains(
                "HOME=\"$CLAUDE_HOME\" PATH=\"$CLAUDE_HOME/.local/bin:$PATH\" \"$CLAUDE_BIN\" update"
            ),
            "claude update should run with persistent CLAUDE_HOME"
        );
    }

    #[test]
    fn claude_wrapper_does_not_override_runtime_home() {
        let script = build_install_script(DEFAULT_PI_SPEC, 1440);

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
