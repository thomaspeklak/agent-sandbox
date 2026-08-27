use std::fmt;
use std::fs;
use std::process::Command;

use crate::cli::Agent;
use crate::config::ValidatedConfig;
use crate::util::shell_quote;

#[path = "update_agents_script.rs"]
mod script;
use script::{build_install_script, resolve_pi_spec};

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
        println!(
            "Verify with: {}",
            verification_command(*agent, &config.config_file)
        );
    } else {
        println!("No agent CLIs are enabled; `ags --agent shell` remains available.");
    }
    Ok(())
}

fn verification_command(agent: Agent, config_file: &std::path::Path) -> String {
    format!(
        "ags --agent {} --config {} -- --version",
        agent.as_str(),
        shell_quote(&config_file.display().to_string())
    )
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

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::process::Command;

    use crate::cli::Agent;
    use crate::config::{DEFAULT_PI_SPEC, LEGACY_PI_SPECS};

    use super::{
        build_install_script, build_podman_run_args, resolve_pi_spec, verification_command,
    };

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
            .expect("legacy Pi package should be removed after install");
        let install_pos = script
            .find("install_pnpm_agent pi '@earendil-works/pi-coding-agent'")
            .expect("current Pi package should be installed");
        assert!(install_pos < cleanup_pos);
        let codex_install_pos = script
            .find("CODEX_NON_INTERACTIVE=true sh /tmp/codex-install.sh")
            .expect("Codex should be installed");
        let codex_cleanup_pos = script
            .find("remove_legacy_pnpm_agent @openai/codex codex")
            .expect("legacy Codex package should be removed");
        assert!(codex_install_pos < codex_cleanup_pos);
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
    }

    #[test]
    fn pi_removal_discovers_installed_dependency_keys() {
        let script = build_install_script(DEFAULT_PI_SPEC, 1440, &[Agent::Codex]);

        assert!(script.contains("remove_pnpm_agents_for_bin pi"));
        assert!(script.contains("list -g --depth=0 --json"));
        assert!(script.contains("Object.entries(root.dependencies || {})"));
        assert!(script.contains("path.join(dependency.path, \"package.json\")"));
        assert!(script.contains("Object.prototype.hasOwnProperty.call(manifest.bin, command)"));
        assert!(!script.contains("remove_pnpm_agent '@earendil-works/pi-coding-agent' pi"));
    }

    #[test]
    fn legacy_cleanup_preserves_launchers_not_owned_by_the_old_package() {
        let script = all_agents_script(DEFAULT_PI_SPEC, 1440);

        assert!(script.contains("[ \"$status\" -eq 3 ] && return 0"));
        assert!(script.contains("backup_unowned_launcher \"$root_launcher\" \"$package_path\""));
        assert!(script.contains("cp -a \"$launcher\" \"$backup\""));
        assert!(script.contains("restore_preserved_launcher \"$root_launcher\""));
    }

    #[test]
    fn generated_reconciliation_script_has_valid_bash_syntax() {
        let script = all_agents_script(DEFAULT_PI_SPEC, 1440);
        let status = Command::new("bash")
            .args(["-n", "-c", &script])
            .status()
            .unwrap();

        assert!(status.success());
    }

    #[test]
    fn verification_command_keeps_and_quotes_the_active_config() {
        assert_eq!(
            verification_command(Agent::Pi, Path::new("/tmp/owner's config.toml")),
            "ags --agent pi --config '/tmp/owner'\\''s config.toml' -- --version"
        );
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
