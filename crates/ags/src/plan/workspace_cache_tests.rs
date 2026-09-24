use super::*;
use std::process::Command;

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

#[test]
fn same_worktree_subdirectories_share_cache_and_different_worktrees_do_not() {
    let cache = tempfile::tempdir().unwrap();
    let first = tempfile::tempdir().unwrap();
    init(first.path());
    fs::create_dir(first.path().join("nested")).unwrap();
    let root_cache = prepare(cache.path(), first.path()).unwrap();
    let nested_cache = prepare(cache.path(), &first.path().join("nested")).unwrap();
    assert_eq!(root_cache.store, nested_cache.store);
    assert_eq!(root_cache.cache, nested_cache.cache);

    let second = tempfile::tempdir().unwrap();
    init(second.path());
    let second_cache = prepare(cache.path(), second.path()).unwrap();
    assert_ne!(root_cache.store, second_cache.store);
}

#[test]
fn reused_device_and_inode_still_get_a_fresh_identity_for_a_new_checkout() {
    let path = Path::new("/same/worktree");
    let first = identity_hash(path, 7, 42, Some("checkout-one"));
    let second = identity_hash(path, 7, 42, Some("checkout-two"));
    assert_ne!(first, second);
    assert_eq!(first, identity_hash(path, 7, 42, Some("checkout-one")));
}

#[test]
fn recreating_checkout_metadata_at_same_path_gets_a_fresh_cache() {
    let cache = tempfile::tempdir().unwrap();
    let worktree = tempfile::tempdir().unwrap();
    init(worktree.path());
    let first = prepare(cache.path(), worktree.path()).unwrap();
    fs::remove_dir_all(worktree.path().join(".git")).unwrap();
    init(worktree.path());
    let second = prepare(cache.path(), worktree.path()).unwrap();
    assert_ne!(first.store, second.store);
}
