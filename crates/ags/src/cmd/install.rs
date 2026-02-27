use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::assets;

#[derive(Debug)]
pub enum InstallError {
    Io(io::Error),
    HomeDir,
}

impl fmt::Display for InstallError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "install I/O error: {e}"),
            Self::HomeDir => f.write_str("could not determine home directory"),
        }
    }
}

impl std::error::Error for InstallError {}

impl From<io::Error> for InstallError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

/// Install ags: write embedded assets and clean up legacy symlinks.
pub fn run() -> Result<(), InstallError> {
    let home = dirs::home_dir().ok_or(InstallError::HomeDir)?;
    let bin_dir = home.join(".local/bin");
    let config_dir = home.join(".config/ags");
    let agent_dir = std::env::var("PI_SBOX_AGENT_DIR")
        .map_or_else(|_| config_dir.join("pi"), PathBuf::from);

    fs::create_dir_all(&config_dir)?;
    fs::create_dir_all(agent_dir.join("extensions"))?;

    // Write embedded Containerfile
    let containerfile = config_dir.join("Containerfile");
    assets::ensure_containerfile(&containerfile)?;
    println!("Wrote Containerfile: {}", containerfile.display());

    // Write guard extension
    assets::ensure_guard_extension(&agent_dir)?;
    println!("Wrote guard extension: {}", agent_dir.join("extensions/guard.ts").display());

    // Write settings template (only if missing)
    let settings = agent_dir.join("settings.json");
    if !settings.exists() {
        assets::ensure_settings_template(&agent_dir)?;
        println!("Wrote settings template: {}", settings.display());
    } else {
        println!("Using existing settings: {}", settings.display());
    }

    // Remove legacy config-dir symlink if it points elsewhere
    remove_legacy_symlink(&config_dir);

    // Remove legacy shell script symlinks
    let legacy_commands = [
        "pis",
        "pisb",
        "pis-setup",
        "pis-doctor",
        "pis-update",
        "pis-run",
        "pi-sbox",
        "pi-sbox-browser",
        "pi-sbox-setup",
    ];
    for cmd in &legacy_commands {
        let path = bin_dir.join(cmd);
        if path.symlink_metadata().is_ok() {
            let _ = fs::remove_file(&path);
            println!("Removed legacy symlink: {}", path.display());
        }
    }

    println!("\nInstall complete.");
    println!("Run: ags doctor");
    Ok(())
}

/// Remove symlinks that were installed by a previous version.
pub fn uninstall() -> Result<(), InstallError> {
    let home = dirs::home_dir().ok_or(InstallError::HomeDir)?;
    let bin_dir = home.join(".local/bin");

    let commands = [
        "pis",
        "pisb",
        "pis-setup",
        "pis-doctor",
        "pis-update",
        "pis-run",
        "pi-sbox",
        "pi-sbox-browser",
        "pi-sbox-setup",
    ];
    for cmd in &commands {
        let path = bin_dir.join(cmd);
        if path.symlink_metadata().is_ok() {
            let _ = fs::remove_file(&path);
            println!("Removed: {}", path.display());
        }
    }

    println!("Uninstall complete.");
    Ok(())
}

/// If `path` is a symlink, remove it so we can use it as a real directory.
fn remove_legacy_symlink(path: &Path) {
    if let Ok(meta) = fs::symlink_metadata(path)
        && meta.file_type().is_symlink()
    {
        let _ = fs::remove_file(path);
        println!("Removed legacy config symlink: {}", path.display());
    }
}
