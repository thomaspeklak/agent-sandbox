use std::fs;
use std::io;
use std::path::Path;

pub const CONTAINERFILE: &str = include_str!("../../../config/Containerfile");
pub const GUARD_TS: &str = include_str!("../../../agent/extensions/guard.ts");
pub const SETTINGS_EXAMPLE: &str = include_str!("../../../agent/settings.example.json");

/// Write the embedded Containerfile to `path`, always overwriting.
pub fn ensure_containerfile(path: &Path) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, CONTAINERFILE)
}

/// Write the embedded guard.ts to `<pi_sandbox>/extensions/guard.ts`, always overwriting.
pub fn ensure_guard_extension(pi_sandbox: &Path) -> io::Result<()> {
    let dir = pi_sandbox.join("extensions");
    fs::create_dir_all(&dir)?;
    fs::write(dir.join("guard.ts"), GUARD_TS)
}

/// Write the embedded settings template to `<pi_sandbox>/settings.json`,
/// only if it doesn't already exist (user may have customized).
pub fn ensure_settings_template(pi_sandbox: &Path) -> io::Result<()> {
    let target = pi_sandbox.join("settings.json");
    if target.exists() {
        return Ok(());
    }
    fs::create_dir_all(pi_sandbox)?;
    fs::write(&target, SETTINGS_EXAMPLE)?;
    set_permissions_600(&target);
    Ok(())
}

fn set_permissions_600(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
    }
}
