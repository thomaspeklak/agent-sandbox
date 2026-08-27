use std::fmt;
use std::fs;
use std::process::Command;

use crate::agent::{OPENCODE_BINARY_PATH, OPENCODE_INSTALL_HOME};
use crate::config::{DEFAULT_PI_SPEC, LEGACY_PI_SPECS, ValidatedConfig};
use crate::github_release::resolve_latest_mature_release;
use crate::util::shell_quote;

const OPENCODE_REPO: &str = "anomalyco/opencode";
// Keep the immutable source revision and content hash together when deliberately updating it.
const OPENCODE_INSTALLER_URL: &str = "https://raw.githubusercontent.com/anomalyco/opencode/5f5ea53afb2630227ead917f1a0ddf784c33150c/install";
const OPENCODE_INSTALLER_SHA256: &str =
    "fc3c1b2123f49b6df545a7622e5127d21cd794b15134fc3b66e1ca49f7fb297e";

/// Options for the update-agents command.
#[derive(Default)]
pub struct UpdateAgentsOptions {
    pub pi_spec: Option<String>,
    pub minimum_release_age: Option<u32>,
}

#[derive(Debug)]
pub enum UpdateAgentsError {
    HostDirCreate(String),
    ReleaseResolveFailed(String),
    InstallFailed(String),
}

impl fmt::Display for UpdateAgentsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::HostDirCreate(msg) => write!(f, "failed to create host directory: {msg}"),
            Self::ReleaseResolveFailed(msg) => {
                write!(f, "failed to resolve mature OpenCode release: {msg}")
            }
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
    let codex_install = cache_dir.join("codex-install");
    let opencode_install = cache_dir.join("opencode-install");
    let claude_install = cache_dir.join("claude-install");
    let npm_global = cache_dir.join("npm-global");

    for dir in [
        &pnpm_home,
        &codex_install,
        &opencode_install,
        &claude_install,
        &npm_global,
    ] {
        fs::create_dir_all(dir)
            .map_err(|e| UpdateAgentsError::HostDirCreate(format!("{}: {e}", dir.display())))?;
    }

    let configured_pi_spec = opts.pi_spec.as_deref().unwrap_or(&config.update.pi_spec);
    let pi_spec = resolve_pi_spec(configured_pi_spec);
    let release_age = opts
        .minimum_release_age
        .unwrap_or(config.update.minimum_release_age);
    let opencode_release = resolve_latest_mature_release(OPENCODE_REPO, release_age, &[])
        .map_err(|error| UpdateAgentsError::ReleaseResolveFailed(error.to_string()))?;

    let script = build_install_script(pi_spec, release_age, &opencode_release.tag_name);

    println!("Installing/updating agents in volumes...");
    if pi_spec == configured_pi_spec {
        println!("  PI spec: {pi_spec}");
    } else {
        println!("  PI spec: {pi_spec} (migrated from legacy {configured_pi_spec})");
    }
    println!("  minimum release age: {release_age} minutes");
    println!("  OpenCode release: {}", opencode_release.tag_name);

    let status = Command::new("podman")
        .args(build_podman_run_args(
            image,
            &pnpm_home,
            &codex_install,
            &opencode_install,
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
    codex_install: &std::path::Path,
    opencode_install: &std::path::Path,
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
        format!("{}:{OPENCODE_INSTALL_HOME}:rw", opencode_install.display()),
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

fn build_install_script(pi_spec: &str, release_age: u32, opencode_version: &str) -> String {
    let pi_spec = shell_quote(pi_spec);
    let opencode_version = shell_quote(opencode_version);
    let legacy_pi_cleanup = legacy_pi_cleanup_script();

    // Always use the pnpm packaged in the sandbox image. `pnpm self-update` writes
    // pnpm's own shims into PNPM_HOME; those shims can shadow `/usr/local/bin/pnpm`
    // and drift to a different store layout than the global agent installs.
    format!(
        r#"set -e && \
mkdir -p "$HOME/.config/pnpm" /usr/local/pnpm {opencode_install_home} && \
printf 'minimum-release-age=%s\nignore-scripts=true\nstore-dir=/usr/local/pnpm/.store\nglobal-bin-dir=/usr/local/pnpm\n' '{release_age}' > "$HOME/.config/pnpm/rc" && \
export PNPM_HOME=/usr/local/pnpm NPM_CONFIG_STORE_DIR=/usr/local/pnpm/.store NPM_CONFIG_GLOBAL_BIN_DIR=/usr/local/pnpm PATH=/usr/local/bin:/usr/bin:/bin:/usr/local/pnpm:/usr/local/pnpm/bin:$PATH && \
PNPM_BIN=/usr/local/bin/pnpm && \
if ! [ -x "$PNPM_BIN" ] || ! "$PNPM_BIN" --version >/dev/null; then \
  echo "sandbox pnpm is unavailable; run 'ags update-image'" >&2; \
  exit 1; \
fi && \
rm -f /usr/local/pnpm/pnpm /usr/local/pnpm/pn /usr/local/pnpm/pnpx /usr/local/pnpm/pnx /usr/local/pnpm/bin/pnpm /usr/local/pnpm/bin/pn /usr/local/pnpm/bin/pnpx /usr/local/pnpm/bin/pnx && \
rm -f /home/dev/.npm-global/bin/pi /home/dev/.npm-global/bin/codex /home/dev/.npm-global/bin/gemini /home/dev/.npm-global/bin/opencode && \
rm -rf /home/dev/.npm-global/lib/node_modules/@mariozechner/pi-coding-agent /home/dev/.npm-global/lib/node_modules/@earendil-works/pi-coding-agent /home/dev/.npm-global/lib/node_modules/@openai/codex /home/dev/.npm-global/lib/node_modules/@google/gemini-cli /home/dev/.npm-global/lib/node_modules/opencode-ai && \
install_pnpm_agent() {{ \
  name="$1"; shift; \
  echo "[ags] updating $name..." >&2; \
  "$PNPM_BIN" add -g "$@" || return; \
  command -v "$name" >/dev/null 2>&1 || return; \
}} && \
remove_pnpm_agent() {{ \
  package="$1"; \
  echo "[ags] removing old $package..." >&2; \
  "$PNPM_BIN" remove -g "$package" >/dev/null 2>&1 || true; \
}} && \
{legacy_pi_cleanup}remove_pnpm_agent opencode-ai && \
install_pnpm_agent pi {pi_spec} && \
remove_pnpm_agent @openai/codex && \
echo '[ags] updating codex...' >&2 && \
curl -fsSL https://chatgpt.com/codex/install.sh -o /tmp/codex-install.sh && \
CODEX_HOME=/opt/codex-home CODEX_INSTALL_DIR=/usr/local/pnpm CODEX_NON_INTERACTIVE=true sh /tmp/codex-install.sh && \
[ -x /usr/local/pnpm/codex ] && \
install_pnpm_agent gemini @google/gemini-cli && \
echo '[ags] updating opencode...' >&2 && \
rm -f /usr/local/pnpm/opencode && \
rm -rf {opencode_install_home}/.opencode && \
OPENCODE_INSTALLER=/tmp/ags-opencode-install.sh && \
curl --proto '=https' --tlsv1.2 -fsSL '{opencode_installer_url}' -o "$OPENCODE_INSTALLER" && \
printf '{opencode_installer_sha256}  %s\n' "$OPENCODE_INSTALLER" | sha256sum -c - && \
HOME={opencode_install_home} bash "$OPENCODE_INSTALLER" --version {opencode_version} --no-modify-path && \
rm -f "$OPENCODE_INSTALLER" && \
[ -x {opencode_binary_path} ] && \
{opencode_binary_path} --version >/dev/null && \
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
        opencode_version = opencode_version,
        opencode_install_home = OPENCODE_INSTALL_HOME,
        opencode_binary_path = OPENCODE_BINARY_PATH,
        opencode_installer_url = OPENCODE_INSTALLER_URL,
        opencode_installer_sha256 = OPENCODE_INSTALLER_SHA256,
    )
}

#[cfg(test)]
#[path = "update_agents_tests.rs"]
mod tests;
