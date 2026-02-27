use std::fs;
use std::path::Path;

use ags::cmd::doctor;
use ags::config::{BrowserConfig, UpdateConfig, ValidatedConfig, ValidatedSandbox};

fn minimal_config(tmp: &Path) -> ValidatedConfig {
    let sandbox_pi_dir = tmp.join("pi-agent");
    fs::create_dir_all(sandbox_pi_dir.join("extensions")).unwrap();
    fs::write(sandbox_pi_dir.join("settings.json"), "{}").unwrap();
    fs::write(sandbox_pi_dir.join("extensions/guard.ts"), "// guard").unwrap();

    let containerfile = tmp.join("Containerfile");
    fs::write(&containerfile, "FROM scratch").unwrap();

    let gitconfig = tmp.join("gitconfig");
    let auth_key = tmp.join("auth-key");
    let sign_key = tmp.join("sign-key");
    let cache_dir = tmp.join("cache");
    fs::create_dir_all(&cache_dir).unwrap();

    ValidatedConfig {
        config_file: tmp.join("config.toml"),
        sandbox: ValidatedSandbox {
            image: "test-image:latest".into(),
            containerfile,
            sandbox_pi_dir,
            host_pi_dir: tmp.join("host-pi"),
            host_claude_dir: tmp.join("host-claude"),
            cache_dir,
            gitconfig_path: gitconfig,
            auth_key,
            sign_key,
            bootstrap_files: vec![],
            container_boot_dirs: vec![],
            passthrough_env: vec![],
        },
        mounts: vec![],
        tools: vec![],
        secrets: vec![],
        browser: BrowserConfig::default(),
        update: UpdateConfig::default(),
    }
}

#[test]
fn doctor_runs_without_panic_on_minimal_config() {
    let tmp = tempfile::tempdir().unwrap();
    let config = minimal_config(tmp.path());
    // doctor returns bool (pass/fail) — just ensure it doesn't panic
    let _result = doctor::run(&config);
}

#[test]
fn doctor_detects_missing_containerfile() {
    let tmp = tempfile::tempdir().unwrap();
    let config = minimal_config(tmp.path());
    // Remove the containerfile
    fs::remove_file(&config.sandbox.containerfile).unwrap();
    let result = doctor::run(&config);
    // Should have at least one failure (missing Containerfile)
    assert!(!result);
}

#[test]
fn doctor_detects_missing_settings() {
    let tmp = tempfile::tempdir().unwrap();
    let config = minimal_config(tmp.path());
    fs::remove_file(config.sandbox.sandbox_pi_dir.join("settings.json")).unwrap();
    let result = doctor::run(&config);
    assert!(!result);
}

#[test]
fn doctor_detects_missing_guard_extension() {
    let tmp = tempfile::tempdir().unwrap();
    let config = minimal_config(tmp.path());
    fs::remove_file(config.sandbox.sandbox_pi_dir.join("extensions/guard.ts")).unwrap();
    let result = doctor::run(&config);
    assert!(!result);
}
