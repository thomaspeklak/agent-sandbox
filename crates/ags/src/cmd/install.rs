use std::fmt;
use std::fs;
use std::io;
use std::os::unix::fs as unix_fs;
use std::path::{Path, PathBuf};

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

/// Install symlinks for ags and legacy pis commands.
pub fn run(project_root: &Path) -> Result<(), InstallError> {
    let home = dirs::home_dir().ok_or(InstallError::HomeDir)?;
    let bin_dir = home.join(".local/bin");
    let config_link = home.join(".config/pi-sandbox");
    let agent_dir = std::env::var("PI_SBOX_AGENT_DIR")
        .map_or_else(|_| home.join(".pi/agent-sandbox"), PathBuf::from);

    fs::create_dir_all(&bin_dir)?;
    fs::create_dir_all(home.join(".config"))?;
    fs::create_dir_all(agent_dir.join("extensions"))?;

    // Link config directory
    link_path(&project_root.join("config"), &config_link)?;

    // Bootstrap sandbox settings
    bootstrap_sandbox_settings(project_root, &agent_dir, &home)?;

    // Link guard extension
    link_path(
        &project_root.join("agent/extensions/guard.ts"),
        &agent_dir.join("extensions/guard.ts"),
    )?;

    // Link CLI commands
    let commands = ["pis", "pisb", "pis-setup", "pis-doctor", "pis-update"];
    for cmd in &commands {
        let source = project_root.join("bin").join(cmd);
        make_executable(&source);
        link_path(&source, &bin_dir.join(cmd))?;
    }

    // Ensure pis-run is executable but not linked into PATH
    make_executable(&project_root.join("bin/pis-run"));
    let _ = fs::remove_file(bin_dir.join("pis-run"));

    // Remove legacy aliases
    for legacy in &["pi-sbox", "pi-sbox-browser", "pi-sbox-setup"] {
        let path = bin_dir.join(legacy);
        if path.exists() || path.symlink_metadata().is_ok() {
            fs::remove_file(&path)?;
            println!("Removed legacy alias: {}", path.display());
        }
    }

    println!("\nInstall complete.");
    println!("Run: pis doctor");
    Ok(())
}

/// Remove symlinks that point back into the project.
pub fn uninstall(project_root: &Path) -> Result<(), InstallError> {
    let home = dirs::home_dir().ok_or(InstallError::HomeDir)?;
    let bin_dir = home.join(".local/bin");
    let config_link = home.join(".config/pi-sandbox");
    let agent_dir = std::env::var("PI_SBOX_AGENT_DIR")
        .map_or_else(|_| home.join(".pi/agent-sandbox"), PathBuf::from);

    let commands = [
        "pis",
        "pisb",
        "pis-setup",
        "pis-doctor",
        "pis-update",
        "pis-run",
    ];
    for cmd in &commands {
        unlink_if_points_to(project_root, &bin_dir.join(cmd));
    }

    unlink_if_points_to(project_root, &config_link);
    unlink_if_points_to(project_root, &agent_dir.join("settings.json"));
    unlink_if_points_to(project_root, &agent_dir.join("extensions/guard.ts"));

    println!("Uninstall complete.");
    Ok(())
}

fn link_path(source: &Path, target: &Path) -> Result<(), InstallError> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }

    // Already correctly linked?
    if target.symlink_metadata().is_ok() {
        if let Ok(current) = fs::read_link(target) {
            let current_resolved = fs::canonicalize(&current).unwrap_or(current);
            let source_resolved = fs::canonicalize(source).unwrap_or_else(|_| source.to_owned());
            if current_resolved == source_resolved {
                println!("Already linked: {}", target.display());
                return Ok(());
            }
        }
        // Wrong target — back up and relink
        backup(target)?;
    } else if target.exists() {
        backup(target)?;
    }

    unix_fs::symlink(source, target)?;
    println!("Linked: {} -> {}", target.display(), source.display());
    Ok(())
}

fn backup(target: &Path) -> Result<(), InstallError> {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let backup_name = format!("{}.bak.{stamp}", target.display());
    let backup_path = PathBuf::from(&backup_name);
    fs::rename(target, &backup_path)?;
    println!(
        "Backed up: {} -> {}",
        target.display(),
        backup_path.display()
    );
    Ok(())
}

fn unlink_if_points_to(project_root: &Path, path: &Path) {
    if fs::symlink_metadata(path).is_err() {
        return;
    }
    let Ok(link_target) = fs::read_link(path) else {
        return;
    };
    let resolved = fs::canonicalize(&link_target).unwrap_or(link_target);
    let root_resolved = fs::canonicalize(project_root).unwrap_or_else(|_| project_root.to_owned());
    if resolved.starts_with(&root_resolved) {
        let _ = fs::remove_file(path);
        println!("Removed: {}", path.display());
    }
}

fn bootstrap_sandbox_settings(
    project_root: &Path,
    agent_dir: &Path,
    home: &Path,
) -> Result<(), InstallError> {
    let target = agent_dir.join("settings.json");

    // Remove legacy symlink if it points to project
    if let Ok(link_target) = fs::read_link(&target) {
        let resolved = fs::canonicalize(&link_target).unwrap_or(link_target);
        if resolved.starts_with(project_root) {
            fs::remove_file(&target)?;
            println!("Removed legacy settings symlink: {}", target.display());
        }
    }

    if target.exists() {
        println!("Using existing sandbox settings: {}", target.display());
        return Ok(());
    }

    fs::create_dir_all(agent_dir)?;

    // Try host settings first
    let host_settings = home.join(".pi/agent/settings.json");
    if host_settings.exists() {
        fs::copy(&host_settings, &target)?;
        set_permissions_600(&target);
        println!(
            "Copied sandbox settings from host: {} -> {}",
            host_settings.display(),
            target.display()
        );
        return Ok(());
    }

    // Fall back to template
    let template = project_root.join("agent/settings.example.json");
    if template.exists() {
        fs::copy(&template, &target)?;
        set_permissions_600(&target);
        println!(
            "Copied sandbox settings from template: {} -> {}",
            template.display(),
            target.display()
        );
        return Ok(());
    }

    eprintln!(
        "Missing sandbox settings source. Expected one of:\n  {}\n  {}",
        host_settings.display(),
        template.display()
    );
    Ok(())
}

fn make_executable(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if let Ok(meta) = fs::metadata(path) {
            let mode = meta.permissions().mode() | 0o111;
            let _ = fs::set_permissions(path, fs::Permissions::from_mode(mode));
        }
    }
}

fn set_permissions_600(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
    }
}
