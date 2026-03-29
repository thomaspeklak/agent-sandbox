use std::ffi::OsString;
use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use super::{ensure_private_dir, runtime_dir};

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

struct EnvVarGuard {
    name: &'static str,
    original: Option<OsString>,
}

impl EnvVarGuard {
    fn set(name: &'static str, value: impl AsRef<std::ffi::OsStr>) -> Self {
        let original = std::env::var_os(name);
        unsafe { std::env::set_var(name, value) };
        Self { name, original }
    }

    fn remove(name: &'static str) -> Self {
        let original = std::env::var_os(name);
        unsafe { std::env::remove_var(name) };
        Self { name, original }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        match self.original.take() {
            Some(value) => unsafe { std::env::set_var(self.name, value) },
            None => unsafe { std::env::remove_var(self.name) },
        }
    }
}

#[test]
fn ensure_private_dir_creates_directory() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("private/runtime");

    ensure_private_dir(&dir).unwrap();

    assert!(dir.is_dir());
}

#[test]
fn runtime_dir_prefers_xdg_runtime_dir() {
    let _guard = env_lock().lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let _xdg_runtime = EnvVarGuard::set("XDG_RUNTIME_DIR", temp.path());

    let dir = runtime_dir().unwrap();

    assert_eq!(dir, temp.path().join("ags"));
    assert!(dir.is_dir());
}

#[test]
#[cfg(unix)]
fn ensure_private_dir_sets_owner_only_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("private");
    fs::create_dir_all(&dir).unwrap();
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();

    ensure_private_dir(&dir).unwrap();

    let mode = fs::metadata(&dir).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o700);
}

#[test]
#[cfg(unix)]
fn runtime_dir_fallback_uses_private_cache_location_when_xdg_is_missing() {
    let _guard = env_lock().lock().unwrap();
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    fs::create_dir_all(&home).unwrap();

    let _xdg_runtime = EnvVarGuard::remove("XDG_RUNTIME_DIR");
    let _xdg_cache = EnvVarGuard::remove("XDG_CACHE_HOME");
    let _home = EnvVarGuard::set("HOME", &home);

    let dir = runtime_dir().unwrap();

    assert_eq!(dir, PathBuf::from(&home).join(".cache/ags/runtime"));
}
