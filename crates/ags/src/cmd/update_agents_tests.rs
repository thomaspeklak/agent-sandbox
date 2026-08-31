use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

use crate::cli::Agent;
use crate::config::{
    ArchiveMemberMatch, DEFAULT_PI_SPEC, LEGACY_PI_SPECS, ToolArchiveFormat, ToolDownloadArtifact,
    ToolDownloadSource,
};

use super::{build_install_script, build_podman_run_args, resolve_pi_spec, verification_command};

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
    assert!(script.contains("install_pnpm_agent pi '@earendil-works/pi-coding-agent'"));
    assert!(script.contains("https://chatgpt.com/codex/install.sh"));
    assert!(script.contains("install_pnpm_agent gemini @google/gemini-cli"));
    assert!(script.contains("exec /opt/claude-home/.local/bin/claude \"$@\""));
    assert!(!script.contains("pnpm self-update"));
    assert!(!script.contains("install_pnpm_agent opencode"));
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
fn deselected_agents_are_removed_without_resolving_opencode() {
    let script = build_install_script(DEFAULT_PI_SPEC, 1_440, &[Agent::Pi], None).unwrap();

    assert!(script.contains("install_pnpm_agent pi '@earendil-works/pi-coding-agent'"));
    assert!(script.contains("rm -rf /opt/codex-home"));
    assert!(script.contains("remove_pnpm_agent @google/gemini-cli gemini"));
    assert!(script.contains("rm -rf /opt/opencode-home/.opencode"));
    assert!(script.contains("rm -f /opt/opencode-home/.opencode.transaction"));
    assert!(script.contains("rm -rf /opt/claude-home"));
    assert!(!script.contains("OPENCODE_URL="));
}

#[test]
fn enabled_opencode_requires_a_resolved_release() {
    let error = build_install_script(DEFAULT_PI_SPEC, 1_440, &[Agent::Opencode], None).unwrap_err();
    assert!(error.contains("no resolved release"));
}

#[test]
fn generated_reconciliation_scripts_have_valid_bash_syntax() {
    for script in [
        all_agents_script(),
        build_install_script(DEFAULT_PI_SPEC, 1_440, &[], None).unwrap(),
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
