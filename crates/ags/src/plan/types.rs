use std::fmt;
use std::path::PathBuf;

use crate::config::MountMode;

/// Error during launch plan construction.
#[derive(Debug)]
pub enum PlanError {
    /// Working directory could not be resolved.
    WorkdirResolve(String),
    /// Failed to create a required directory on the host.
    DirCreate {
        path: PathBuf,
        source: std::io::Error,
    },
    /// A required (non-optional) mount's host path does not exist.
    MountMissing { host: PathBuf, context: String },
    /// An environment variable has an invalid value.
    InvalidEnv { var: String, value: String },
}

impl fmt::Display for PlanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WorkdirResolve(msg) => write!(f, "workdir resolve error: {msg}"),
            Self::DirCreate { path, source } => {
                write!(f, "failed to create {}: {source}", path.display())
            }
            Self::MountMissing { host, context } => {
                write!(
                    f,
                    "required mount source missing: {} ({context})",
                    host.display()
                )
            }
            Self::InvalidEnv { var, value } => {
                write!(f, "invalid {var} value: {value}")
            }
        }
    }
}

impl std::error::Error for PlanError {}

/// Complete description of a container launch, ready for podman rendering.
#[derive(Debug, Clone)]
pub struct LaunchPlan {
    pub image: String,
    pub containerfile: PathBuf,
    pub workdir: WorkdirMapping,
    pub mounts: Vec<PlanMount>,
    pub env: PlanEnv,
    pub security: SecurityConfig,
    pub network_mode: String,
    pub boot_dirs: Vec<String>,
    pub entrypoint: String,
}

/// Host-to-container working directory mapping.
#[derive(Debug, Clone)]
pub struct WorkdirMapping {
    pub host: PathBuf,
    pub container: String,
}

/// A single bind mount in the launch plan.
#[derive(Debug, Clone)]
pub struct PlanMount {
    pub host: PathBuf,
    pub container: String,
    pub mode: MountMode,
}

/// All environment configuration for the container.
#[derive(Debug, Clone)]
pub struct PlanEnv {
    /// Explicit KEY=VALUE pairs set with `-e KEY=VALUE`.
    pub inline: Vec<(String, String)>,
    /// Host env vars passed through by name with `-e KEY`.
    pub passthrough_names: Vec<String>,
    /// KEY=VALUE pairs written to the env file.
    pub env_file_entries: Vec<(String, String)>,
    /// JSON-encoded read root paths for the guard extension.
    pub read_roots_json: String,
    /// JSON-encoded write root paths for the guard extension.
    pub write_roots_json: String,
}

/// Podman security flags.
#[derive(Debug, Clone)]
pub struct SecurityConfig {
    pub userns: String,
    pub security_opts: Vec<String>,
    pub cap_drop: String,
    pub pids_limit: u32,
    pub pull: String,
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            userns: "keep-id".to_owned(),
            security_opts: vec!["no-new-privileges".to_owned(), "label=disable".to_owned()],
            cap_drop: "all".to_owned(),
            pids_limit: 4096,
            pull: "never".to_owned(),
        }
    }
}
