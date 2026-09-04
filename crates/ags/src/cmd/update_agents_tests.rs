use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

use crate::cli::Agent;
use crate::config::{
    ArchiveMemberMatch, DEFAULT_PI_SPEC, LEGACY_PI_SPECS, LockedAgentProvider, ToolArchiveFormat,
    ToolDownloadArtifact, ToolDownloadSource,
};

use super::{
    build_install_script, build_podman_run_args, recover_opencode_transaction,
    resolve_opencode_with_recovery, resolve_pi_spec, verification_command,
};

fn default_providers() -> Vec<LockedAgentProvider> {
    serde_json::from_str(crate::assets::DEFAULT_AGENT_PROVIDERS_LOCK).unwrap()
}

fn opencode_download() -> ToolDownloadSource {
    ToolDownloadSource {
        version: "1.2.3".to_owned(),
        archive: ToolArchiveFormat::TarGz,
        member: "opencode".to_owned(),
        member_match: ArchiveMemberMatch::Exact,
        install_as: "opencode".to_owned(),
        artifacts: BTreeMap::from([
            (
                "x86_64".to_owned(),
                ToolDownloadArtifact {
                    url: "https://example.test/opencode-x64.tar.gz".to_owned(),
                    sha256: "a".repeat(64),
                },
            ),
            (
                "aarch64".to_owned(),
                ToolDownloadArtifact {
                    url: "https://example.test/opencode-arm64.tar.gz".to_owned(),
                    sha256: "b".repeat(64),
                },
            ),
        ]),
    }
}

fn all_agents_script() -> String {
    build_install_script(
        DEFAULT_PI_SPEC,
        1_440,
        &Agent::INSTALLABLE,
        &default_providers(),
        Some(&opencode_download()),
    )
    .unwrap()
}

#[test]
fn podman_run_args_mount_all_reconciliation_volumes_without_relabeling() {
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
    for mount in [
        "/tmp/pnpm-home:/usr/local/pnpm:rw",
        "/tmp/codex-home:/opt/codex-home:rw",
        "/tmp/opencode-home:/opt/opencode-home:rw",
        "/tmp/claude-home:/opt/claude-home:rw",
        "/tmp/npm-global:/home/dev/.npm-global:rw",
    ] {
        assert!(
            args.windows(2)
                .any(|pair| pair[0] == "-v" && pair[1] == mount)
        );
    }
    assert!(!args.iter().any(|arg| arg.contains(":rw,z")));
}

#[test]
fn selected_agents_are_installed_with_stable_pnpm_state() {
    let script = all_agents_script();

    assert!(script.contains("store-dir=/usr/local/pnpm/.store"));
    assert!(script.contains("install_pnpm_candidate pi '@earendil-works/pi-coding-agent'"));
    assert!(script.contains("https://chatgpt.com/codex/install.sh"));
    assert!(script.contains("install_pnpm_candidate gemini '@google/gemini-cli'"));
    assert!(script.contains("exec /opt/claude-home/.local/bin/claude \"$@\""));
    assert!(!script.contains("pnpm self-update"));
    assert!(!script.contains("install_pnpm_candidate opencode"));
}

#[test]
fn pnpm_install_unlinks_only_the_old_launcher_until_the_candidate_is_verified() {
    let script = all_agents_script();
    let helper = script
        .split_once("install_pnpm_candidate() {")
        .unwrap()
        .1
        .split_once("\n}\ncommit_pnpm_agent()")
        .unwrap()
        .0;

    let backup = helper.find("backup_pnpm_agent_launchers").unwrap();
    let unlink = helper.find("rm -f \"/usr/local/pnpm/$name\"").unwrap();
    let install = helper.find("add -g").unwrap();
    let ownership = helper.find("verify_pnpm_agent").unwrap();
    assert!(backup < unlink);
    assert!(unlink < install);
    assert!(install < ownership);
    assert!(helper.contains("restore_pnpm_agent_launchers"));
    assert!(helper.contains("! remove_pnpm_dependency \"$package\""));
}

#[test]
fn pnpm_reconciliation_protects_enabled_packages_and_verifies_them_last() {
    let script = all_agents_script();
    let pi_install = script
        .find("install_pnpm_candidate pi '@earendil-works/pi-coding-agent'")
        .unwrap();
    let gemini_install = script
        .find("install_pnpm_candidate gemini '@google/gemini-cli'")
        .unwrap();
    let pi_cleanup = script
        .find("commit_pnpm_agent pi '@earendil-works/pi-coding-agent' '@earendil-works/pi-coding-agent' '@google/gemini-cli'")
        .unwrap();
    let gemini_cleanup = script
        .find("commit_pnpm_agent gemini '@google/gemini-cli' '@earendil-works/pi-coding-agent' '@google/gemini-cli'")
        .unwrap();
    let final_pi = script
        .rfind("verify_pnpm_agent '@earendil-works/pi-coding-agent' pi")
        .unwrap();
    let final_gemini = script
        .rfind("verify_pnpm_agent '@google/gemini-cli' gemini")
        .unwrap();

    assert!(pi_install < gemini_install);
    assert!(gemini_install < pi_cleanup);
    assert!(gemini_install < gemini_cleanup);
    assert!(gemini_cleanup < final_pi);
    assert!(gemini_cleanup < final_gemini);
    assert!(script.contains("global-bin-dir=/usr/local/pnpm/bin"));
}

#[test]
fn provider_policy_controls_gemini_package() {
    let mut providers = default_providers();
    let gemini = providers
        .iter_mut()
        .find(|entry| entry.agent == Agent::Gemini)
        .unwrap();
    gemini.provider = crate::config::AgentProviderPolicy::Pnpm {
        package: "@example/gemini-cli".to_owned(),
    };

    let script =
        build_install_script(DEFAULT_PI_SPEC, 1_440, &[Agent::Gemini], &providers, None).unwrap();

    assert!(script.contains("install_pnpm_candidate gemini '@example/gemini-cli'"));
}

#[test]
fn enabled_agent_requires_provider() {
    let error = build_install_script(DEFAULT_PI_SPEC, 1_440, &[Agent::Pi], &[], None).unwrap_err();
    assert!(error.contains("enabled agent 'pi' has no provider"));
}

#[test]
fn opencode_uses_verified_transactional_release_install() {
    let script = all_agents_script();
    let checksum = script.find("sha256sum -c -").unwrap();
    let extraction = script.find("tar -xOzf").unwrap();
    let stage_validation = script
        .find("OPENCODE_STAGE/bin/opencode\" --version")
        .unwrap();
    let backup = script
        .find("mv \"$OPENCODE_ACTIVE\" \"$OPENCODE_BACKUP\"")
        .unwrap();
    let activation = script
        .find("mv \"$OPENCODE_STAGE\" \"$OPENCODE_ACTIVE\"")
        .unwrap();
    let commit = script.rfind("rm -f \"$OPENCODE_TRANSACTION\"").unwrap();
    let legacy_cleanup = script
        .find("remove_pnpm_agent opencode-ai opencode")
        .unwrap();

    assert!(script.contains("https://example.test/opencode-x64.tar.gz"));
    assert!(script.contains("https://example.test/opencode-arm64.tar.gz"));
    assert!(script.contains(&"a".repeat(64)));
    assert!(script.contains(&"b".repeat(64)));
    assert!(script.contains("unsupported architecture for OpenCode"));
    assert!(script.contains("trap recover_opencode_transaction EXIT"));
    assert!(checksum < extraction);
    assert!(extraction < stage_validation);
    assert!(stage_validation < backup);
    assert!(backup < activation);
    assert!(activation < commit);
    assert!(commit < legacy_cleanup);
}

#[test]
fn generated_recovery_runs_before_other_agent_actions() {
    let script = all_agents_script();
    let recovery = script.find("recover_opencode_transaction\n").unwrap();
    let pi = script.find("install_pnpm_candidate pi").unwrap();
    let codex = script.find("[ags] updating codex").unwrap();
    let gemini = script.find("install_pnpm_candidate gemini").unwrap();

    assert!(recovery < pi);
    assert!(recovery < codex);
    assert!(recovery < gemini);
}

#[test]
fn host_recovery_restores_backup_before_release_resolution() {
    let dir = tempfile::tempdir().unwrap();
    let active = dir.path().join(".opencode");
    let backup = dir.path().join(".opencode.previous");
    let stage = dir.path().join(".opencode.stage");
    std::fs::create_dir_all(&active).unwrap();
    std::fs::write(active.join("broken"), "broken").unwrap();
    std::fs::create_dir_all(&backup).unwrap();
    std::fs::write(backup.join("working"), "working").unwrap();
    std::fs::create_dir_all(&stage).unwrap();
    std::fs::write(dir.path().join(".opencode.transaction"), "").unwrap();

    let download = resolve_opencode_with_recovery(
        dir.path(),
        &[Agent::Opencode],
        &default_providers(),
        1_440,
        |_, _| {
            assert!(active.join("working").is_file());
            assert!(!active.join("broken").exists());
            assert!(!backup.exists());
            assert!(!stage.exists());
            assert!(!dir.path().join(".opencode.transaction").exists());
            Ok(opencode_download())
        },
    )
    .unwrap();

    assert!(download.is_some());
}

#[test]
fn host_recovery_handles_interruption_before_transaction_marker() {
    let dir = tempfile::tempdir().unwrap();
    let backup = dir.path().join(".opencode.previous");
    std::fs::create_dir_all(&backup).unwrap();
    std::fs::write(backup.join("working"), "working").unwrap();

    recover_opencode_transaction(dir.path()).unwrap();

    assert!(dir.path().join(".opencode/working").is_file());
    assert!(!backup.exists());
}

#[test]
fn host_recovery_keeps_active_install_and_removes_stale_backup() {
    let dir = tempfile::tempdir().unwrap();
    let active = dir.path().join(".opencode");
    let backup = dir.path().join(".opencode.previous");
    std::fs::create_dir_all(&active).unwrap();
    std::fs::write(active.join("current"), "current").unwrap();
    std::fs::create_dir_all(&backup).unwrap();
    std::fs::write(backup.join("stale"), "stale").unwrap();

    recover_opencode_transaction(dir.path()).unwrap();

    assert!(active.join("current").is_file());
    assert!(!backup.exists());
}

#[test]
fn deselected_agents_are_removed_without_resolving_opencode() {
    let script = build_install_script(
        DEFAULT_PI_SPEC,
        1_440,
        &[Agent::Pi],
        &default_providers(),
        None,
    )
    .unwrap();

    assert!(script.contains("install_pnpm_candidate pi '@earendil-works/pi-coding-agent'"));
    assert!(script.contains("rm -rf /opt/codex-home"));
    assert!(script.contains("remove_pnpm_agents_for_bin_except gemini"));
    assert!(script.contains("rm -rf /opt/opencode-home/.opencode"));
    assert!(script.contains("rm -f /opt/opencode-home/.opencode.transaction"));
    assert!(script.contains("rm -rf /opt/claude-home"));
    assert!(!script.contains("OPENCODE_URL="));
}

#[test]
fn enabled_opencode_requires_a_resolved_release() {
    let error = build_install_script(
        DEFAULT_PI_SPEC,
        1_440,
        &[Agent::Opencode],
        &default_providers(),
        None,
    )
    .unwrap_err();
    assert!(error.contains("no resolved release"));
}

#[test]
fn generated_reconciliation_scripts_have_valid_bash_syntax() {
    for script in [
        all_agents_script(),
        build_install_script(DEFAULT_PI_SPEC, 1_440, &[], &default_providers(), None).unwrap(),
    ] {
        let status = Command::new("bash")
            .args(["-n", "-c", &script])
            .status()
            .unwrap();
        assert!(status.success());
    }
}

#[test]
fn legacy_pi_spec_resolves_to_current_default() {
    assert_eq!(resolve_pi_spec(LEGACY_PI_SPECS[0]), DEFAULT_PI_SPEC);
    assert_eq!(resolve_pi_spec("@custom/pi"), "@custom/pi");
}

#[test]
fn verification_command_keeps_and_quotes_the_active_config() {
    assert_eq!(
        verification_command(Agent::Pi, Path::new("/tmp/owner's config.toml")),
        "ags --agent pi --config '/tmp/owner'\\''s config.toml' -- --version"
    );
}
