//! Host-side 1Password Secure Note retrieval.
//!
//! This module deliberately never reads or parses item JSON. `op` writes its
//! stdout directly to an anonymous, sealed memfd; only the container bootstrap
//! may consume that payload.

use std::fmt;
use std::fs::File;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

/// Largest accepted serialized Secure Note item (1 MiB).
const MAX_ITEM_BYTES: i64 = 1024 * 1024;

/// Owns the non-secret, private bootstrap asset for precisely one payload run.
/// A scrubbed helper removes it if the host is terminated before Rust can drop
/// the `TempDir`. It starts before payload descriptors exist and cannot inherit
/// them; normal returns synchronously stop it before removing the directory.
pub(crate) struct BootstrapAssetGuard {
    dir: tempfile::TempDir,
    reaper: Child,
}

impl BootstrapAssetGuard {
    pub(crate) fn prepare(runtime_base: &Path) -> std::io::Result<Self> {
        let dir = tempfile::Builder::new()
            .prefix("ags-onepassword-")
            .tempdir_in(runtime_base)?;
        crate::assets::ensure_onepassword_bootstrap(dir.path())?;
        let reaper = spawn_bootstrap_reaper(dir.path())?;
        Ok(Self { dir, reaper })
    }

    pub(crate) fn path(&self) -> PathBuf {
        self.dir
            .path()
            .join(crate::assets::ONEPASSWORD_BOOTSTRAP_NAME)
    }
}

impl Drop for BootstrapAssetGuard {
    fn drop(&mut self) {
        let _ = self.reaper.kill();
        let _ = self.reaper.wait();
    }
}

fn spawn_bootstrap_reaper(path: &Path) -> std::io::Result<Child> {
    // The expected parent PID is captured before spawn. If AGS exits before
    // Python starts, the mismatch makes the helper clean up immediately.
    const REAPER: &str = r#"import os, shutil, signal, sys, time
for interrupt in (signal.SIGINT, signal.SIGTERM, signal.SIGHUP):
    signal.signal(interrupt, signal.SIG_IGN)
expected_parent = int(sys.argv[2])
while os.getppid() == expected_parent:
    time.sleep(0.1)
shutil.rmtree(sys.argv[1], ignore_errors=True)
"#;
    Command::new("python3")
        .args(["-c", REAPER])
        .arg(path)
        .arg(std::process::id().to_string())
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SourceRef {
    vault: String,
    item: String,
}

impl SourceRef {
    pub(crate) fn parse(raw: &str) -> Result<Self, OnePasswordError> {
        let Some((vault, item)) = raw.split_once('/') else {
            return Err(OnePasswordError::InvalidSource);
        };
        if vault.is_empty() || item.is_empty() {
            return Err(OnePasswordError::InvalidSource);
        }
        Ok(Self {
            vault: vault.to_owned(),
            item: item.to_owned(),
        })
    }

    pub(crate) fn vault(&self) -> &str {
        &self.vault
    }

    pub(crate) fn item(&self) -> &str {
        &self.item
    }
}

impl fmt::Display for SourceRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.vault, self.item)
    }
}

#[derive(Debug)]
pub(crate) enum OnePasswordError {
    InvalidSource,
    Memfd(std::io::ErrorKind),
    Spawn {
        source: SourceRef,
        kind: std::io::ErrorKind,
    },
    LookupFailed {
        source: SourceRef,
        status: Option<i32>,
    },
    PayloadStat {
        source: SourceRef,
        kind: std::io::ErrorKind,
    },
    EmptyPayload {
        source: SourceRef,
    },
    OversizedPayload {
        source: SourceRef,
    },
    Rewind {
        source: SourceRef,
        kind: std::io::ErrorKind,
    },
    Seal {
        source: SourceRef,
        kind: std::io::ErrorKind,
    },
}

impl fmt::Display for OnePasswordError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidSource => f.write_str("invalid 1Password source (expected VAULT/ITEM)"),
            Self::Memfd(kind) => write!(f, "could not create 1Password payload descriptor: {kind}"),
            Self::Spawn {
                source,
                kind: std::io::ErrorKind::NotFound,
            } => write!(
                f,
                "op is not installed or not on PATH; required by --op-secret-set for {source}"
            ),
            Self::Spawn { source, kind } => write!(f, "could not start op for {source}: {kind}"),
            Self::LookupFailed { source, status } => {
                write!(
                    f,
                    "op lookup failed for {source} (status {})",
                    status.unwrap_or(-1)
                )
            }
            Self::PayloadStat { source, kind } => {
                write!(
                    f,
                    "could not inspect 1Password payload for {source}: {kind}"
                )
            }
            Self::EmptyPayload { source } => write!(f, "op returned an empty item for {source}"),
            Self::OversizedPayload { source } => {
                write!(f, "op returned an oversized item for {source}")
            }
            Self::Rewind { source, kind } => {
                write!(f, "could not rewind 1Password payload for {source}: {kind}")
            }
            Self::Seal { source, kind } => {
                write!(f, "could not seal 1Password payload for {source}: {kind}")
            }
        }
    }
}

impl std::error::Error for OnePasswordError {}

/// A sealed, rewound anonymous item payload. Its bytes are never exposed here.
pub(crate) struct PreparedItem {
    source: SourceRef,
    fd: OwnedFd,
}

impl PreparedItem {
    #[cfg(test)]
    fn source(&self) -> &SourceRef {
        &self.source
    }

    #[cfg(test)]
    fn fd(&self) -> std::os::fd::RawFd {
        self.fd.as_raw_fd()
    }

    pub(crate) fn into_fd(self) -> OwnedFd {
        self.fd
    }
}

impl fmt::Debug for PreparedItem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PreparedItem")
            .field("source", &self.source)
            .field("fd", &self.fd.as_raw_fd())
            .finish()
    }
}

/// Retrieve every requested item into a sealed anonymous descriptor in order.
pub(crate) fn prepare(sources: &[SourceRef]) -> Result<Vec<PreparedItem>, OnePasswordError> {
    prepare_with_op(sources, Path::new("op"))
}

fn prepare_with_op(
    sources: &[SourceRef],
    op_path: &Path,
) -> Result<Vec<PreparedItem>, OnePasswordError> {
    sources
        .iter()
        .cloned()
        .map(|source| prepare_one(source, op_path))
        .collect()
}

fn prepare_one(source: SourceRef, op_path: &Path) -> Result<PreparedItem, OnePasswordError> {
    let fd = create_memfd().map_err(|error| OnePasswordError::Memfd(error.kind()))?;
    let stdout: File = fd
        .try_clone()
        .map_err(|error| OnePasswordError::Memfd(error.kind()))?
        .into();
    let mut op = Command::new(op_path);
    op.args(["item", "get", source.item(), "--vault", source.vault()])
        .args(["--format=json", "--reveal"])
        .stdin(Stdio::inherit())
        .stderr(Stdio::inherit())
        .stdout(Stdio::from(stdout));
    let status = op.status().map_err(|error| OnePasswordError::Spawn {
        source: source.clone(),
        kind: error.kind(),
    })?;
    // `Command` owns the duplicated stdout descriptor. Drop it before adding
    // F_SEAL_WRITE: an extra writable reference would make sealing fail EBUSY.
    drop(op);
    if !status.success() {
        return Err(OnePasswordError::LookupFailed {
            source,
            status: status.code(),
        });
    }

    validate_rewind_and_seal(&fd, &source)?;
    Ok(PreparedItem { source, fd })
}

fn create_memfd() -> std::io::Result<OwnedFd> {
    let name = c"ags-op-item";
    let fd =
        unsafe { libc::memfd_create(name.as_ptr(), libc::MFD_CLOEXEC | libc::MFD_ALLOW_SEALING) };
    if fd < 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok(unsafe { OwnedFd::from_raw_fd(fd) })
}

fn validate_rewind_and_seal(fd: &OwnedFd, source: &SourceRef) -> Result<(), OnePasswordError> {
    let mut stat = unsafe { std::mem::zeroed::<libc::stat>() };
    if unsafe { libc::fstat(fd.as_raw_fd(), &mut stat) } != 0 {
        return Err(OnePasswordError::PayloadStat {
            source: source.clone(),
            kind: std::io::Error::last_os_error().kind(),
        });
    }
    if stat.st_size == 0 {
        return Err(OnePasswordError::EmptyPayload {
            source: source.clone(),
        });
    }
    if stat.st_size < 0 || stat.st_size > MAX_ITEM_BYTES {
        return Err(OnePasswordError::OversizedPayload {
            source: source.clone(),
        });
    }
    if unsafe { libc::lseek(fd.as_raw_fd(), 0, libc::SEEK_SET) } < 0 {
        return Err(OnePasswordError::Rewind {
            source: source.clone(),
            kind: std::io::Error::last_os_error().kind(),
        });
    }
    let seals = libc::F_SEAL_WRITE | libc::F_SEAL_GROW | libc::F_SEAL_SHRINK | libc::F_SEAL_SEAL;
    if unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_ADD_SEALS, seals) } != 0 {
        return Err(OnePasswordError::Seal {
            source: source.clone(),
            kind: std::io::Error::last_os_error().kind(),
        });
    }
    Ok(())
}

#[cfg(test)]
#[path = "onepassword_tests.rs"]
mod tests;
