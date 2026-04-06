use std::fs;
use std::io;
use std::ops::ControlFlow;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

#[cfg(test)]
#[path = "util_tests.rs"]
mod util_tests;

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

/// Return the AGS private runtime directory, creating it with restrictive
/// permissions if needed.
pub fn runtime_dir() -> io::Result<PathBuf> {
    let dir = runtime_dir_base();
    ensure_private_dir(&dir)?;
    Ok(dir)
}

/// Ensure a directory exists and is private to the current user.
pub fn ensure_private_dir(path: &Path) -> io::Result<()> {
    fs::create_dir_all(path)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }

    Ok(())
}

fn runtime_dir_base() -> PathBuf {
    if let Ok(dir) = std::env::var("XDG_RUNTIME_DIR")
        && !dir.is_empty()
    {
        return PathBuf::from(dir).join("ags");
    }

    if let Some(cache_dir) = dirs::cache_dir() {
        return cache_dir.join("ags/runtime");
    }

    temp_runtime_dir_fallback()
}

#[cfg(unix)]
fn temp_runtime_dir_fallback() -> PathBuf {
    std::env::temp_dir().join(format!("ags-uid-{}", unsafe { libc::geteuid() }))
}

#[cfg(not(unix))]
fn temp_runtime_dir_fallback() -> PathBuf {
    std::env::temp_dir().join("ags")
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

/// Capitalize the first character of a string.
pub fn capitalize_first(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
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
