use std::io::Cursor;

use tempfile::TempDir;

use super::{
    ConfigBootstrap, accepts_create_default, ensure_global_config_exists_with_io, parse_document,
    recover_broken_config_with_io,
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

    let bootstrap = ensure_global_config_exists_with_io(&path, &mut input, &mut output).unwrap();

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
        recover_broken_config_with_io(&path, "parse error", true, &mut input, &mut output).unwrap();

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
