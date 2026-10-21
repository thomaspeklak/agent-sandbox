use crate::config::{self, ValidatedConfig};
use crate::trust::StdioRepoConfigPrompter;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

pub fn load_config(override_path: Option<&Path>) -> Result<ValidatedConfig, ExitCode> {
    load_config_for_workdir(override_path, None)
}

/// Resolve overlay trust and fail-closed hook checks from the selected working directory.
pub(crate) fn load_config_for_workdir(
    override_path: Option<&Path>,
    workdir: Option<&Path>,
) -> Result<ValidatedConfig, ExitCode> {
    let cwd = workdir
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok());
    let config_path = override_path
        .map(PathBuf::from)
        .unwrap_or_else(crate::config::default_config_path);

    if !config_path.exists() {
        if let Err(e) = crate::config::create_default_config(&config_path) {
            eprintln!("error: could not create default config: {e}");
            return Err(ExitCode::from(2));
        }
        eprintln!("Created default config: {}", config_path.display());
    }

    let repo_local_config = cwd
        .as_deref()
        .and_then(|cwd| resolve_repo_local_config_at(&config_path, cwd));
    if repo_local_config.is_none()
        && let Some(cwd) = cwd.as_deref()
        && let Err(error) = crate::trust::refuse_unloaded_hook_overlay(cwd, &config_path)
    {
        eprintln!("error: {error}");
        return Err(ExitCode::from(2));
    }

    config::parse_and_validate_with_overlay(&config_path, repo_local_config.as_deref()).map_err(
        |e| {
            eprintln!("error: {e}");
            ExitCode::from(2)
        },
    )
}

pub fn resolve_repo_local_config(config_path: &Path) -> Option<PathBuf> {
    std::env::current_dir()
        .ok()
        .and_then(|cwd| resolve_repo_local_config_at(config_path, &cwd))
}

fn resolve_repo_local_config_at(config_path: &Path, cwd: &Path) -> Option<PathBuf> {
    match crate::trust::resolve_repo_local_overlay(
        cwd,
        config_path,
        &crate::trust::default_trust_store_path(),
        &StdioRepoConfigPrompter,
    ) {
        Ok(path) => path,
        Err(err) => {
            eprintln!("warning: could not load repo trust state: {err}");
            None
        }
    }
}
