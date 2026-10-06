use super::*;
use crate::workspace_cache::prepare;
use std::cell::Cell;
use std::os::unix::fs::symlink;
use std::process::Command;

fn immediate() -> PruneOptions {
    PruneOptions {
        grace: Duration::ZERO,
        ..Default::default()
    }
}

fn init(path: &Path) {
    assert!(
        Command::new("git")
            .args(["init", "-q"])
            .current_dir(path)
            .status()
            .unwrap()
            .success()
    );
}

fn gone(cache: &Path) -> PathBuf {
    let checkout = tempfile::tempdir().unwrap();
    let prepared = prepare(cache, checkout.path()).unwrap();
    prepared.store.parent().unwrap().to_owned()
}

#[test]
fn missing_empty_and_live_caches_never_invoke_podman() {
    let cache = tempfile::tempdir().unwrap();
    let no_inspect = || -> io::Result<Vec<PathBuf>> { panic!("unnecessary Podman inventory") };
    assert!(
        prune_with(cache.path(), &immediate(), no_inspect)
            .unwrap()
            .removed
            .is_empty()
    );
    let checkout = tempfile::tempdir().unwrap();
    init(checkout.path());
    let prepared = prepare(cache.path(), checkout.path()).unwrap();
    drop(prepared);
    assert_eq!(
        prune_with(cache.path(), &immediate(), no_inspect)
            .unwrap()
            .orphans,
        0
    );
}

#[test]
fn grace_starts_at_first_observation_and_dry_run_never_marks_or_deletes() {
    let cache = tempfile::tempdir().unwrap();
    let path = gone(cache.path());
    let dry = PruneOptions {
        dry_run: true,
        ..Default::default()
    };
    let no_inspect = || -> io::Result<Vec<PathBuf>> { panic!("grace not elapsed") };
    let report = prune_with(cache.path(), &dry, no_inspect).unwrap();
    assert_eq!(report.orphans, 1);
    assert!(!path.join(".orphaned").exists());
    prune_with(cache.path(), &Default::default(), no_inspect).unwrap();
    let marker = path.join(".orphaned");
    assert!(marker.exists());
    prune_with(cache.path(), &Default::default(), no_inspect).unwrap();
    fs::File::open(marker)
        .unwrap()
        .set_times(
            fs::FileTimes::new().set_modified(SystemTime::now() - Duration::from_secs(8 * 86400)),
        )
        .unwrap();
    let report = prune_with(
        cache.path(),
        &PruneOptions {
            dry_run: true,
            ..Default::default()
        },
        || Ok(vec![]),
    )
    .unwrap();
    assert_eq!(report.eligible, vec![path.clone()]);
    assert!(path.join("pnpm-store").exists());
    assert_eq!(
        prune_with(cache.path(), &Default::default(), || Ok(vec![]))
            .unwrap()
            .removed
            .len(),
        1
    );
}

#[test]
fn pending_launch_and_cloned_plan_owners_pin_cache() {
    let cache = tempfile::tempdir().unwrap();
    let checkout = tempfile::tempdir().unwrap();
    let prepared = prepare(cache.path(), checkout.path()).unwrap();
    let path = prepared.store.parent().unwrap().to_owned();
    let clone = prepared.lease.clone();
    drop(prepared);
    drop(checkout);
    assert!(
        prune_with(cache.path(), &immediate(), || Ok(vec![]))
            .unwrap()
            .eligible
            .is_empty()
    );
    assert!(path.exists());
    drop(clone);
    assert_eq!(
        prune_with(cache.path(), &immediate(), || Ok(vec![]))
            .unwrap()
            .removed
            .len(),
        1
    );
}

#[test]
fn mount_inventory_retains_children_ancestors_and_aliases() {
    let cache = tempfile::tempdir().unwrap();
    let path = gone(cache.path());
    let alias = cache.path().join("alias");
    symlink(&path, &alias).unwrap();
    for source in [
        path.join("pnpm-store"),
        path.clone(),
        cache.path().to_owned(),
        alias,
    ] {
        let source = usage::canonical(&source);
        assert!(
            prune_with(cache.path(), &immediate(), || Ok(vec![source]))
                .unwrap()
                .eligible
                .is_empty()
        );
    }
    assert!(path.exists());
}

#[test]
fn inspection_failure_is_fail_closed() {
    let cache = tempfile::tempdir().unwrap();
    let path = gone(cache.path());
    assert!(
        prune_with(cache.path(), &immediate(), || Err(io::Error::other(
            "Podman unavailable"
        )))
        .is_err()
    );
    assert!(path.join("identity.json").exists());
    assert!(
        !path
            .with_file_name(format!(
                ".gc-{}",
                path.file_name().unwrap().to_string_lossy()
            ))
            .exists()
    );
}

#[test]
fn recreated_checkout_at_same_path_collects_only_old_identity() {
    let cache = tempfile::tempdir().unwrap();
    let checkout = tempfile::tempdir().unwrap();
    init(checkout.path());
    let old = prepare(cache.path(), checkout.path()).unwrap();
    let old_path = old.store.parent().unwrap().to_owned();
    drop(old);
    fs::remove_dir_all(checkout.path().join(".git")).unwrap();
    init(checkout.path());
    let new = prepare(cache.path(), checkout.path()).unwrap();
    let report = prune_with(cache.path(), &immediate(), || Ok(vec![])).unwrap();
    assert_eq!(report.removed.len(), 1);
    assert!(!old_path.exists());
    assert!(new.store.exists());
}

#[test]
fn legacy_identity_and_non_git_checkout_supported() {
    let cache = tempfile::tempdir().unwrap();
    let checkout = tempfile::tempdir().unwrap();
    init(checkout.path());
    let prepared = prepare(cache.path(), checkout.path()).unwrap();
    let path = prepared.store.parent().unwrap().to_owned();
    drop(prepared);
    let mut identity: serde_json::Value =
        serde_json::from_slice(&fs::read(path.join("identity.json")).unwrap()).unwrap();
    identity.as_object_mut().unwrap().remove("git_anchor");
    fs::write(
        path.join("identity.json"),
        serde_json::to_vec(&identity).unwrap(),
    )
    .unwrap();
    assert_eq!(
        prune_with(cache.path(), &immediate(), || panic!("live checkout"))
            .unwrap()
            .orphans,
        0
    );
    fs::remove_dir_all(checkout.path().join(".git")).unwrap();
    assert_eq!(
        prune_with(cache.path(), &immediate(), || Ok(vec![]))
            .unwrap()
            .removed
            .len(),
        1
    );
    let non_git = prepare(cache.path(), checkout.path()).unwrap();
    assert_eq!(
        prune_with(cache.path(), &immediate(), || panic!("live directory"))
            .unwrap()
            .orphans,
        0
    );
    assert!(non_git.store.exists());
}

#[test]
fn linked_worktrees_are_live_without_spawning_git_during_prune() {
    let cache = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    init(repo.path());
    assert!(
        Command::new("git")
            .args([
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.com",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "--allow-empty",
                "-qm",
                "init"
            ])
            .current_dir(repo.path())
            .status()
            .unwrap()
            .success()
    );
    let linked_parent = tempfile::tempdir().unwrap();
    let linked = linked_parent.path().join("linked");
    assert!(
        Command::new("git")
            .args(["worktree", "add", "-q", "--detach"])
            .arg(&linked)
            .current_dir(repo.path())
            .status()
            .unwrap()
            .success()
    );
    let prepared = prepare(cache.path(), &linked).unwrap();
    drop(prepared);
    assert_eq!(
        prune_with(cache.path(), &immediate(), || panic!("live worktree"))
            .unwrap()
            .orphans,
        0
    );
    fs::remove_dir_all(linked).unwrap();
    assert_eq!(
        prune_with(cache.path(), &immediate(), || Ok(vec![]))
            .unwrap()
            .removed
            .len(),
        1
    );
}

#[test]
fn deletion_is_bounded_resumable_and_does_not_follow_symlinks() {
    let cache = tempfile::tempdir().unwrap();
    let path = gone(cache.path());
    let external = tempfile::tempdir().unwrap();
    fs::write(external.path().join("precious"), b"keep").unwrap();
    symlink(external.path(), path.join("pnpm-store/alias")).unwrap();
    for n in 0..20 {
        fs::write(path.join(format!("pnpm-store/{n}")), b"data").unwrap();
    }
    let options = PruneOptions {
        max_deletions: 5,
        ..immediate()
    };
    let first = prune_with(cache.path(), &options, || Ok(vec![])).unwrap();
    assert!(!path.exists());
    assert_eq!(first.pending.len(), 1);
    assert!(first.pending[0].join("identity.json").exists());
    assert!(first.deletions <= 5);
    for _ in 0..10 {
        let report = prune_with(cache.path(), &options, || Ok(vec![])).unwrap();
        assert!(report.deletions <= 5);
        if !first.pending[0].exists() {
            break;
        }
    }
    assert!(!first.pending[0].exists());
    assert_eq!(fs::read(external.path().join("precious")).unwrap(), b"keep");
}

#[test]
fn cache_limit_and_single_inventory_are_enforced() {
    let cache = tempfile::tempdir().unwrap();
    let paths: Vec<_> = (0..5).map(|_| gone(cache.path())).collect();
    let calls = Cell::new(0);
    let report = prune_with(cache.path(), &immediate(), || {
        calls.set(calls.get() + 1);
        Ok(vec![])
    })
    .unwrap();
    assert_eq!(calls.get(), 1);
    assert_eq!(report.removed.len(), 2);
    assert_eq!(paths.iter().filter(|p| p.exists()).count(), 3);
}

#[test]
fn busy_pruners_and_launch_gate_skip_without_inventory() {
    let cache = tempfile::tempdir().unwrap();
    gone(cache.path());
    for filename in ["prune.lock", "cleanup.lock"] {
        let lock = Lock::open(&cache.path().join(ROOT).join(filename)).unwrap();
        lock.0.lock().unwrap();
        assert!(
            prune_with(cache.path(), &immediate(), || panic!("busy"))
                .unwrap()
                .busy
        );
    }
}

#[test]
fn malformed_unrelated_and_symlinked_entries_are_untouched() {
    let cache = tempfile::tempdir().unwrap();
    let path = gone(cache.path());
    fs::write(path.join("identity.json"), b"broken").unwrap();
    let unrelated = cache.path().join(ROOT).join("unrelated");
    fs::create_dir(&unrelated).unwrap();
    let alias = cache.path().join(ROOT).join("a".repeat(64));
    symlink(&path, &alias).unwrap();
    assert!(
        prune_with(cache.path(), &immediate(), || panic!("no valid candidates"))
            .unwrap()
            .eligible
            .is_empty()
    );
    assert!(path.exists());
    assert!(alias.exists());
    assert!(unrelated.exists());
}

#[test]
fn restored_checkout_clears_orphan_marker_and_empty_crash_trash_is_resumed() {
    let cache = tempfile::tempdir().unwrap();
    let checkout = tempfile::tempdir().unwrap();
    let prepared = prepare(cache.path(), checkout.path()).unwrap();
    let path = prepared.store.parent().unwrap().to_owned();
    fs::write(path.join(".orphaned"), b"").unwrap();
    drop(prepared);
    prepare(cache.path(), checkout.path()).unwrap();
    assert!(!path.join(".orphaned").exists());
    let trash = cache
        .path()
        .join(ROOT)
        .join(format!(".gc-{}", "b".repeat(64)));
    fs::create_dir(&trash).unwrap();
    assert_eq!(
        prune_with(cache.path(), &immediate(), || Ok(vec![]))
            .unwrap()
            .removed,
        vec![trash]
    );
}
