use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;

use super::defaults::DEFAULT_PI_SPEC;
use crate::network::PodmanNetwork;

/// Validated, path-resolved configuration ready for use by the launch pipeline.
#[derive(Debug, Clone)]
pub struct ValidatedConfig {
    pub config_file: PathBuf,
    pub sandbox: ValidatedSandbox,
    pub mounts: Vec<ValidatedMount>,
    pub tools: Vec<ValidatedTool>,
    pub secrets: Vec<ValidatedSecret>,
    pub browser: BrowserConfig,
    pub update: UpdateConfig,
    pub auth_proxy: AuthProxyConfig,
    pub host_ui: HostUiConfig,
    pub clipboard: ClipboardConfig,
    pub desktop_passthrough: DesktopPassthroughConfig,
    pub psp: PspConfig,
}

#[derive(Debug, Clone)]
pub struct ValidatedSandbox {
    pub image: String,
    pub containerfile: PathBuf,
    pub podman_network: PodmanNetwork,
    pub cache_dir: PathBuf,
    pub gitconfig_path: PathBuf,
    pub auth_key: PathBuf,
    pub sign_key: PathBuf,
    pub bootstrap_files: Vec<String>,
    pub container_boot_dirs: Vec<String>,
    pub passthrough_env: Vec<String>,
}

impl ValidatedConfig {
    /// Return the host source path for an exact container mount path.
    pub fn mount_host_for_container(&self, container_path: &str) -> Option<&PathBuf> {
        self.mounts
            .iter()
            .find(|m| m.container == container_path)
            .map(|m| &m.host)
    }
}

#[derive(Debug, Clone)]
pub struct ValidatedMount {
    pub host: PathBuf,
    pub container: String,
    pub mode: MountMode,
    pub kind: MountKind,
    pub when: MountWhen,
    pub create: bool,
    pub optional: bool,
    pub source: String,
}

#[derive(Debug, Clone)]
pub struct ValidatedTool {
    pub name: String,
    pub path: PathBuf,
    pub container_path: String,
    pub mode: MountMode,
    pub when: MountWhen,
    pub optional: bool,
}

#[derive(Debug, Clone)]
pub struct ValidatedSecret {
    pub env: String,
    pub source: SecretSource,
    pub origin: String,
    pub tool: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MountMode {
    Ro,
    Rw,
}

impl fmt::Display for MountMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ro => f.write_str("ro"),
            Self::Rw => f.write_str("rw"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MountKind {
    Dir,
    File,
}

impl fmt::Display for MountKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Dir => f.write_str("dir"),
            Self::File => f.write_str("file"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MountWhen {
    Always,
    Browser,
}

impl fmt::Display for MountWhen {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Always => f.write_str("always"),
            Self::Browser => f.write_str("browser"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SecretSource {
    Env {
        from_env: String,
    },
    SecretTool {
        attributes: BTreeMap<String, String>,
    },
}

#[derive(Debug, Clone, Default)]
pub struct BrowserConfig {
    pub enabled: bool,
    pub command: String,
    pub profile_dir: PathBuf,
    pub debug_port: u16,
    pub pi_skill_path: String,
    pub command_args: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct UpdateConfig {
    pub pi_spec: String,
    pub minimum_release_age: u32,
}

#[derive(Debug, Clone, Default)]
pub struct AuthProxyConfig {
    pub auto_allow_domains: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClipboardMode {
    Off,
    Read,
    ReadWrite,
}

impl ClipboardMode {
    pub fn can_read(self) -> bool {
        matches!(self, Self::Read | Self::ReadWrite)
    }

    pub fn can_write(self) -> bool {
        matches!(self, Self::ReadWrite)
    }
}

impl fmt::Display for ClipboardMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Off => f.write_str("off"),
            Self::Read => f.write_str("read"),
            Self::ReadWrite => f.write_str("readwrite"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ClipboardConfig {
    pub enabled: bool,
    pub mode: ClipboardMode,
    pub max_bytes: usize,
    pub approval_required: bool,
    pub approval_seconds: u64,
    pub approve_writes: bool,
}

impl ClipboardConfig {
    pub fn effective_mode(&self) -> ClipboardMode {
        if self.enabled {
            self.mode
        } else {
            ClipboardMode::Off
        }
    }
}

impl Default for ClipboardConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            mode: ClipboardMode::ReadWrite,
            max_bytes: 32 * 1024 * 1024,
            approval_required: true,
            approval_seconds: 300,
            approve_writes: false,
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct DesktopPassthroughConfig {
    pub wayland: bool,
}

#[derive(Debug, Clone)]
pub struct HostUiConfig {
    pub enabled: bool,
    pub binary: String,
    pub renderer: String,
    pub renderer_bin: Option<PathBuf>,
    pub idle_timeout_ms: u64,
    pub log_level: String,
}

impl Default for HostUiConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            binary: "glimpse-host-ui".to_owned(),
            renderer: "stub".to_owned(),
            renderer_bin: None,
            idle_timeout_ms: 0,
            log_level: "info".to_owned(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct PspConfig {
    /// Optional override path to the `psp` binary. If empty, uses PATH lookup.
    pub binary: String,
}

impl Default for UpdateConfig {
    fn default() -> Self {
        Self {
            pi_spec: DEFAULT_PI_SPEC.to_owned(),
            minimum_release_age: 1440,
        }
    }
}
