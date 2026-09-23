use super::*;
use ags::agent_runtime::{ROOT, selected};
use std::os::unix::fs::MetadataExt;

fn run(root: &Path, version: &str) -> Output {
    fs::write(root.join("pinned-version"), version).unwrap();
    let output = update(root, "");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output
}

#[test]
fn noop_keeps_current_previous_and_does_not_leave_a_candidate_behind() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    setup(root);
    run(root, "1");
    run(root, "2");
    let runtime_root = root.join("cache").join(ROOT);
    let current = fs::read(runtime_root.join("current")).unwrap();
    let previous = fs::read(runtime_root.join("previous")).unwrap();
    let before = fs::read_dir(&runtime_root).unwrap().count();
    let output = run(root, "2");
    assert!(String::from_utf8_lossy(&output.stdout).contains("Already up to date"));
    assert_eq!(fs::read(runtime_root.join("current")).unwrap(), current);
    assert_eq!(fs::read(runtime_root.join("previous")).unwrap(), previous);
    assert_eq!(fs::read_dir(&runtime_root).unwrap().count(), before);
}

#[test]
fn unchanged_update_reuses_updater_cache_without_exposing_it_to_verification() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    setup(root);
    run(root, "1");
    run(root, "1");
    assert_eq!(fs::read_to_string(root.join("downloads")).unwrap(), "1");
    assert!(
        root.join("cache/agent-downloads/pnpm-store/package-content")
            .exists()
    );
    let calls = fs::read_to_string(root.join("calls")).unwrap();
    assert!(calls.contains(":/var/cache/ags/agent-pnpm-store:rw"));
    assert!(calls.contains(":/var/cache/ags/agent-pnpm-cache:rw"));
    assert!(calls.contains("--network=none"));
}

#[test]
fn noop_still_cleans_unreferenced_legacy_runtime() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    setup(root);
    run(root, "1");
    assert!(root.join("cache/pnpm-home/legacy").exists());
    for name in ["running", "stopped"] {
        fs::write(
            root.join(format!("{name}.json")),
            format!(r#"[{{"Name":"{name}","State":{{"Status":"running"}},"Mounts":[]}}]"#),
        )
        .unwrap();
    }
    let output = run(root, "1");
    assert!(String::from_utf8_lossy(&output.stdout).contains("Already up to date"));
    assert!(!root.join("cache/pnpm-home").exists());
}

#[test]
fn changed_generations_share_only_identical_files_and_image_change_is_not_a_noop() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    setup(root);
    run(root, "1");
    let first = selected(&root.join("cache")).unwrap();
    run(root, "2");
    let second = selected(&root.join("cache")).unwrap();
    assert_ne!(first, second);
    assert_eq!(
        fs::metadata(first.join("pnpm-home/shared.js"))
            .unwrap()
            .ino(),
        fs::metadata(second.join("pnpm-home/shared.js"))
            .unwrap()
            .ino()
    );
    assert_ne!(
        fs::metadata(first.join("pnpm-home/bin/pi")).unwrap().ino(),
        fs::metadata(second.join("pnpm-home/bin/pi")).unwrap().ino()
    );
    assert_eq!(
        fs::read_to_string(first.join("pnpm-home/bin/pi")).unwrap(),
        "1"
    );
    fs::write(root.join("image-id"), format!("sha256:{}", "a".repeat(64))).unwrap();
    let output = run(root, "2");
    assert!(!String::from_utf8_lossy(&output.stdout).contains("Already up to date"));
    assert_ne!(selected(&root.join("cache")).unwrap(), second);
}
