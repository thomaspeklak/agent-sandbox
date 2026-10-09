use crate::config::{self, ValidatedConfig};
use crate::trust::StdioRepoConfigPrompter;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

pub fn load_config(override_path: Option<&Path>) -> Result<ValidatedConfig, ExitCode> {
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

    let repo_local_config = resolve_repo_local_config(&config_path);
    if repo_local_config.is_none()
        && let Ok(cwd) = std::env::current_dir()
        && let Err(error) = crate::trust::refuse_unloaded_hook_overlay(&cwd, &config_path)
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
    std::env::current_dir().ok().and_then(|cwd| {
        match crate::trust::resolve_repo_local_overlay(
            &cwd,
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
    })
}
