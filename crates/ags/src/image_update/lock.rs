//! Per-output advisory lock serializing image creation, update, and
//! publication. It lives in the canonical state directory, so configs with
//! different `sandbox.cache_dir` values still share it.

use std::fs::{File, OpenOptions};
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;

use super::error::ImageUpdateError;

/// Held until dropped; closing the file releases the `flock`.
#[derive(Debug)]
pub struct UpdateLock {
    _file: File,
}

fn open(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .mode(0o600)
        .open(path)
}

fn flock(file: &File, operation: libc::c_int) -> io::Result<()> {
    loop {
        // SAFETY: flock only reads the descriptor, which `file` keeps open.
        if unsafe { libc::flock(file.as_raw_fd(), operation) } == 0 {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        if error.kind() != io::ErrorKind::Interrupted {
            return Err(error);
        }
    }
}

/// Take the lock without waiting; `Ok(None)` when another process holds it.
pub fn try_acquire(path: &Path) -> Result<Option<UpdateLock>, ImageUpdateError> {
    let file = open(path).map_err(|error| lock_error(path, error))?;
    match flock(&file, libc::LOCK_EX | libc::LOCK_NB) {
        Ok(()) => Ok(Some(UpdateLock { _file: file })),
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => Ok(None),
        Err(error) => Err(lock_error(path, error)),
    }
}

/// Take the lock, waiting for a concurrent update of the same image.
pub fn acquire(path: &Path, image: &str) -> Result<UpdateLock, ImageUpdateError> {
    if let Some(lock) = try_acquire(path)? {
        return Ok(lock);
    }
    eprintln!("Waiting for another AGS update of {image} to finish...");
    let file = open(path).map_err(|error| lock_error(path, error))?;
    flock(&file, libc::LOCK_EX).map_err(|error| lock_error(path, error))?;
    Ok(UpdateLock { _file: file })
}

fn lock_error(path: &Path, error: io::Error) -> ImageUpdateError {
    ImageUpdateError::Lock(format!("{}: {error}", path.display()))
}

#[cfg(test)]
#[path = "lock_tests.rs"]
mod tests;
