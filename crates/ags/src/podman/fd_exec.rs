//! Descriptor handoff for anonymous payloads.
//!
//! `std::process::Command::pre_exec` cannot safely remap to fd 3: Rust uses
//! an internal exec-error pipe in that range. `posix_spawnp` performs the
//! remapping with libc file actions, avoiding Rust code in a post-fork child.

use std::ffi::{CStr, CString};
use std::io;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};

unsafe extern "C" {
    static mut environ: *mut *mut libc::c_char;
}

pub(super) struct SpawnedProcess {
    pid: libc::pid_t,
}

impl SpawnedProcess {
    pub(super) fn wait(self) -> io::Result<std::process::ExitStatus> {
        let mut status = 0;
        loop {
            let result = unsafe { libc::waitpid(self.pid, &mut status, 0) };
            if result == self.pid {
                return Ok(std::os::unix::process::ExitStatusExt::from_raw(status));
            }
            if result < 0 && io::Error::last_os_error().kind() != io::ErrorKind::Interrupted {
                return Err(io::Error::last_os_error());
            }
        }
    }
}

/// Spawn `program args` with `payloads` remapped to contiguous fds from 3.
///
/// The payloads are first duplicated above the target range in the parent so
/// file actions cannot clobber a later source descriptor. The parent drops all
/// original and duplicate payload descriptors as soon as `posix_spawnp`
/// returns.
pub(super) fn spawn_with_payload_fds(
    program: &CStr,
    args: &[CString],
    payloads: Vec<OwnedFd>,
) -> io::Result<SpawnedProcess> {
    if payloads.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "payload descriptor list is empty",
        ));
    }
    let target_limit = 3usize.checked_add(payloads.len()).ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "too many payload descriptors")
    })?;
    if target_limit > libc::c_int::MAX as usize {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "too many payload descriptors",
        ));
    }

    let sources = duplicate_sources(&payloads, target_limit as RawFd)?;
    let mut actions = SpawnFileActions::new()?;
    for (offset, source) in sources.iter().enumerate() {
        let target = 3 + offset as RawFd;
        actions.duplicate(source.as_raw_fd(), target)?;
        actions.close(source.as_raw_fd())?;
    }

    let mut argv: Vec<*mut libc::c_char> = args.iter().map(|arg| arg.as_ptr().cast_mut()).collect();
    argv.push(std::ptr::null_mut());
    let mut pid = 0;
    let result = unsafe {
        libc::posix_spawnp(
            &mut pid,
            program.as_ptr(),
            actions.as_ptr(),
            std::ptr::null(),
            argv.as_ptr(),
            environ,
        )
    };
    drop(sources);
    drop(payloads);
    if result != 0 {
        return Err(io::Error::from_raw_os_error(result));
    }
    Ok(SpawnedProcess { pid })
}

fn duplicate_sources(payloads: &[OwnedFd], minimum: RawFd) -> io::Result<Vec<OwnedFd>> {
    payloads
        .iter()
        .map(|payload| {
            let duplicate =
                unsafe { libc::fcntl(payload.as_raw_fd(), libc::F_DUPFD_CLOEXEC, minimum) };
            if duplicate < 0 {
                Err(io::Error::last_os_error())
            } else {
                // `fcntl(F_DUPFD_CLOEXEC)` returned a new descriptor owned here.
                Ok(unsafe { OwnedFd::from_raw_fd(duplicate) })
            }
        })
        .collect()
}

struct SpawnFileActions {
    inner: libc::posix_spawn_file_actions_t,
}

impl SpawnFileActions {
    fn new() -> io::Result<Self> {
        let mut inner = unsafe { std::mem::zeroed() };
        let result = unsafe { libc::posix_spawn_file_actions_init(&mut inner) };
        if result != 0 {
            return Err(io::Error::from_raw_os_error(result));
        }
        Ok(Self { inner })
    }

    fn duplicate(&mut self, source: RawFd, target: RawFd) -> io::Result<()> {
        let result =
            unsafe { libc::posix_spawn_file_actions_adddup2(&mut self.inner, source, target) };
        result_to_io(result)
    }

    fn close(&mut self, fd: RawFd) -> io::Result<()> {
        let result = unsafe { libc::posix_spawn_file_actions_addclose(&mut self.inner, fd) };
        result_to_io(result)
    }

    fn as_ptr(&mut self) -> *mut libc::posix_spawn_file_actions_t {
        &mut self.inner
    }
}

impl Drop for SpawnFileActions {
    fn drop(&mut self) {
        unsafe { libc::posix_spawn_file_actions_destroy(&mut self.inner) };
    }
}

fn result_to_io(result: libc::c_int) -> io::Result<()> {
    if result == 0 {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::fd::AsRawFd;

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
}
