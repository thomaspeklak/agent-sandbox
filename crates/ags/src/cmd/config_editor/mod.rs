pub mod agents;
mod discovery;
pub mod model;
pub mod schema;
mod ui;

use std::fs;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::create_default_config;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConfigBootstrap {
    AlreadyPresent,
    Created,
    Declined,
}

/// Entry point for `ags config`.
pub fn run(config_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let bootstrap = ensure_global_config_exists(config_path)?;
    if bootstrap == ConfigBootstrap::Declined {
        return Ok(());
    }

    if !ensure_configs_parse(config_path)? {
        return Ok(());
    }

    let mut app = ui::App::new(config_path)?;
    if bootstrap == ConfigBootstrap::Created {
        app.set_info_status(
            "Created starter config. Review [sandbox].auth_key and sign_key, then press Ctrl-S.",
        );
    }
    app.run()
}

fn ensure_global_config_exists(
    config_path: &Path,
) -> Result<ConfigBootstrap, Box<dyn std::error::Error>> {
    let stdin = io::stdin();
    let mut input = stdin.lock();
    let stderr = io::stderr();
    let mut output = stderr.lock();
    ensure_global_config_exists_with_io(config_path, &mut input, &mut output)
}

fn ensure_global_config_exists_with_io<R: BufRead, W: Write>(
    config_path: &Path,
    input: &mut R,
    output: &mut W,
) -> Result<ConfigBootstrap, Box<dyn std::error::Error>> {
    if config_path.exists() {
        return Ok(ConfigBootstrap::AlreadyPresent);
    }

    writeln!(output, "[ags] No config found at {}", config_path.display())?;
    write!(output, "Create a starter config now? [Y/n] ")?;
    output.flush()?;

    let mut response = String::new();
    input.read_line(&mut response)?;

    if !accepts_create_default(&response) {
        writeln!(
            output,
            "[ags] No config created. See config/config.example.toml for a starting point."
        )?;
        return Ok(ConfigBootstrap::Declined);
    }

    create_default_config(config_path)?;
    writeln!(
        output,
        "[ags] Created starter config at {}",
        config_path.display()
    )?;
    Ok(ConfigBootstrap::Created)
}

fn accepts_create_default(response: &str) -> bool {
    matches!(
        response.trim().to_ascii_lowercase().as_str(),
        "" | "y" | "yes"
    )
}

fn ensure_configs_parse(config_path: &Path) -> Result<bool, Box<dyn std::error::Error>> {
    let cwd = std::env::current_dir().ok();
    let (local_path, repo_local_available) = resolve_local_target_path(cwd.as_deref());

    loop {
        if let Err(error) = parse_document(config_path) {
            if !recover_broken_config(config_path, &error, true)? {
                return Ok(false);
            }
            continue;
        }

        if repo_local_available
            && local_path.exists()
            && let Err(error) = parse_document(&local_path)
        {
            if !recover_broken_config(&local_path, &error, false)? {
                return Ok(false);
            }
            continue;
        }

        return Ok(true);
    }
}

fn resolve_local_target_path(cwd: Option<&Path>) -> (PathBuf, bool) {
    let Some(cwd) = cwd else {
        return (PathBuf::from(".ags/config.toml"), false);
    };

    match crate::git::repo_root(cwd) {
        Some(root) => (root.join(".ags/config.toml"), true),
        None => (cwd.join(".ags/config.toml"), false),
    }
}

fn parse_document(path: &Path) -> Result<(), String> {
    let content = fs::read_to_string(path)
        .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    content
        .parse::<toml_edit::DocumentMut>()
        .map(|_| ())
        .map_err(|error| format!("{}\n\n{}", path.display(), error))
}

fn recover_broken_config(
    path: &Path,
    error: &str,
    is_global: bool,
) -> Result<bool, Box<dyn std::error::Error>> {
    let stdin = io::stdin();
    let mut input = stdin.lock();
    let stderr = io::stderr();
    let mut output = stderr.lock();
    recover_broken_config_with_io(path, error, is_global, &mut input, &mut output)
}

fn recover_broken_config_with_io<R: BufRead, W: Write>(
    path: &Path,
    error: &str,
    is_global: bool,
    input: &mut R,
    output: &mut W,
) -> Result<bool, Box<dyn std::error::Error>> {
    loop {
        let backup_exists = path.with_extension("toml.bak").exists();
        writeln!(output, "[ags] Failed to parse {}", path.display())?;
        writeln!(output, "{error}")?;
        write!(output, "Recovery: [E]dit")?;
        if backup_exists {
            write!(output, "  [R]estore backup")?;
        }
        if is_global {
            write!(output, "  [D]efault config")?;
        } else {
            write!(output, "  [D]elete and recreate empty local overlay")?;
        }
        writeln!(output, "  [Q]uit")?;
        write!(output, "Choose recovery action: ")?;
        output.flush()?;

        let mut response = String::new();
        input.read_line(&mut response)?;
        let choice = response.trim().to_ascii_lowercase();

        match choice.as_str() {
            "e" | "edit" => {
                open_in_editor(path)?;
            }
            "r" | "restore" if backup_exists => {
                fs::copy(path.with_extension("toml.bak"), path)?;
                writeln!(output, "[ags] Restored backup for {}", path.display())?;
                return Ok(true);
            }
            "d" | "default" | "delete" => {
                if is_global {
                    create_default_config(path)?;
                    writeln!(
                        output,
                        "[ags] Recreated default config at {}",
                        path.display()
                    )?;
                } else {
                    if let Some(parent) = path.parent() {
                        fs::create_dir_all(parent)?;
                    }
                    fs::write(path, "")?;
                    writeln!(
                        output,
                        "[ags] Recreated empty local overlay at {}",
                        path.display()
                    )?;
                }
                return Ok(true);
            }
            "q" | "quit" => return Ok(false),
            _ => {
                writeln!(output, "[ags] Unknown choice. Please pick E, R, D, or Q.")?;
            }
        }
    }
}

fn open_in_editor(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let editor = std::env::var("EDITOR").unwrap_or_else(|_| "vi".to_string());
    let status = Command::new(&editor).arg(path).status()?;
    if !status.success() {
        return Err(format!("editor exited with status {status}").into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use tempfile::TempDir;

    use super::{
        ConfigBootstrap, accepts_create_default, ensure_global_config_exists_with_io,
        parse_document, recover_broken_config_with_io,
    };
    use crate::config::DEFAULT_CONFIG;

    #[test]
    fn accepts_default_yes_responses() {
        for response in ["", "y", "Y", "yes", "YES", " yes "] {
            assert!(accepts_create_default(response), "response={response:?}");
        }
    }

    #[test]
    fn rejects_non_yes_responses() {
        for response in ["n", "no", "nah", "0"] {
            assert!(!accepts_create_default(response), "response={response:?}");
        }
    }

    #[test]
    fn first_run_creates_default_config_when_accepted() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        let mut input = Cursor::new(b"\n");
        let mut output = Vec::new();

        let created = ensure_global_config_exists_with_io(&path, &mut input, &mut output).unwrap();

        assert_eq!(created, ConfigBootstrap::Created);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), DEFAULT_CONFIG);
    }

    #[test]
    fn first_run_does_not_create_config_when_declined() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        let mut input = Cursor::new(b"n\n");
        let mut output = Vec::new();

        let created = ensure_global_config_exists_with_io(&path, &mut input, &mut output).unwrap();

        assert_eq!(created, ConfigBootstrap::Declined);
        assert!(!path.exists());
    }

    #[test]
    fn existing_config_skips_bootstrap_prompt() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, DEFAULT_CONFIG).unwrap();
        let mut input = Cursor::new(b"n\n");
        let mut output = Vec::new();

        let bootstrap =
            ensure_global_config_exists_with_io(&path, &mut input, &mut output).unwrap();

        assert_eq!(bootstrap, ConfigBootstrap::AlreadyPresent);
    }

    #[test]
    fn parse_document_reports_path_on_toml_error() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("broken.toml");
        std::fs::write(&path, "[sandbox\nimage = 'broken'").unwrap();

        let error = parse_document(&path).unwrap_err();

        assert!(error.contains(path.to_string_lossy().as_ref()));
    }

    #[test]
    fn broken_global_recovery_can_restore_backup() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, "broken").unwrap();
        std::fs::write(path.with_extension("toml.bak"), DEFAULT_CONFIG).unwrap();
        let mut input = Cursor::new(b"r\n");
        let mut output = Vec::new();

        let recovered =
            recover_broken_config_with_io(&path, "parse error", true, &mut input, &mut output)
                .unwrap();

        assert!(recovered);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), DEFAULT_CONFIG);
    }

    #[test]
    fn broken_local_recovery_can_recreate_empty_overlay() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join(".ags/config.toml");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "broken").unwrap();
        let mut input = Cursor::new(b"d\n");
        let mut output = Vec::new();

        let recovered =
            recover_broken_config_with_io(&path, "parse error", false, &mut input, &mut output)
                .unwrap();

        assert!(recovered);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "");
    }
}
