use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

pub const INPUT_SCHEMA: &str = include_str!("../../../../docs/schemas/prepare-input.schema.json");
pub const OUTPUT_SCHEMA: &str = include_str!("../../../../docs/schemas/prepare-output.schema.json");

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Context {
    pub version: u8,
    pub event: String,
    pub launch_id: String,
    pub project: PathBuf,
    pub workdir: PathBuf,
    pub agent: crate::cli::Agent,
}
impl Context {
    pub fn new(workdir: &Path, agent: crate::cli::Agent) -> Result<Self, String> {
        let workdir = workdir.canonicalize().map_err(|e| e.to_string())?;
        let project = crate::git::repo_root(&workdir).unwrap_or_else(|| workdir.clone());
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_nanos();
        Ok(Self {
            version: 1,
            event: "prepare".into(),
            launch_id: format!("ags-{}-{now}", std::process::id()),
            project,
            workdir,
            agent,
        })
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.version != 1
            || self.event != "prepare"
            || self.launch_id.is_empty()
            || !self.project.is_absolute()
            || !self.workdir.is_absolute()
        {
            return Err("context requires version 1, event prepare, nonempty launch_id and absolute project/workdir".into());
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(untagged)]
pub enum EnvValue {
    Literal(String),
    Secret(SecretRef),
}
#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct SecretRef {
    pub op: String,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GeneratedFile {
    pub destination: String,
    pub content: String,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct BindMount {
    pub source: PathBuf,
    pub destination: String,
    pub mode: Mode,
}
#[derive(Debug, Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Mode {
    Ro,
    Rw,
}
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Response {
    pub version: u8,
    #[serde(default)]
    pub env: BTreeMap<String, EnvValue>,
    #[serde(default)]
    pub files: Vec<GeneratedFile>,
    #[serde(default)]
    pub mounts: Vec<BindMount>,
}

pub fn validate_response(bytes: &[u8]) -> Result<Response, String> {
    if bytes.len() > 1024 * 1024 {
        return Err("hook stdout exceeded 1 MiB".into());
    }
    let response: Response = super::strict_json::parse(bytes)?;
    response.validate()?;
    Ok(response)
}
impl Response {
    pub fn validate(&self) -> Result<(), String> {
        if self.version != 1 {
            return Err("response version must be 1".into());
        }
        for (key, value) in &self.env {
            validate_env_name(key)?;
            match value {
                EnvValue::Literal(s) => validate_env_value(s)?,
                EnvValue::Secret(s) => crate::onepassword_refs::validate_reference(&s.op)?,
            }
        }
        for file in &self.files {
            validate_destination(&file.destination)?;
        }
        for mount in &self.mounts {
            validate_destination(&mount.destination)?;
            if !mount.source.is_absolute()
                || mount.source.to_string_lossy().contains(['\0', '\n', ':'])
            {
                return Err(
                    "hook mount source must be an absolute path without NUL, newline or ':'".into(),
                );
            }
        }
        Ok(())
    }
}

pub(crate) fn validate_env_name(key: &str) -> Result<(), String> {
    let valid = !key.is_empty()
        && key
            .bytes()
            .enumerate()
            .all(|(i, b)| b == b'_' || b.is_ascii_alphabetic() || (i > 0 && b.is_ascii_digit()));
    // Variables that affect AGS guards, wrappers, runtime ownership, host auth
    // or agent configuration discovery (even at otherwise valid data paths).
    let protected = [
        "HOME",
        "PATH",
        "BASH_ENV",
        "ENV",
        "SHELLOPTS",
        "BASHOPTS",
        "CDPATH",
        "IFS",
        "NODE_OPTIONS",
        "NODE_PATH",
        "PYTHONPATH",
        "PYTHONHOME",
        "RUSTUP_HOME",
        "CARGO_HOME",
        "GOPATH",
        "GOCACHE",
        "SCCACHE_DIR",
        "CACHEPOT_DIR",
        "SSH_AUTH_SOCK",
        "GIT_CONFIG_GLOBAL",
        "BROWSER",
        "GLIMPSE_BINARY_PATH",
        "DOCKER_HOST",
        "TESTCONTAINERS_HOST_OVERRIDE",
        "PSP_SESSION_ID",
        "DISABLE_AUTOUPDATER",
        "OPENCODE_DISABLE_AUTOUPDATE",
        "OPENCODE_CONFIG_CONTENT",
        "OPENCODE_CONFIG",
        "OPENCODE_CONFIG_DIR",
        "OPENCODE_TUI_CONFIG",
        "CODEX_HOME",
        "GEMINI_CLI_HOME",
        "GEMINI_CLI_SYSTEM_SETTINGS_PATH",
        "GEMINI_CLI_SYSTEM_DEFAULTS_PATH",
        "GEMINI_CLI_TRUSTED_FOLDERS_PATH",
        "PI_CODING_AGENT_DIR",
        "NPM_CONFIG_PREFIX",
        "WAYLAND_DISPLAY",
        "XDG_CONFIG_HOME",
        "XDG_RUNTIME_DIR",
        "XDG_SESSION_TYPE",
    ];
    if !valid
        || protected.contains(&key)
        || [
            "AGS_",
            "OP_",
            "PNPM_",
            "MISE_",
            "LD_",
            "DCG_",
            "PI_GUARD_",
            "GIT_CONFIG_",
            "CLAUDE_",
        ]
        .iter()
        .any(|p| key.starts_with(p))
    {
        return Err(format!(
            "hook environment key {key:?} is invalid or protected"
        ));
    }
    Ok(())
}
pub(crate) fn validate_env_value(value: &str) -> Result<(), String> {
    if value.contains(['\0', '\n', '\r']) {
        return Err("environment values must not contain NUL or newlines".into());
    }
    Ok(())
}
pub(crate) fn validate_destination(raw: &str) -> Result<(), String> {
    let path = Path::new(raw);
    if !path.is_absolute()
        || raw.contains(['\0', '\n', ':'])
        || raw.ends_with('/')
        || raw.contains("//")
        || path
            .components()
            .any(|c| !matches!(c, Component::RootDir | Component::Normal(_)))
        || raw.split('/').any(|s| s == "." || s == "..")
    {
        return Err("hook destination must be a normalized absolute path".into());
    }
    // Only work/data areas, never image executables, agent settings or AGS infrastructure.
    let allowed = ["/tmp", "/workspace", "/home/dev"]
        .iter()
        .any(|p| path.starts_with(p) && path != Path::new(p));
    let protected = [
        "/home/dev/.config",
        "/home/dev/.local",
        "/home/dev/.pi",
        "/home/dev/.claude",
        "/home/dev/.claude.json",
        "/home/dev/.gitconfig",
        "/home/dev/.npmrc",
        "/home/dev/.codex",
        "/home/dev/.gemini",
        "/home/dev/.opencode",
        "/home/dev/.cargo",
        "/home/dev/.cache",
        "/home/dev/.ssh",
        "/home/dev/.npm-global",
        "/home/dev/go",
        "/home/dev/.bashrc",
        "/home/dev/.bash_profile",
        "/home/dev/.profile",
    ];
    let hidden_home_setting = path
        .strip_prefix("/home/dev")
        .ok()
        .and_then(|p| p.components().next())
        .is_some_and(|c| c.as_os_str().to_string_lossy().starts_with('.'));
    let ags_temporary_resource = path
        .strip_prefix("/tmp")
        .ok()
        .and_then(|p| p.components().next())
        .is_some_and(|c| c.as_os_str().to_string_lossy().starts_with("ags-"));
    if !allowed
        || hidden_home_setting
        || ags_temporary_resource
        || protected.iter().any(|p| overlaps(path, Path::new(p)))
    {
        return Err(format!(
            "hook destination {raw:?} is protected (use /workspace, /tmp or unreserved /home/dev data paths)"
        ));
    }
    Ok(())
}
pub(crate) fn overlaps(a: &Path, b: &Path) -> bool {
    a.starts_with(b) || b.starts_with(a)
}
