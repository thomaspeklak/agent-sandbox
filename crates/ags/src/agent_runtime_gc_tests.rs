use super::*;

fn publish(cache: &Path) -> Update {
    let update = Update::begin(cache).unwrap();
    update.publish().unwrap();
    update
}

#[test]
fn keeps_latest_previous_and_container_references_only() {
    let temp = tempfile::tempdir().unwrap();
    let cache = temp.path();
    let mut paths = Vec::new();
    for _ in 0..4 {
        let update = publish(cache);
        paths.push(update.path.clone());
    }
    let latest = publish(cache);
    let references = BTreeSet::from([paths[1].clone(), paths[2].clone()]);
    let report = latest.cleanup_unlocked(&references).unwrap();
    assert_eq!(report.removed, vec![paths[0].clone()]);
    for path in paths.iter().skip(1).chain([&latest.path]) {
        assert!(path.is_dir(), "{}", path.display());
    }
    let report = latest.cleanup_unlocked(&BTreeSet::new()).unwrap();
    assert_eq!(report.removed.len(), 2);
    assert!(paths[3].exists());
    assert!(latest.path.exists());
}

#[test]
fn pending_launch_survives_two_updates_and_is_cleaned_after_last_lease_drops() {
    let temp = tempfile::tempdir().unwrap();
    let first = publish(temp.path());
    let first_path = first.path.clone();
    let lease = pin(temp.path()).unwrap();
    let clone = lease.clone();
    drop(first);
    drop(publish(temp.path()));
    let latest = publish(temp.path());
    assert!(
        latest
            .cleanup_unlocked(&BTreeSet::new())
            .unwrap()
            .removed
            .is_empty()
    );
    drop(lease);
    assert!(
        latest
            .cleanup_unlocked(&BTreeSet::new())
            .unwrap()
            .removed
            .is_empty()
    );
    drop(clone);
    assert_eq!(
        latest.cleanup_unlocked(&BTreeSet::new()).unwrap().removed,
        vec![first_path]
    );
}

#[test]
fn incomplete_legacy_unrelated_and_symlinked_trees_are_not_deleted() {
    let temp = tempfile::tempdir().unwrap();
    let incomplete = Update::begin(temp.path()).unwrap();
    let incomplete_path = incomplete.path.clone();
    let root = incomplete.root.clone();
    drop(incomplete);
    fs::create_dir(temp.path().join("pnpm-home")).unwrap();
    fs::create_dir(root.join("other-data")).unwrap();
    let external = tempfile::tempdir().unwrap();
    fs::write(external.path().join("valuable"), "keep").unwrap();
    std::os::unix::fs::symlink(external.path(), root.join("generation-symlink")).unwrap();
    drop(publish(temp.path()));
    let latest = publish(temp.path());
    assert!(
        latest
            .cleanup_unlocked(&BTreeSet::new())
            .unwrap()
            .removed
            .is_empty()
    );
    assert!(incomplete_path.exists());
    assert!(temp.path().join("pnpm-home").exists());
    assert!(root.join("other-data").exists());
    assert!(external.path().join("valuable").exists());
}

#[test]
fn invalid_previous_fails_before_any_deletion() {
    let temp = tempfile::tempdir().unwrap();
    let old = publish(temp.path());
    let old_path = old.path.clone();
    drop(old);
    drop(publish(temp.path()));
    let latest = publish(temp.path());
    fs::write(latest.root.join("previous"), "../elsewhere").unwrap();
    assert!(latest.cleanup_unlocked(&BTreeSet::new()).is_err());
    assert!(old_path.exists());
}

#[test]
fn cleanup_gate_blocks_selection_until_deletion_is_complete() {
    let temp = tempfile::tempdir().unwrap();
    drop(publish(temp.path()));
    drop(publish(temp.path()));
    let latest = publish(temp.path());
    let gate = Lock::open(&latest.root.join("cleanup.lock")).unwrap();
    gate.0.lock().unwrap();
    let barrier = std::sync::Barrier::new(2);
    std::thread::scope(|scope| {
        let (sender, receiver) = std::sync::mpsc::channel();
        // Scoped child cannot finish pinning while cleanup owns the selection gate.
        let barrier_ref = &barrier;
        let cache = temp.path();
        scope.spawn(move || {
            barrier_ref.wait();
            sender.send(pin(cache).unwrap()).unwrap();
        });
        barrier.wait();
        assert!(receiver.try_recv().is_err());
        latest.cleanup_unlocked(&BTreeSet::new()).unwrap();
        drop(gate);
        let lease = receiver.recv().unwrap();
        assert_eq!(lease.path, latest.path);
    });
}

#[test]
fn publication_tracks_previous_even_across_failed_installations() {
    let temp = tempfile::tempdir().unwrap();
    let first = publish(temp.path());
    let first_path = first.path.clone();
    drop(first);
    drop(Update::begin(temp.path()).unwrap()); // failed install, never published
    let second = publish(temp.path());
    assert_eq!(
        read_selection(&second.root, "previous").unwrap(),
        Some(first_path)
    );
    second.publish().unwrap(); // idempotent publication must not lose previous
    assert_ne!(
        read_selection(&second.root, "previous").unwrap(),
        Some(second.path.clone())
    );
}
