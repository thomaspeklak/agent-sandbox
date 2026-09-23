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

fn clear_reference(root: &Path, name: &str) {
    fs::write(
        root.join(format!("{name}.json")),
        serde_json::to_vec(&serde_json::json!([{
            "Name": name, "State": {"Status": "running"}, "Mounts": []
        }]))
        .unwrap(),
    )
    .unwrap();
}

#[test]
fn legacy_cleanup_waits_for_last_container_and_leaves_user_caches_untouched() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    setup(root);
    let cache = root.join("cache");
    for suffix in [
        "codex-install",
        "claude-install",
        "opencode-install",
        "npm-global",
        "cargo-home",
        "ags-hooks",
    ] {
        fs::create_dir(cache.join(suffix)).unwrap();
        fs::write(cache.join(suffix).join("data"), "keep").unwrap();
    }
    successful_update(root);
    clear_reference(root, "running");
    successful_update(root); // The stopped old container must still protect legacy files.
    assert!(cache.join("pnpm-home/legacy").exists());
    assert!(cache.join("claude-install/data").exists());
    clear_reference(root, "stopped");
    let output = update(root, "cleanup");
    assert!(output.status.success());
    assert!(cache.join("pnpm-home/legacy").exists()); // Inspection failure: no deletion.
    successful_update(root);
    for suffix in ags::agent_runtime::RUNTIME_DIRS {
        assert!(!cache.join(suffix).exists());
    }
    for suffix in ["npm-global", "cargo-home", "ags-hooks"] {
        assert!(cache.join(suffix).join("data").exists());
    }
}

#[test]
fn pending_legacy_launch_is_protected_across_first_generation_update() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    setup(root);
    clear_reference(root, "running");
    clear_reference(root, "stopped");
    let cache = root.join("cache");
    let pending = ags::agent_runtime::pin(&cache).unwrap();
    assert_eq!(pending.path, cache);
    successful_update(root);
    assert!(cache.join("pnpm-home/legacy").exists());
    drop(pending);
    successful_update(root);
    assert!(!cache.join("pnpm-home").exists());
}

#[test]
fn late_legacy_container_reference_is_honored_by_cleanup() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    setup(root);
    clear_reference(root, "running");
    clear_reference(root, "stopped");
    reference(root, "late-stopped.json", &root.join("cache"), "exited");
    successful_update(root);
    assert!(root.join("cache/pnpm-home/legacy").exists());
}

#[test]
fn stale_incomplete_candidate_is_cleaned_but_fresh_one_is_retained() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    setup(root);
    let runtimes = root.join("cache/agent-runtimes");
    let stale = runtimes.join("generation-abandoned");
    let fresh = runtimes.join("generation-starting");
    for path in [&stale, &fresh] {
        fs::create_dir_all(path).unwrap();
        fs::write(path.join(".installing"), "").unwrap();
    }
    let old = std::time::SystemTime::now() - std::time::Duration::from_secs(11 * 60);
    fs::File::open(stale.join(".installing"))
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(old))
        .unwrap();
    let output = update(root, "");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!stale.exists());
    assert!(fresh.exists());
    assert!(
        String::from_utf8_lossy(&output.stdout).contains(&format!("cleaned: {}", stale.display()))
    );
}

#[test]
fn failed_update_discards_its_candidate_and_sweeps_older_stale_incomplete() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    setup(root);
    let selected = successful_update(root);
    let stale = root.join("cache/agent-runtimes/generation-old-failure");
    fs::create_dir_all(&stale).unwrap();
    fs::write(stale.join(".installing"), "").unwrap();
    let old = std::time::SystemTime::now() - std::time::Duration::from_secs(11 * 60);
    fs::File::open(stale.join(".installing"))
        .unwrap()
        .set_times(fs::FileTimes::new().set_modified(old))
        .unwrap();
    let output = update(root, "install");
    assert!(!output.status.success());
    assert!(!stale.exists());
    assert_eq!(
        ags::agent_runtime::selected(&root.join("cache")).unwrap(),
        selected
    );
    assert!(
        String::from_utf8_lossy(&output.stdout).contains(&format!("cleaned: {}", stale.display()))
    );
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
