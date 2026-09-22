//! End-to-end orchestration tests use a fake Podman; no daemon or downloads needed.
#[path = "support/generation_cleanup.rs"]
mod generation_cleanup;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Output};

fn setup(root: &Path) {
    fs::write(
        root.join("config.toml"),
        format!(
            r#"
[sandbox]
image = "localhost/agent-sandbox:test"
containerfile = "{0}/Containerfile"
cache_dir = "{0}/cache"
gitconfig_path = "{0}/gitconfig"
auth_key = "{0}/auth"
sign_key = "{0}/sign"
enabled_agents = ["pi"]
"#,
            root.display()
        ),
    )
    .unwrap();
    fs::write(
        root.join("podman"),
        r#"#!/bin/sh
set -eu
printf '%s\n' "$*" >> "$TEST_ROOT/calls"
case "$1" in
  ps)
    [ "${FAIL_AT:-}" != inspect ] || exit 3
    if [ "${FAIL_AT:-}" = cleanup ] && [ -f "$TEST_ROOT/verified" ]; then exit 8; fi
    printf 'running\nstopped\n' ;;
  container) cat "$TEST_ROOT/$3.json" ;;
  run)
    case "$*" in
      *:rw*) [ "${FAIL_AT:-}" != install ] || exit 4 ;;
      *:ro*)
        [ "${FAIL_AT:-}" != verify ] || exit 5
        touch "$TEST_ROOT/verified"
        for name in running stopped; do
          if [ -f "$TEST_ROOT/late-$name.json" ]; then cp "$TEST_ROOT/late-$name.json" "$TEST_ROOT/$name.json"; fi
        done ;;
      *) exit 6 ;;
    esac ;;
  *) exit 7 ;;
esac
"#,
    )
    .unwrap();
    fs::set_permissions(root.join("podman"), fs::Permissions::from_mode(0o755)).unwrap();
    fs::create_dir_all(root.join("cache/pnpm-home")).unwrap();
    fs::write(root.join("cache/pnpm-home/legacy"), "untouched").unwrap();
    for (name, status) in [("running", "running"), ("stopped", "exited")] {
        fs::write(
            root.join(format!("{name}.json")),
            serde_json::to_vec(&serde_json::json!([{
                "Name": name, "State": {"Status": status},
                "Mounts": [{"Source": root.join("cache/pnpm-home")}]
            }]))
            .unwrap(),
        )
        .unwrap();
    }
}

fn update(root: &Path, fail_at: &str) -> Output {
    let _ = fs::remove_file(root.join("verified"));
    Command::new(env!("CARGO_BIN_EXE_ags"))
        .current_dir(root)
        .env("PATH", format!("{}:/usr/bin:/bin", root.display()))
        .env("TEST_ROOT", root)
        .env("FAIL_AT", fail_at)
        .args(["update-agents", "--config"])
        .arg(root.join("config.toml"))
        .output()
        .unwrap()
}

#[test]
fn update_reports_all_container_references_and_publishes_only_verified_generation() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    setup(root);
    let output = update(root, "");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("running (running)"), "{stdout}");
    assert!(stdout.contains("stopped (exited)"), "{stdout}");
    let cache = root.join("cache");
    let first = ags::agent_runtime::selected(&cache).unwrap();
    assert_ne!(first, cache);
    fs::write(
        root.join("running.json"),
        serde_json::to_vec(&serde_json::json!([{
            "Name": "pinned", "State": {"Status": "running"},
            "Mounts": [{"Source": first.join("pnpm-home")}]
        }]))
        .unwrap(),
    )
    .unwrap();
    let output = update(root, "");
    assert!(output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stdout)
            .contains(&format!("retained: {} — pinned", first.display()))
    );
    assert_ne!(ags::agent_runtime::selected(&cache).unwrap(), first);
    assert!(first.is_dir());
    assert_eq!(
        fs::read_to_string(cache.join("pnpm-home/legacy")).unwrap(),
        "untouched"
    );
    let calls = fs::read_to_string(root.join("calls")).unwrap();
    assert!(calls.contains("ps --all --quiet --no-trunc"));
    assert!(calls.contains("container inspect stopped"));
    assert!(calls.contains(":/usr/local/pnpm:ro"));
    assert!(calls.contains(&format!(
        "timeout 60 {} --version",
        ags::util::shell_quote("/usr/local/pnpm/bin/pi")
    )));
    assert!(!calls.contains(&format!(
        "{}:/usr/local/pnpm:rw",
        cache.join("pnpm-home").display()
    )));
}

#[test]
fn failed_inspection_install_or_verification_preserves_selection() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    setup(root);
    assert!(update(root, "").status.success());
    let original = ags::agent_runtime::selected(&root.join("cache")).unwrap();
    for phase in ["inspect", "install", "verify"] {
        let output = update(root, phase);
        assert!(!output.status.success(), "{phase}");
        assert_eq!(
            ags::agent_runtime::selected(&root.join("cache")).unwrap(),
            original
        );
    }
}
