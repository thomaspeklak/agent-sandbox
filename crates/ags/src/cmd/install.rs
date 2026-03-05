use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::assets;
use crate::cli::InstallOptions;

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

/// Install ags: write embedded assets and ensure config layout.
pub fn run(opts: &InstallOptions) -> Result<(), InstallError> {
    let home = dirs::home_dir().ok_or(InstallError::HomeDir)?;
    let bin_dir = home.join(".local/bin");
    let config_dir = home.join(".config/ags");
    let agent_dir =
        std::env::var("AGS_AGENT_DIR").map_or_else(|_| config_dir.join("pi"), PathBuf::from);

    fs::create_dir_all(&config_dir)?;
    fs::create_dir_all(agent_dir.join("extensions"))?;

    // Write embedded Containerfile
    let containerfile = config_dir.join("Containerfile");
    assets::ensure_containerfile(&containerfile)?;
    println!("Wrote Containerfile: {}", containerfile.display());

    // Write guard extension
    assets::ensure_guard_extension(&agent_dir)?;
    println!(
        "Wrote guard extension: {}",
        agent_dir.join("extensions/guard.ts").display()
    );

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

    if opts.link_self {
        fs::create_dir_all(&bin_dir)?;
        let link_path = bin_dir.join("ags");
        link_self_executable(&link_path, opts.force)?;
    } else if opts.force {
        eprintln!("warning: --force has no effect without --link-self");
    }

    // Legacy binary/alias cleanup intentionally omitted.
    // Binary aliases are treated as userland responsibility.

    println!("\nInstall complete.");
    println!("Run: ags doctor");
    if !opts.link_self {
        println!("Tip: run `ags install --link-self` to link ags into ~/.local/bin");
    }
    Ok(())
}

/// Uninstall currently performs no binary alias cleanup.
pub fn uninstall() -> Result<(), InstallError> {
    let _ = dirs::home_dir().ok_or(InstallError::HomeDir)?;
    println!("Uninstall complete.");
    Ok(())
}

fn link_self_executable(link_path: &Path, force: bool) -> Result<(), InstallError> {
    let mut current = std::env::current_exe()?;
    if let Ok(canon) = fs::canonicalize(&current) {
        current = canon;
    }

    if !is_stable_executable_path(&current) && !force {
        eprintln!(
            "warning: current executable looks build-local ({}); skipping link (use --force to override)",
            current.display()
        );
        return Ok(());
    }

    if let Ok(meta) = fs::symlink_metadata(link_path) {
        if meta.file_type().is_symlink() {
            if let Ok(existing_target) = fs::read_link(link_path) {
                let existing_abs = if existing_target.is_absolute() {
                    existing_target
                } else {
                    link_path
                        .parent()
                        .unwrap_or_else(|| Path::new("."))
                        .join(existing_target)
                };
                if let Ok(existing_canon) = fs::canonicalize(existing_abs)
                    && existing_canon == current
                {
                    println!("Self-link already up to date: {}", link_path.display());
                    return Ok(());
                }
            }

            if !force {
                eprintln!(
                    "warning: existing symlink at {} (use --force to replace)",
                    link_path.display()
                );
                return Ok(());
            }
            fs::remove_file(link_path)?;
        } else {
            if !force {
                eprintln!(
                    "warning: existing non-symlink at {} (use --force to replace)",
                    link_path.display()
                );
                return Ok(());
            }
            if meta.is_dir() {
                eprintln!(
                    "warning: {} is a directory; refusing to replace",
                    link_path.display()
                );
                return Ok(());
            }
            fs::remove_file(link_path)?;
        }
    }

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&current, link_path)?;
        println!(
            "Linked self binary: {} -> {}",
            link_path.display(),
            current.display()
        );
    }
    #[cfg(not(unix))]
    {
        let _ = (current, link_path);
        eprintln!("warning: self-link install is only implemented on unix platforms");
    }

    Ok(())
}

fn is_stable_executable_path(path: &Path) -> bool {
    let s = path.to_string_lossy();
    !s.contains("/target/debug/")
        && !s.contains("/target/release/")
        && !s.contains("\\target\\debug\\")
        && !s.contains("\\target\\release\\")
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
