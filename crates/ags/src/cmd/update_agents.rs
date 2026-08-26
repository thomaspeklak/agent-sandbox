use std::fmt;
use std::fs;
use std::process::Command;

use crate::cli::Agent;
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

/// Reconcile selected agents in persistent volumes via a throwaway container.
pub fn run(config: &ValidatedConfig, opts: &UpdateAgentsOptions) -> Result<(), UpdateAgentsError> {
    let cache_dir = &config.sandbox.cache_dir;
    let image = &config.sandbox.image;

    let pnpm_home = cache_dir.join("pnpm-home");
    let codex_install = cache_dir.join("codex-install");
    let claude_install = cache_dir.join("claude-install");
    let npm_global = cache_dir.join("npm-global");

    // 1. Ensure host dirs exist
    for dir in [&pnpm_home, &codex_install, &claude_install, &npm_global] {
        fs::create_dir_all(dir)
            .map_err(|e| UpdateAgentsError::HostDirCreate(format!("{}: {e}", dir.display())))?;
    }

    let configured_pi_spec = opts.pi_spec.as_deref().unwrap_or(&config.update.pi_spec);
    let pi_spec = resolve_pi_spec(configured_pi_spec);
    let release_age = opts
        .minimum_release_age
        .unwrap_or(config.update.minimum_release_age);

    // 2. Build the install script
    let enabled_agents = &config.sandbox.enabled_agents;
    let script = build_install_script(pi_spec, release_age, enabled_agents);

    // 3. Run throwaway container
    println!("Reconciling agent CLIs in persistent volumes...");
    println!(
        "  enabled: {}",
        agent_list(enabled_agents).unwrap_or_else(|| "none (shell only)".to_owned())
    );
    let disabled_agents = Agent::INSTALLABLE
        .into_iter()
        .filter(|agent| !enabled_agents.contains(agent))
        .collect::<Vec<_>>();
    if let Some(disabled) = agent_list(&disabled_agents) {
        println!("  removing: {disabled}");
    }
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
            &codex_install,
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

    println!("\nDone. Agent CLI volumes reconciled.");
    if let Some(agent) = enabled_agents.first() {
        println!("Verify with: ags --agent {} -- --version", agent.as_str());
    } else {
        println!("No agent CLIs are enabled; `ags --agent shell` remains available.");
    }
    Ok(())
}

fn agent_list(agents: &[Agent]) -> Option<String> {
    (!agents.is_empty()).then(|| {
        agents
            .iter()
            .map(|agent| agent.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    })
}

fn build_podman_run_args(
    image: &str,
    pnpm_home: &std::path::Path,
    codex_install: &std::path::Path,
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
        format!("{}:/opt/codex-home:rw", codex_install.display()),
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

fn pnpm_package_name(spec: &str) -> &str {
    let version_at = if spec.starts_with('@') {
        spec.find('/')
            .and_then(|slash| spec[slash + 1..].find('@').map(|index| slash + index + 1))
    } else {
        spec.find('@')
    };
    version_at.map_or(spec, |index| &spec[..index])
}

fn legacy_pi_cleanup_script() -> String {
    LEGACY_PI_SPECS
        .iter()
        .map(|spec| format!("remove_legacy_pnpm_agent {} pi\n", shell_quote(spec)))
        .collect()
}

fn build_install_script(pi_spec: &str, release_age: u32, enabled_agents: &[Agent]) -> String {
    let pi_package = shell_quote(pnpm_package_name(pi_spec));
    let pi_spec = shell_quote(pi_spec);
    let legacy_pi_cleanup = legacy_pi_cleanup_script();
    let pi_action = if enabled_agents.contains(&Agent::Pi) {
        format!("{legacy_pi_cleanup}install_pnpm_agent pi {pi_spec}")
    } else {
        format!("{legacy_pi_cleanup}remove_pnpm_agent {pi_package} pi")
    };
    let codex_action = if enabled_agents.contains(&Agent::Codex) {
        r#"remove_legacy_pnpm_agent @openai/codex codex
echo '[ags] updating codex...' >&2
curl -fsSL https://chatgpt.com/codex/install.sh -o /tmp/codex-install.sh
CODEX_HOME=/opt/codex-home CODEX_INSTALL_DIR=/usr/local/pnpm CODEX_NON_INTERACTIVE=true sh /tmp/codex-install.sh
[ -x /usr/local/pnpm/codex ]"#
    } else {
        r#"remove_legacy_pnpm_agent @openai/codex codex
echo '[ags] removing codex...' >&2
rm -f /usr/local/pnpm/codex
rm -rf /opt/codex-home/* /opt/codex-home/.[!.]* /opt/codex-home/..?*"#
    };
    let gemini_action = if enabled_agents.contains(&Agent::Gemini) {
        "install_pnpm_agent gemini @google/gemini-cli"
    } else {
        "remove_pnpm_agent @google/gemini-cli gemini"
    };
    let opencode_action = if enabled_agents.contains(&Agent::Opencode) {
        r#"install_pnpm_agent opencode opencode-ai
OPENCODE_LIST="$("$PNPM_BIN" list -g opencode-ai --depth=0 --parseable)"
OPENCODE_PATHS="$(printf '%s\n' "$OPENCODE_LIST" | grep '/node_modules/opencode-ai$' || true)"
OPENCODE_PATH_COUNT="$(printf '%s\n' "$OPENCODE_PATHS" | grep -c . || true)"
if [ "$OPENCODE_PATH_COUNT" -ne 1 ]; then
  echo "expected exactly one global opencode-ai package path, found $OPENCODE_PATH_COUNT" >&2
  exit 1
fi
OPENCODE_ROOT="$OPENCODE_PATHS"
[ -f "$OPENCODE_ROOT/postinstall.mjs" ]
node "$OPENCODE_ROOT/postinstall.mjs"
opencode --version >/dev/null"#
    } else {
        "remove_pnpm_agent opencode-ai opencode"
    };
    let claude_action = if enabled_agents.contains(&Agent::Claude) {
        r#"CLAUDE_HOME=/opt/claude-home
CLAUDE_BIN="$CLAUDE_HOME/.local/bin/claude"
if [ -x "$CLAUDE_BIN" ]; then
  HOME="$CLAUDE_HOME" PATH="$CLAUDE_HOME/.local/bin:$PATH" "$CLAUDE_BIN" update || {
    echo 'claude update failed; reinstalling via install.sh' >&2
    export HOME="$CLAUDE_HOME" PATH="$CLAUDE_HOME/.local/bin:$PATH"
    curl -fsSL https://claude.ai/install.sh | bash
  }
else
  export HOME="$CLAUDE_HOME" PATH="$CLAUDE_HOME/.local/bin:$PATH"
  curl -fsSL https://claude.ai/install.sh | bash
fi
[ -x "$CLAUDE_BIN" ]
rm -f /usr/local/pnpm/claude
printf '%s\n' '#!/usr/bin/env bash' 'export PATH=/opt/claude-home/.local/bin:$PATH' 'exec /opt/claude-home/.local/bin/claude "$@"' > /usr/local/pnpm/claude
chmod +x /usr/local/pnpm/claude"#
    } else {
        r#"echo '[ags] removing claude...' >&2
rm -f /usr/local/pnpm/claude
rm -rf /opt/claude-home/* /opt/claude-home/.[!.]* /opt/claude-home/..?*"#
    };

    // Always use the pnpm packaged in the sandbox image. `pnpm self-update` writes
    // pnpm's own shims into PNPM_HOME; those shims can shadow `/usr/local/bin/pnpm`
    // and drift to a different store layout than the global agent installs.
    format!(
        r#"set -e
mkdir -p "$HOME/.config/pnpm" /usr/local/pnpm /opt/codex-home /opt/claude-home
printf 'minimum-release-age=%s\nignore-scripts=true\nstore-dir=/usr/local/pnpm/.store\nglobal-bin-dir=/usr/local/pnpm\n' '{release_age}' > "$HOME/.config/pnpm/rc"
export PNPM_HOME=/usr/local/pnpm NPM_CONFIG_STORE_DIR=/usr/local/pnpm/.store NPM_CONFIG_GLOBAL_BIN_DIR=/usr/local/pnpm PATH=/usr/local/bin:/usr/bin:/bin:/usr/local/pnpm:/usr/local/pnpm/bin:$PATH
PNPM_BIN=/usr/local/bin/pnpm
if ! [ -x "$PNPM_BIN" ] || ! "$PNPM_BIN" --version >/dev/null; then
  echo "sandbox pnpm is unavailable; run 'ags update-image'" >&2
  exit 1
fi
rm -f /usr/local/pnpm/pnpm /usr/local/pnpm/pn /usr/local/pnpm/pnpx /usr/local/pnpm/pnx /usr/local/pnpm/bin/pnpm /usr/local/pnpm/bin/pn /usr/local/pnpm/bin/pnpx /usr/local/pnpm/bin/pnx
rm -f /home/dev/.npm-global/bin/pi /home/dev/.npm-global/bin/codex /home/dev/.npm-global/bin/gemini /home/dev/.npm-global/bin/opencode
rm -rf /home/dev/.npm-global/lib/node_modules/@mariozechner/pi-coding-agent /home/dev/.npm-global/lib/node_modules/@earendil-works/pi-coding-agent /home/dev/.npm-global/lib/node_modules/@openai/codex /home/dev/.npm-global/lib/node_modules/@google/gemini-cli /home/dev/.npm-global/lib/node_modules/opencode-ai
install_pnpm_agent() {{
  name="$1"; shift
  echo "[ags] updating $name..." >&2
  "$PNPM_BIN" add -g "$@" || return
  command -v "$name" >/dev/null 2>&1 || return
}}
remove_pnpm_agent() {{
  package="$1"
  name="$2"
  echo "[ags] removing $package..." >&2
  package_paths="$("$PNPM_BIN" list -g "$package" --depth=0 --parseable)" || return
  if printf '%s\n' "$package_paths" | grep -Fq "/node_modules/${{package}}"; then
    "$PNPM_BIN" remove -g "$package" >/dev/null || return
  fi
  rm -f "/usr/local/pnpm/$name" "/usr/local/pnpm/bin/$name" || return
  package_paths="$("$PNPM_BIN" list -g "$package" --depth=0 --parseable)" || return
  if printf '%s\n' "$package_paths" | grep -Fq "/node_modules/${{package}}" || [ -e "/usr/local/pnpm/$name" ] || [ -e "/usr/local/pnpm/bin/$name" ]; then
    echo "failed to remove $package runtime" >&2
    return 1
  fi
}}
remove_legacy_pnpm_agent() {{
  remove_pnpm_agent "$@" || echo "warning: could not fully clean obsolete package $1" >&2
}}
{pi_action}
{codex_action}
{gemini_action}
{opencode_action}
{claude_action}
"$PNPM_BIN" store prune
"#,
    )
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use crate::cli::Agent;
    use crate::config::{DEFAULT_PI_SPEC, LEGACY_PI_SPECS};

    use super::{build_install_script, build_podman_run_args, pnpm_package_name, resolve_pi_spec};

    fn all_agents_script(pi_spec: &str, release_age: u32) -> String {
        build_install_script(pi_spec, release_age, &Agent::INSTALLABLE)
    }

    #[test]
    fn podman_run_args_disable_selinux_relabeling() {
        let args = build_podman_run_args(
            "localhost/agent-sandbox:latest",
            Path::new("/tmp/pnpm-home"),
            Path::new("/tmp/codex-home"),
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
                .any(|w| w[0] == "-v" && w[1] == "/tmp/codex-home:/opt/codex-home:rw")
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
        let script = all_agents_script(DEFAULT_PI_SPEC, 1440);

        let cleanup_pos = script
            .find("remove_legacy_pnpm_agent '@mariozechner/pi-coding-agent' pi")
            .expect("legacy Pi package should be removed before install");
        let install_pos = script
            .find("install_pnpm_agent pi '@earendil-works/pi-coding-agent'")
            .expect("current Pi package should be installed");
        assert!(cleanup_pos < install_pos);
        assert!(script.contains("remove_legacy_pnpm_agent @openai/codex codex"));
        assert!(script.contains("https://chatgpt.com/codex/install.sh"));
        assert!(script.contains("CODEX_HOME=/opt/codex-home"));
        assert!(script.contains("CODEX_INSTALL_DIR=/usr/local/pnpm"));
        assert!(script.contains("CODEX_NON_INTERACTIVE=true"));
        assert!(script.contains("install_pnpm_agent gemini @google/gemini-cli"));
        assert!(script.contains("install_pnpm_agent opencode opencode-ai"));
        assert!(script.contains("\"$PNPM_BIN\" add -g \"$@\" || return"));
        assert!(script.contains("PNPM_BIN=/usr/local/bin/pnpm"));
        let preflight_pos = script
            .find("\"$PNPM_BIN\" --version >/dev/null")
            .expect("image pnpm should be executed before package updates");
        assert!(preflight_pos < cleanup_pos);
        assert!(preflight_pos < script.find("rm -f /usr/local/pnpm/pnpm").unwrap());
        assert!(script.contains("run 'ags update-image'"));
        assert!(
            !script.contains("using existing installs"),
            "pnpm update failures must not be masked by an existing stale pi binary"
        );
    }

    #[test]
    fn pnpm_update_uses_stable_store_and_ignores_stale_self_update_shims() {
        let script = all_agents_script(DEFAULT_PI_SPEC, 1440);

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
    fn opencode_postinstall_resolves_isolated_global_package_before_runtime_validation() {
        let script = all_agents_script(DEFAULT_PI_SPEC, 1440);

        let install_pos = script
            .find("install_pnpm_agent opencode opencode-ai")
            .expect("OpenCode should be installed by pnpm");
        let root_pos = script
            .find("\"$PNPM_BIN\" list -g opencode-ai --depth=0 --parseable")
            .expect("pnpm should report the isolated OpenCode package directory");
        let count_pos = script
            .find("OPENCODE_PATH_COUNT=")
            .expect("matching OpenCode package directories should be counted");
        let unique_path_pos = script
            .find("[ \"$OPENCODE_PATH_COUNT\" -ne 1 ]")
            .expect("OpenCode should require exactly one package directory");
        let diagnostic_pos = script
            .find("expected exactly one global opencode-ai package path")
            .expect("invalid package path counts should produce a diagnostic");
        let postinstall_check_pos = script
            .find("[ -f \"$OPENCODE_ROOT/postinstall.mjs\" ]")
            .expect("the resolved OpenCode postinstall script should exist");
        let postinstall_pos = script
            .find("node \"$OPENCODE_ROOT/postinstall.mjs\"")
            .expect("OpenCode's required postinstall script should run explicitly");
        let validation_pos = script
            .find("opencode --version >/dev/null")
            .expect("the installed OpenCode binary should be executed");

        assert!(script.contains("ignore-scripts=true"));
        assert!(script.contains("grep '/node_modules/opencode-ai$'"));
        assert!(script.contains("grep -c . || true"));
        assert!(!script.contains("\"$PNPM_BIN\" root -g"));
        assert!(install_pos < root_pos);
        assert!(root_pos < count_pos);
        assert!(count_pos < unique_path_pos);
        assert!(unique_path_pos < diagnostic_pos);
        assert!(diagnostic_pos < postinstall_check_pos);
        assert!(postinstall_check_pos < postinstall_pos);
        assert!(postinstall_pos < validation_pos);
    }

    #[test]
    fn legacy_pi_spec_resolves_to_current_default() {
        assert_eq!(resolve_pi_spec(LEGACY_PI_SPECS[0]), DEFAULT_PI_SPEC);
        assert_eq!(resolve_pi_spec("@custom/pi"), "@custom/pi");
    }

    #[test]
    fn pi_spec_is_shell_quoted_in_install_script() {
        let script = all_agents_script("@scope/pkg; echo bad", 1440);

        assert!(script.contains("install_pnpm_agent pi '@scope/pkg; echo bad'"));
        assert_eq!(pnpm_package_name("@scope/pkg@1.2.3"), "@scope/pkg");
    }

    #[test]
    fn claude_update_still_uses_persistent_install_home() {
        let script = all_agents_script(DEFAULT_PI_SPEC, 1440);

        assert!(
            script.contains(
                "HOME=\"$CLAUDE_HOME\" PATH=\"$CLAUDE_HOME/.local/bin:$PATH\" \"$CLAUDE_BIN\" update"
            ),
            "claude update should run with persistent CLAUDE_HOME"
        );
    }

    #[test]
    fn claude_wrapper_does_not_override_runtime_home() {
        let script = all_agents_script(DEFAULT_PI_SPEC, 1440);

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

    #[test]
    fn deselected_agents_are_removed_instead_of_installed() {
        let script = build_install_script(DEFAULT_PI_SPEC, 1440, &[Agent::Pi]);

        assert!(script.contains("install_pnpm_agent pi '@earendil-works/pi-coding-agent'"));
        assert!(!script.contains("https://chatgpt.com/codex/install.sh"));
        assert!(!script.contains("install_pnpm_agent gemini @google/gemini-cli"));
        assert!(!script.contains("install_pnpm_agent opencode opencode-ai"));
        assert!(!script.contains("https://claude.ai/install.sh"));
        assert!(script.contains("rm -rf /opt/codex-home/*"));
        assert!(script.contains("remove_pnpm_agent @google/gemini-cli gemini"));
        assert!(script.contains("remove_pnpm_agent opencode-ai opencode"));
        assert!(script.contains("rm -rf /opt/claude-home/*"));
        assert!(script.contains("\"$PNPM_BIN\" store prune"));
        assert!(script.contains("failed to remove $package runtime"));
        assert!(script.contains("\"/usr/local/pnpm/bin/$name\""));
        assert!(!script.contains("remove -g \"$package\" >/dev/null 2>&1 || true"));
    }
}
