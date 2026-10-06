use super::*;

#[test]
fn a_second_holder_is_refused_until_the_first_releases() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("output.lock");

    let first = try_acquire(&path).unwrap().expect("first lock");
    assert!(try_acquire(&path).unwrap().is_none());
    drop(first);
    assert!(try_acquire(&path).unwrap().is_some());
}

#[test]
fn lock_file_is_private() {
    use std::os::unix::fs::PermissionsExt;

    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("output.lock");
    let _lock = acquire(&path, "localhost/agent-sandbox:latest").unwrap();
    let mode = std::fs::metadata(&path).unwrap().permissions().mode();
    assert_eq!(mode & 0o077, 0);
}

#[test]
fn waiting_acquire_blocks_until_release() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("output.lock");
    let held = try_acquire(&path).unwrap().unwrap();

    let waiter_path = path.clone();
    let waiter = std::thread::spawn(move || {
        acquire(&waiter_path, "image").map(|_| std::time::Instant::now())
    });
    std::thread::sleep(std::time::Duration::from_millis(100));
    let released = std::time::Instant::now();
    drop(held);
    let acquired = waiter.join().unwrap().unwrap();
    assert!(acquired >= released);
}
