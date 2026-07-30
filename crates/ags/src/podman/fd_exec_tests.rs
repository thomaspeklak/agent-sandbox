use super::*;
use std::os::fd::AsRawFd;
use std::os::unix::process::ExitStatusExt;

fn identity(fd: RawFd) -> Option<(libc::dev_t, libc::ino_t)> {
    let mut stat = unsafe { std::mem::zeroed::<libc::stat>() };
    (unsafe { libc::fstat(fd, &mut stat) } == 0).then_some((stat.st_dev, stat.st_ino))
}

#[test]
fn remaps_payload_and_closes_the_parent_handoff_copy() {
    let payload = tempfile::tempfile().unwrap();
    let handoff = payload.try_clone().unwrap();
    let handoff_number = handoff.as_raw_fd();
    let payload_identity = identity(handoff_number).unwrap();
    let program = CString::new("sh").unwrap();
    let args = [
        CString::new("sh").unwrap(),
        CString::new("-c").unwrap(),
        CString::new("test -r /proc/self/fd/3").unwrap(),
    ];

    let status = spawn_with_payload_fds(program.as_c_str(), &args, vec![handoff.into()])
        .unwrap()
        .wait()
        .unwrap();

    assert!(status.success());
    assert_ne!(
        identity(handoff_number),
        Some(payload_identity),
        "parent must close its handoff descriptor after spawn"
    );
}

#[test]
fn restores_sigpipe_default_for_podman() {
    let payload = tempfile::tempfile().unwrap();
    let program = CString::new("sh").unwrap();
    let args = [
        CString::new("sh").unwrap(),
        CString::new("-c").unwrap(),
        CString::new("kill -PIPE $$; exit 99").unwrap(),
    ];

    let status = spawn_with_payload_fds(program.as_c_str(), &args, vec![payload.into()])
        .unwrap()
        .wait()
        .unwrap();

    assert_eq!(status.signal(), Some(libc::SIGPIPE));
}
