use std::path::Path;
use std::process::Command;

use crate::agent::OPENCODE_BINARY_PATH;
use crate::config::{DEFAULT_PI_SPEC, LEGACY_PI_SPECS};

use super::{
    OPENCODE_INSTALLER_SHA256, OPENCODE_INSTALLER_URL, build_install_script, build_podman_run_args,
    resolve_pi_spec,
};

const OPENCODE_VERSION: &str = "v1.2.3";

fn script() -> String {
    build_install_script(DEFAULT_PI_SPEC, 1_440, OPENCODE_VERSION)
}

#[test]
fn podman_run_args_disable_selinux_relabeling() {
    let args = build_podman_run_args(
        "localhost/agent-sandbox:latest",
        Path::new("/tmp/pnpm-home"),
        Path::new("/tmp/codex-home"),
        Path::new("/tmp/opencode-home"),
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
            .any(|w| { w[0] == "-v" && w[1] == "/tmp/opencode-home:/opt/opencode-home:rw" })
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
    let script = script();

    let cleanup_pos = script
        .find("remove_pnpm_agent '@mariozechner/pi-coding-agent'")
        .expect("legacy Pi package should be removed before install");
    let install_pos = script
        .find("install_pnpm_agent pi '@earendil-works/pi-coding-agent'")
        .expect("current Pi package should be installed");
    assert!(cleanup_pos < install_pos);
    assert!(script.contains("remove_pnpm_agent @openai/codex"));
    assert!(script.contains("https://chatgpt.com/codex/install.sh"));
    assert!(script.contains("CODEX_HOME=/opt/codex-home"));
    assert!(script.contains("CODEX_INSTALL_DIR=/usr/local/pnpm"));
    assert!(script.contains("CODEX_NON_INTERACTIVE=true"));
    assert!(script.contains("install_pnpm_agent gemini @google/gemini-cli"));
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
    let script = script();

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
fn opencode_binary_installer_uses_an_immutable_verified_source_and_mature_version() {
    let script = script();

    let remove_old_package_pos = script
        .find("remove_pnpm_agent opencode-ai")
        .expect("the old package should be removed before binary installation");
    let stage_pos = script
        .find("OPENCODE_STAGE_HOME=/opt/opencode-home/.opencode-stage")
        .expect("OpenCode should install into a separate staging directory");
    let download_pos = script
        .find(OPENCODE_INSTALLER_URL)
        .expect("the official installer should be pinned to an immutable commit");
    let checksum_pos = script
        .find(OPENCODE_INSTALLER_SHA256)
        .expect("the downloaded installer should be verified");
    let install_pos = script
        .find("HOME=\"$OPENCODE_STAGE_HOME\" bash \"$OPENCODE_INSTALLER\" --version v1.2.3 --no-modify-path")
        .expect("the pinned installer should receive the resolved version in staging without modifying shell files");
    let stage_validation_pos = script
        .find("\"$OPENCODE_STAGE_PATH/bin/opencode\" --version >/dev/null")
        .expect("the staged OpenCode binary should be checked before activation");
    let backup_pos = script
        .find("mv \"$OPENCODE_ACTIVE_PATH\" \"$OPENCODE_BACKUP_PATH\"")
        .expect("the active OpenCode install should be backed up only after staging validates");
    let activation_pos = script
        .find("mv \"$OPENCODE_STAGE_PATH\" \"$OPENCODE_ACTIVE_PATH\"")
        .expect("the validated staging directory should replace the active install");
    let legacy_cleanup_pos = script
        .find("rm -f /usr/local/pnpm/opencode")
        .expect("the prior OpenCode location should be removed after migration succeeds");
    assert!(
        !script.contains("rm -rf /opt/opencode-home/.opencode"),
        "the active OpenCode install must survive until staging validates"
    );
    assert!(
        script.contains("trap cleanup_opencode_stage EXIT") && script.contains("trap - EXIT"),
        "failed staged updates should clean temporary artifacts and restore the active install"
    );
    assert!(
        !script.contains("ln -s .opencode/bin/opencode"),
        "OpenCode should not be exposed through the pnpm runtime volume"
    );
    let validation_pos = script
        .find(&format!("{OPENCODE_BINARY_PATH} --version >/dev/null"))
        .expect("the installed OpenCode runtime command should execute directly");

    assert!(script.contains("curl --proto '=https' --tlsv1.2 -fsSL"));
    assert!(script.contains("sha256sum -c -"));
    assert!(script.contains("ignore-scripts=true"));
    assert!(!script.contains("install_pnpm_agent opencode"));
    assert!(!script.contains("postinstall.mjs"));
    assert!(!script.contains("list -g opencode-ai"));
    assert!(remove_old_package_pos < stage_pos);
    assert!(stage_pos < download_pos);
    assert!(download_pos < checksum_pos);
    assert!(checksum_pos < install_pos);
    assert!(install_pos < stage_validation_pos);
    assert!(stage_validation_pos < backup_pos);
    assert!(backup_pos < activation_pos);
    assert!(activation_pos < validation_pos);
    assert!(validation_pos < legacy_cleanup_pos);
}

#[test]
fn generated_install_script_is_valid_bash() {
    let status = Command::new("bash")
        .args(["-n", "-c", &script()])
        .status()
        .expect("bash should be available for generated script validation");

    assert!(status.success());
}

#[test]
fn legacy_pi_spec_resolves_to_current_default() {
    assert_eq!(resolve_pi_spec(LEGACY_PI_SPECS[0]), DEFAULT_PI_SPEC);
    assert_eq!(resolve_pi_spec("@custom/pi"), "@custom/pi");
}

#[test]
fn pi_spec_and_opencode_version_are_shell_quoted_in_install_script() {
    let script = build_install_script("@scope/pkg; echo bad", 1_440, "v1.2.3; echo bad");

    assert!(script.contains("install_pnpm_agent pi '@scope/pkg; echo bad'"));
    assert!(script.contains("--version 'v1.2.3; echo bad' --no-modify-path"));
}

#[test]
fn claude_update_still_uses_persistent_install_home() {
    let script = script();

    assert!(
        script.contains(
            "HOME=\"$CLAUDE_HOME\" PATH=\"$CLAUDE_HOME/.local/bin:$PATH\" \"$CLAUDE_BIN\" update"
        ),
        "claude update should run with persistent CLAUDE_HOME"
    );
}

#[test]
fn claude_wrapper_does_not_override_runtime_home() {
    let script = script();

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
