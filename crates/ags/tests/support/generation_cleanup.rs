use super::*;
use ags::agent_runtime::selected;
use std::path::PathBuf;

fn successful_update(root: &Path) -> PathBuf {
    let output = update(root, "");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!String::from_utf8_lossy(&output.stderr).contains("cleanup did not complete"));
    selected(&root.join("cache")).unwrap()
}

fn reference(root: &Path, file: &str, path: &Path, status: &str) {
    fs::write(
        root.join(file),
        serde_json::to_vec(&serde_json::json!([{
            "Name": file, "State": {"Status": status},
            "Mounts": [{"Source": path.join("pnpm-home")}]
        }]))
        .unwrap(),
    )
    .unwrap();
}

#[test]
fn automatically_cleans_unreferenced_older_generations() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    setup(root);
    let first = successful_update(root);
    let second = successful_update(root);
    let third = successful_update(root);
    assert!(!first.exists());
    assert!(second.exists());
    assert!(third.exists());
    assert!(root.join("cache/pnpm-home/legacy").exists());
}

#[test]
fn refreshes_container_references_after_install_and_keeps_stopped_containers() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    setup(root);
    let first = successful_update(root);
    let second = successful_update(root);
    // These references appear only during verification, after the initial inspection.
    reference(root, "late-running.json", &first, "running");
    reference(root, "late-stopped.json", &second, "exited");
    successful_update(root);
    let fourth = successful_update(root);
    assert!(first.exists());
    assert!(second.exists());
    // Remove references and let the next update collect them, but retain previous.
    for name in ["running", "stopped"] {
        fs::remove_file(root.join(format!("late-{name}.json"))).unwrap();
        reference(
            root,
            &format!("{name}.json"),
            &root.join("cache"),
            "running",
        );
    }
    successful_update(root);
    assert!(!first.exists());
    assert!(!second.exists());
    assert!(fourth.exists());
}

#[test]
fn failed_cleanup_inspection_does_not_delete_any_generation_or_undo_update() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    setup(root);
    let first = successful_update(root);
    let second = successful_update(root);
    let output = update(root, "cleanup");
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("cleanup did not complete"));
    assert!(first.exists());
    assert!(second.exists());
    assert_ne!(selected(&root.join("cache")).unwrap(), second);
}

#[test]
fn pending_launch_survives_updates_until_its_lease_is_released() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    setup(root);
    let first = successful_update(root);
    let pending_launch = ags::agent_runtime::pin(&root.join("cache")).unwrap();
    successful_update(root);
    successful_update(root);
    assert!(first.exists());
    drop(pending_launch);
    successful_update(root);
    assert!(!first.exists());
}
