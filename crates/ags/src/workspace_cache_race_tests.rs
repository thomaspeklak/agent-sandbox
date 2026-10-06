use super::*;
use crate::workspace_cache::prepare;
use std::os::unix::fs::symlink;
use std::process::Command;

#[test]
fn a_checkout_restored_during_inventory_is_revalidated_before_deletion() {
    let cache = tempfile::tempdir().unwrap();
    let checkout = tempfile::tempdir().unwrap();
    let prepared = prepare(cache.path(), checkout.path()).unwrap();
    let path = prepared.store.parent().unwrap().to_owned();
    drop(prepared);
    let renamed = checkout.path().with_extension("moved");
    fs::rename(checkout.path(), &renamed).unwrap();
    let options = PruneOptions {
        grace: Duration::ZERO,
        ..Default::default()
    };
    let report = prune_with(cache.path(), &options, || {
        fs::rename(&renamed, checkout.path()).unwrap();
        Ok(vec![])
    })
    .unwrap();
    assert!(report.eligible.is_empty());
    assert!(path.exists());
}

#[test]
fn grace_reset_during_inventory_is_revalidated() {
    let cache = tempfile::tempdir().unwrap();
    let checkout = tempfile::tempdir().unwrap();
    let prepared = prepare(cache.path(), checkout.path()).unwrap();
    let path = prepared.store.parent().unwrap().to_owned();
    drop(prepared);
    drop(checkout);
    let marker = path.join(".orphaned");
    fs::write(&marker, b"").unwrap();
    fs::File::open(&marker)
        .unwrap()
        .set_times(
            fs::FileTimes::new().set_modified(SystemTime::now() - Duration::from_secs(8 * 86400)),
        )
        .unwrap();
    let report = prune_with(cache.path(), &Default::default(), || {
        fs::remove_file(&marker).unwrap();
        Ok(vec![])
    })
    .unwrap();
    assert!(report.eligible.is_empty());
    assert!(path.exists());
}

#[test]
fn symlinked_git_metadata_is_not_mistaken_for_a_removed_checkout() {
    let cache = tempfile::tempdir().unwrap();
    let checkout = tempfile::tempdir().unwrap();
    assert!(
        Command::new("git")
            .args(["init", "-q"])
            .current_dir(checkout.path())
            .status()
            .unwrap()
            .success()
    );
    let git = checkout.path().join(".git");
    let metadata = checkout.path().join("metadata");
    fs::rename(&git, &metadata).unwrap();
    symlink(&metadata, &git).unwrap();
    let prepared = prepare(cache.path(), checkout.path()).unwrap();
    drop(prepared);
    assert_eq!(
        prune_with(
            cache.path(),
            &PruneOptions {
                grace: Duration::ZERO,
                ..Default::default()
            },
            || panic!("live checkout")
        )
        .unwrap()
        .orphans,
        0
    );
}
