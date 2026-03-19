use std::ops::ControlFlow;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// Check if a path has any execute permission bit set.
#[cfg(unix)]
pub fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.metadata()
        .is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
}

#[cfg(not(unix))]
pub fn is_executable(path: &Path) -> bool {
    path.exists()
}

/// Look up a binary by name on `$PATH`, returning the first executable match.
pub fn which(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|dir| dir.join(name))
            .find(|path| path.is_file() && is_executable(path))
    })
}

/// Check if a command is available on `$PATH`.
pub fn has_command(name: &str) -> bool {
    which(name).is_some()
}

/// Return `$XDG_RUNTIME_DIR` if set, otherwise the system temp directory.
pub fn runtime_dir() -> PathBuf {
    std::env::var("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::temp_dir())
}

/// Shell-quote a value using single quotes if it contains special characters.
pub fn shell_quote(s: &str) -> String {
    if s.bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'/' | b'.' | b'-' | b'_'))
    {
        s.to_owned()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

/// Poll `check` every `interval` until it returns `Break(T)` or `timeout` elapses.
///
/// Returns `Some(T)` if `check` broke early, `None` on timeout.
pub fn poll_until<T>(
    timeout: Duration,
    interval: Duration,
    mut check: impl FnMut() -> ControlFlow<T>,
) -> Option<T> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let ControlFlow::Break(val) = check() {
            return Some(val);
        }
        std::thread::sleep(interval);
    }
    None
}
