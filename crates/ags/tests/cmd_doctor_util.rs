use std::fs;
use std::path::Path;

use ags::cli::Agent;
use ags::cmd::doctor;
use ags::config::{BrowserConfig, UpdateConfig, ValidatedConfig, ValidatedSandbox};

fn minimal_config(tmp: &Path) -> ValidatedConfig {
    let agent_sandbox_base = tmp.join("agent-sandboxes");
    // Pi sandbox lives at agent_sandbox_base/pi
    let pi_sandbox = agent_sandbox_base.join("pi");
    fs::create_dir_all(pi_sandbox.join("extensions")).unwrap();
    fs::write(pi_sandbox.join("settings.json"), "{}").unwrap();
    fs::write(pi_sandbox.join("extensions/guard.ts"), "// guard").unwrap();

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
            sandbox_pi_dir: pi_sandbox,
            host_pi_dir: tmp.join("host-pi"),
            host_claude_dir: tmp.join("host-claude"),
            agent_sandbox_base,
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
fn doctor_self_heals_missing_containerfile() {
    let tmp = tempfile::tempdir().unwrap();
    let config = minimal_config(tmp.path());
    // Remove the containerfile — doctor should recreate it from embedded asset
    fs::remove_file(&config.sandbox.containerfile).unwrap();
    let result = doctor::run(&config);
    assert!(result);
    assert!(config.sandbox.containerfile.exists());
}

#[test]
fn doctor_detects_missing_settings() {
    let tmp = tempfile::tempdir().unwrap();
    let config = minimal_config(tmp.path());
    let pi_sandbox = config.sandbox.sandbox_dir_for(Agent::Pi);
    fs::remove_file(pi_sandbox.join("settings.json")).unwrap();
    let result = doctor::run(&config);
    assert!(!result);
}

#[test]
fn doctor_self_heals_missing_guard_extension() {
    let tmp = tempfile::tempdir().unwrap();
    let config = minimal_config(tmp.path());
    let pi_sandbox = config.sandbox.sandbox_dir_for(Agent::Pi);
    // Remove guard extension — doctor should recreate it from embedded asset
    fs::remove_file(pi_sandbox.join("extensions/guard.ts")).unwrap();
    let result = doctor::run(&config);
    assert!(result);
    assert!(pi_sandbox.join("extensions/guard.ts").exists());
}
