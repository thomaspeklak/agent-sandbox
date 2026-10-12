use std::ffi::CString;
use std::io;
use std::os::fd::OwnedFd;

/// Same anonymous descriptor transport as ordinary launches, for a server boot.
pub(crate) fn spawn_persistent_exec(
    args: &[String],
    descriptors: Vec<OwnedFd>,
) -> io::Result<super::SpawnedProcess> {
    super::exec::ensure_local_podman().map_err(io::Error::other)?;
    let argv = std::iter::once("podman".to_owned())
        .chain(args.iter().cloned())
        .map(CString::new)
        .collect::<Result<Vec<_>, _>>()
        .map_err(io::Error::other)?;
    super::fd_exec::spawn_with_payload_fds(c"podman", &argv, descriptors)
}
