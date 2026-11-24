use std::collections::BTreeMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Output};

use sha2::{Digest, Sha256};

use super::{RECOVERY_SCRIPT, build_action};
use crate::config::{
    ArchiveMemberMatch, ToolArchiveFormat, ToolDownloadArtifact, ToolDownloadSource,
};

struct Installer {
    root: tempfile::TempDir,
    download: ToolDownloadSource,
}

impl Installer {
    fn new(previous_install: bool) -> Self {
        let root = tempfile::Builder::new()
            .prefix("ags opencode ")
            .tempdir()
            .unwrap();
        let source = root.path().join("source");
        fs::create_dir(&source).unwrap();
        fs::write(
            source.join("opencode"),
            r#"#!/usr/bin/env bash
set -e
[ "$#" -eq 1 ] && [ "$1" = --version ]
case "$0" in
  */.opencode.stage/bin/opencode) version="$STAGED_VERSION"; status="$STAGED_STATUS" ;;
  */.opencode/bin/opencode) version="$ACTIVATED_VERSION"; status="$ACTIVATED_STATUS" ;;
  *) exit 99 ;;
esac
printf '%s\n' "$version"
if [ "$status" -ne 0 ]; then printf 'fixture execution failed\n' >&2; fi
exit "$status"
"#,
        )
        .unwrap();
        fs::set_permissions(source.join("opencode"), fs::Permissions::from_mode(0o755)).unwrap();
        let archive = root.path().join("source.tar.gz");
        let output = Command::new("tar")
            .arg("-czf")
            .arg(&archive)
            .args(["--", "opencode"])
            .current_dir(&source)
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        let sha256 = format!("{:x}", Sha256::digest(fs::read(archive).unwrap()));
        let artifact = ToolDownloadArtifact {
            url: "https://example.test/opencode.tar.gz".to_owned(),
            sha256,
        };
        let download = ToolDownloadSource {
            version: "2.4.0".to_owned(),
            archive: ToolArchiveFormat::TarGz,
            member: "opencode".to_owned(),
            member_match: ArchiveMemberMatch::Exact,
            install_as: "opencode".to_owned(),
            artifacts: BTreeMap::from([
                ("x86_64".to_owned(), artifact.clone()),
                ("aarch64".to_owned(), artifact),
            ]),
        };
        if previous_install {
            fs::create_dir_all(root.path().join(".opencode/bin")).unwrap();
            fs::write(root.path().join(".opencode/bin/opencode"), "old-runtime").unwrap();
        }
        Self { root, download }
    }

    fn run(&self, staged: &str, activated: &str, stage_status: i32, active_status: i32) -> Output {
        // Exercise the actual generated installer, checksum, extraction, and recovery
        // using a local archive and isolated paths instead of network or Podman.
        let script = format!(
            r#"set -e
{RECOVERY_SCRIPT}
curl() {{ command cp -- "$TEST_ROOT/source.tar.gz" "$OPENCODE_ARCHIVE"; }}
remove_pnpm_agent() {{ : > "$TEST_ROOT/legacy-cleaned"; }}
{}
: > "$TEST_ROOT/completed"
"#,
            build_action(Some(&self.download)).unwrap()
        )
        .replace("/opt/opencode-home", "\"$TEST_ROOT\"")
        .replace("/tmp/ags-opencode.tar.gz", "\"$TEST_ROOT/archive.tar.gz\"");
        Command::new("bash")
            .args(["-c", &script])
            .env("TEST_ROOT", self.root.path())
            .env("STAGED_VERSION", staged)
            .env("ACTIVATED_VERSION", activated)
            .env("STAGED_STATUS", stage_status.to_string())
            .env("ACTIVATED_STATUS", active_status.to_string())
            .output()
            .unwrap()
    }

    fn assert_recovered(&self, previous_install: bool) {
        if previous_install {
            assert_eq!(
                fs::read_to_string(self.root.path().join(".opencode/bin/opencode")).unwrap(),
                "old-runtime"
            );
        } else {
            assert!(!self.root.path().join(".opencode").exists());
        }
        self.assert_temporary_files_removed();
        assert!(!self.root.path().join("legacy-cleaned").exists());
        assert!(!self.root.path().join("completed").exists());
    }

    fn assert_temporary_files_removed(&self) {
        for entry in [
            ".opencode.stage",
            ".opencode.previous",
            ".opencode.transaction",
            "archive.tar.gz",
        ] {
            assert!(!self.root.path().join(entry).exists(), "{entry}");
        }
    }
}

#[test]
fn installer_accepts_legacy_and_v2_versions_before_and_after_activation() {
    for version in ["2.4.0", "v2.4.0", "opencode 2.4.0", "opencode v2.4.0"] {
        let installer = Installer::new(true);
        let output = installer.run(version, version, 0, 0);
        assert!(output.status.success(), "{version}: {output:?}");
        let installed = installer.root.path().join(".opencode/bin/opencode");
        assert_eq!(
            fs::read(installed).unwrap(),
            fs::read(installer.root.path().join("source/opencode")).unwrap()
        );
        assert!(installer.root.path().join("legacy-cleaned").exists());
        assert!(installer.root.path().join("completed").exists());
        installer.assert_temporary_files_removed();
    }
}

#[test]
fn staged_version_mismatch_reports_output_and_preserves_previous_install() {
    for version in [
        "",
        "2.4.1",
        "opencode v2.4.1",
        "opencode v2.4.00",
        "other v2.4.0",
        "error 2.4.0",
        "opencode v2.4.0 extra",
        "notice\nopencode v2.4.0",
    ] {
        let installer = Installer::new(true);
        let output = installer.run(version, "opencode v2.4.0", 0, 0);
        assert_eq!(output.status.code(), Some(1), "{version}: {output:?}");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains("OpenCode staged version mismatch: expected 2.4.0, got "),
            "{stderr}"
        );
        installer.assert_recovered(true);
    }
}

#[test]
fn staged_execution_failure_keeps_status_and_stderr_despite_matching_version() {
    let installer = Installer::new(true);
    let output = installer.run("opencode v2.4.0", "opencode v2.4.0", 23, 0);
    assert_eq!(output.status.code(), Some(23), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("fixture execution failed"), "{stderr}");
    assert!(
        stderr.contains("OpenCode staged version check failed:"),
        "{stderr}"
    );
    assert!(
        stderr.contains("exited with status 23 (expected 2.4.0, stdout "),
        "{stderr}"
    );
    assert!(stderr.contains("opencode\\ v2.4.0"), "{stderr}");
    installer.assert_recovered(true);
}

#[test]
fn activated_version_mismatch_reports_actual_output_and_restores_previous_install() {
    let installer = Installer::new(true);
    let output = installer.run("opencode v2.4.0", "v2.4.1", 0, 0);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("OpenCode activated version mismatch: expected 2.4.0, got v2.4.1"),
        "{stderr}"
    );
    installer.assert_recovered(true);
}

#[test]
fn activated_execution_failure_restores_previous_install_and_preserves_exit_status() {
    let installer = Installer::new(true);
    let output = installer.run("opencode v2.4.0", "opencode v2.4.0", 0, 42);
    assert_eq!(output.status.code(), Some(42), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("fixture execution failed"), "{stderr}");
    assert!(
        stderr.contains("OpenCode activated version check failed:"),
        "{stderr}"
    );
    assert!(
        stderr.contains("exited with status 42 (expected 2.4.0, stdout "),
        "{stderr}"
    );
    installer.assert_recovered(true);
}

#[test]
fn failed_first_install_removes_rejected_candidate_before_or_after_activation() {
    for (staged, activated) in [("2.4.1", "2.4.0"), ("2.4.0", "2.4.1")] {
        let installer = Installer::new(false);
        let output = installer.run(staged, activated, 0, 0);
        assert!(!output.status.success(), "{output:?}");
        installer.assert_recovered(false);
    }
}
