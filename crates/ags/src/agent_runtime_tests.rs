use super::*;

#[test]
fn publication_pins_old_selection_and_keeps_legacy_untouched() {
    let temp = tempfile::tempdir().unwrap();
    let cache = temp.path();
    fs::create_dir(cache.join("pnpm-home")).unwrap();
    fs::write(cache.join("pnpm-home/legacy"), "keep").unwrap();
    assert_eq!(selected(cache).unwrap(), cache);
    let first = Update::begin(cache).unwrap();
    fs::write(first.path.join("pnpm-home/module.js"), "old").unwrap();
    assert_eq!(selected(cache).unwrap(), cache);
    first.publish().unwrap();
    let pinned = selected(cache).unwrap();
    drop(first);
    let second = Update::begin(cache).unwrap();
    fs::write(second.path.join("pnpm-home/module.js"), "new").unwrap();
    assert_eq!(selected(cache).unwrap(), pinned);
    second.publish().unwrap();
    assert_eq!(selected(cache).unwrap(), second.path);
    assert_eq!(
        fs::read_to_string(pinned.join("pnpm-home/module.js")).unwrap(),
        "old"
    );
    assert_eq!(
        fs::read_to_string(cache.join("pnpm-home/legacy")).unwrap(),
        "keep"
    );
    assert_eq!(
        mount_source(cache, &pinned, "pnpm-home"),
        pinned.join("pnpm-home")
    );
    assert_eq!(
        mount_source(cache, &pinned, "npm-global"),
        cache.join("npm-global")
    );
}

#[test]
fn failed_update_does_not_publish_and_lock_is_released_on_drop() {
    let temp = tempfile::tempdir().unwrap();
    let first = Update::begin(temp.path()).unwrap();
    first.publish().unwrap();
    let original = first.path.clone();
    assert!(Update::begin(temp.path()).is_err());
    drop(first);
    let failed = Update::begin(temp.path()).unwrap();
    let failed_path = failed.path.clone();
    drop(failed);
    assert_eq!(selected(temp.path()).unwrap(), original);
    assert!(failed_path.exists());
    assert!(Update::begin(temp.path()).is_ok());
}

#[test]
fn malformed_missing_and_escaping_selections_fail_closed() {
    let temp = tempfile::tempdir().unwrap();
    let update = Update::begin(temp.path()).unwrap();
    for name in [
        "",
        "../outside",
        "generation-missing",
        "/tmp",
        "generation-a/../b",
    ] {
        fs::write(update.root.join("current"), name).unwrap();
        assert!(selected(temp.path()).is_err(), "{name}");
    }
    let outside = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(outside.path(), update.root.join("generation-escape")).unwrap();
    fs::write(update.root.join("current"), "generation-escape").unwrap();
    assert!(selected(temp.path()).is_err());
    fs::remove_file(update.root.join("current")).unwrap();
    std::os::unix::fs::symlink("missing", update.root.join("current")).unwrap();
    assert!(selected(temp.path()).is_err());
    fs::remove_file(update.root.join("current")).unwrap();
    update.publish().unwrap();
    fs::remove_dir(update.path.join("claude-install")).unwrap();
    assert!(selected(temp.path()).is_err());
}

#[test]
fn usage_tracks_running_stopped_and_legacy_mount_sources() {
    let temp = tempfile::tempdir().unwrap();
    let update = Update::begin(temp.path()).unwrap();
    for status in ["running", "exited", "created", "paused"] {
        let json = serde_json::json!({
            "Name": "sandbox", "State": {"Status": status},
            "Mounts": [
                {"Source": update.path.join("pnpm-home")},
                {"Source": update.path.join("claude-install")},
                {"Source": temp.path().join("pnpm-home")},
                {"Source": "/unrelated/agent-runtimes/generation-other/pnpm-home"},
                {"Source": temp.path().join("cargo-home")},
                {"Type": "tmpfs", "Destination": "/tmp"}
            ]
        });
        let container: ContainerUse = serde_json::from_value(json).unwrap();
        assert_eq!(
            referenced_roots(temp.path(), &container),
            BTreeSet::from([temp.path().to_owned(), update.path.clone()])
        );
    }
}

#[test]
fn delayed_read_in_running_process_survives_publication() {
    // Exercise a real delayed filesystem read, independent of Node availability.
    use std::io::{BufRead, BufReader};
    use std::process::Stdio;
    let temp = tempfile::tempdir().unwrap();
    let first = Update::begin(temp.path()).unwrap();
    fs::write(first.path.join("pnpm-home/module"), "old").unwrap();
    first.publish().unwrap();
    let pinned = selected(temp.path()).unwrap();
    let mut child = Command::new("sh")
        .args(["-c", "echo ready; read signal; cat \"$1/module\"", "sh"])
        .arg(pinned.join("pnpm-home"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut ready = String::new();
    stdout.read_line(&mut ready).unwrap();
    assert_eq!(ready, "ready\n");
    drop(first);
    let second = Update::begin(temp.path()).unwrap();
    fs::write(second.path.join("pnpm-home/module"), "new").unwrap();
    second.publish().unwrap();
    writeln!(child.stdin.take().unwrap(), "go").unwrap();
    let mut loaded = String::new();
    std::io::Read::read_to_string(&mut stdout, &mut loaded).unwrap();
    assert!(child.wait().unwrap().success());
    assert_eq!(loaded, "old");
    assert_eq!(
        fs::read_to_string(selected(temp.path()).unwrap().join("pnpm-home/module")).unwrap(),
        "new"
    );
}
