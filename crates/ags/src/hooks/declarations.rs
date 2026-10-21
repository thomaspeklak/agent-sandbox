use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Hook {
    pub name: String,
    pub executable: PathBuf,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u64,
    #[serde(skip)]
    pub project: Option<PathBuf>,
}
fn default_timeout() -> u64 {
    30
}

pub fn parse_declarations(
    value: &toml::Value,
    config: &Path,
    project: Option<&Path>,
) -> Result<Vec<Hook>, String> {
    let Some(value) = value.get("prepare_hook") else {
        return Ok(Vec::new());
    };
    // Infer scope from an absolute config path: `.ags/config.toml` otherwise
    // has an empty relative project parent, which cannot be canonicalized.
    let config = std::path::absolute(config).map_err(|e| e.to_string())?;
    let implicit_project = config
        .parent()
        .filter(|p| p.file_name().is_some_and(|n| n == ".ags"))
        .and_then(Path::parent);
    let project = project.or(implicit_project);
    let mut hooks: Vec<Hook> = value
        .clone()
        .try_into()
        .map_err(|e| format!("prepare_hook: {e}"))?;
    if hooks.len() > 64 {
        return Err("at most 64 prepare hooks per config are supported".into());
    }
    for hook in &mut hooks {
        if hook.name.is_empty()
            || !hook
                .name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        {
            return Err(
                "prepare hook name must contain only ASCII letters, digits, '-' or '_'".into(),
            );
        }
        if !(1..=300).contains(&hook.timeout_seconds) {
            return Err("hook timeout_seconds must be 1..300".into());
        }
        if hook.args.iter().any(|a| a.contains('\0')) {
            return Err("hook args must not contain NUL".into());
        }
        if !hook.executable.is_absolute() {
            hook.executable = config
                .parent()
                .unwrap_or(Path::new("."))
                .join(&hook.executable);
        }
        hook.executable = std::path::absolute(&hook.executable).map_err(|e| e.to_string())?;
        hook.project = project
            .map(|p| p.canonicalize().map_err(|e| e.to_string()))
            .transpose()?;
    }
    Ok(hooks)
}
