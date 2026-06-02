use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::model::ToolConfigError;

#[derive(Debug, Clone, Default, Deserialize)]
pub struct InstallDefinition {
    #[serde(default)]
    pub apt: Option<String>,
    #[serde(default)]
    pub apt_binary: Option<String>,
    #[serde(default)]
    pub dnf: Option<String>,
    #[serde(default)]
    pub dnf_binary: Option<String>,
}

impl InstallDefinition {
    pub fn package_for(&self, manager: PackageManager) -> Option<&str> {
        match manager {
            PackageManager::Apt => self.apt.as_deref(),
            PackageManager::Dnf => self.dnf.as_deref(),
        }
    }

    pub fn binary_for(&self, manager: PackageManager) -> Option<&str> {
        match manager {
            PackageManager::Apt => self.apt_binary.as_deref(),
            PackageManager::Dnf => self.dnf_binary.as_deref(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageManager {
    Apt,
    Dnf,
}

impl PackageManager {
    pub fn label(self) -> &'static str {
        match self {
            Self::Apt => "apt",
            Self::Dnf => "dnf",
        }
    }

    fn binary(self) -> &'static str {
        match self {
            Self::Apt => "apt",
            Self::Dnf => "dnf",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ToolInstaller {
    pub manager: PackageManager,
    pub use_sudo: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallCommand {
    pub program: String,
    pub args: Vec<String>,
}

impl InstallCommand {
    pub fn display_command(&self) -> String {
        std::iter::once(self.program.as_str())
            .chain(self.args.iter().map(String::as_str))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

impl ToolInstaller {
    pub fn command_for(self, package: &str) -> InstallCommand {
        let manager_binary = self.manager.binary();
        let manager_args = ["install".to_owned(), package.to_owned()];

        if self.use_sudo {
            let mut args = Vec::with_capacity(manager_args.len() + 1);
            args.push(manager_binary.to_owned());
            args.extend(manager_args);
            InstallCommand {
                program: "sudo".to_owned(),
                args,
            }
        } else {
            InstallCommand {
                program: manager_binary.to_owned(),
                args: manager_args.to_vec(),
            }
        }
    }
}

pub fn validate_install_definition(
    package: &str,
    tool: &str,
    install: &InstallDefinition,
) -> Result<(), ToolConfigError> {
    if let Some(install_package) = &install.apt {
        validate_install_name(package, tool, "apt package", install_package)?;
    }
    if let Some(binary) = &install.apt_binary {
        validate_install_name(package, tool, "apt binary", binary)?;
    }
    if let Some(install_package) = &install.dnf {
        validate_install_name(package, tool, "dnf package", install_package)?;
    }
    if let Some(binary) = &install.dnf_binary {
        validate_install_name(package, tool, "dnf binary", binary)?;
    }

    Ok(())
}

fn validate_install_name(
    package: &str,
    tool: &str,
    kind: &str,
    value: &str,
) -> Result<(), ToolConfigError> {
    if value.trim().is_empty()
        || value.starts_with('-')
        || value.chars().any(|c| c.is_whitespace() || c.is_control())
    {
        return Err(ToolConfigError::InvalidPackage(format!(
            "{kind} for tool '{tool}' in package '{package}' must be a name, not a shell expression"
        )));
    }

    Ok(())
}

pub fn detect_host_tool_installer() -> Option<ToolInstaller> {
    let manager = detect_host_package_manager()?;
    Some(ToolInstaller {
        manager,
        use_sudo: !running_as_root(),
    })
}

fn detect_host_package_manager() -> Option<PackageManager> {
    match std::env::consts::OS {
        "linux" => detect_linux_package_manager(),
        _ => None,
    }
}

fn detect_linux_package_manager() -> Option<PackageManager> {
    let os_release = fs::read_to_string("/etc/os-release").ok()?;
    let manager = package_manager_from_os_release(&os_release)?;
    find_on_path(manager.binary()).map(|_| manager)
}

pub fn package_manager_from_os_release(content: &str) -> Option<PackageManager> {
    let id = os_release_value(content, "ID");
    if id.as_deref().is_some_and(is_debian_family) {
        return Some(PackageManager::Apt);
    }
    if id.as_deref().is_some_and(is_rpm_dnf_family) {
        return Some(PackageManager::Dnf);
    }

    let id_like = os_release_value(content, "ID_LIKE").unwrap_or_default();
    for value in id_like.split_whitespace() {
        if is_debian_family(value) {
            return Some(PackageManager::Apt);
        }
        if is_rpm_dnf_family(value) {
            return Some(PackageManager::Dnf);
        }
    }

    None
}

fn os_release_value(content: &str, key: &str) -> Option<String> {
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((line_key, value)) = line.split_once('=') else {
            continue;
        };
        if line_key == key {
            return Some(value.trim_matches('"').trim_matches('\'').to_owned());
        }
    }
    None
}

fn is_debian_family(value: &str) -> bool {
    matches!(value, "debian" | "ubuntu" | "linuxmint" | "pop")
}

fn is_rpm_dnf_family(value: &str) -> bool {
    matches!(
        value,
        "fedora" | "rhel" | "centos" | "rocky" | "almalinux" | "ol" | "amzn"
    )
}

fn running_as_root() -> bool {
    #[cfg(unix)]
    {
        // SAFETY: geteuid has no preconditions and does not retain pointers.
        unsafe { libc::geteuid() == 0 }
    }

    #[cfg(not(unix))]
    {
        false
    }
}

pub(crate) fn find_on_path(name: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path_var) {
        let candidate = if dir.as_os_str().is_empty() {
            PathBuf::from(name)
        } else {
            dir.join(name)
        };
        if is_executable_file(&candidate) {
            return Some(candidate);
        }
    }
    None
}

fn is_executable_file(path: &Path) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }

    #[cfg(not(unix))]
    {
        true
    }
}
