use std::fs;
use std::path::Path;

use ags::cmd::doctor;
use ags::config::{
    AuthProxyConfig, BrowserConfig, MountKind, MountMode, MountWhen, UpdateConfig, ValidatedConfig,
    ValidatedMount, ValidatedSandbox,
};

fn minimal_config(tmp: &Path) -> ValidatedConfig {
    let pi_root = tmp.join("pi");
    let pi_agent = pi_root.join("agent");
    fs::create_dir_all(pi_agent.join("extensions")).unwrap();
    fs::write(pi_agent.join("settings.json"), "{}").unwrap();
    fs::write(pi_agent.join("extensions/guard.ts"), "// guard").unwrap();

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
            cache_dir,
            gitconfig_path: gitconfig,
            auth_key,
            sign_key,
            bootstrap_files: vec![],
            container_boot_dirs: vec![],
            passthrough_env: vec![],
        },
        mounts: vec![ValidatedMount {
            host: pi_root,
            container: "/home/dev/.pi".into(),
            mode: MountMode::Rw,
            kind: MountKind::Dir,
            when: MountWhen::Always,
            create: false,
            optional: false,
            source: "agent_mount".into(),
        }],
        tools: vec![],
        secrets: vec![],
        browser: BrowserConfig::default(),
        update: UpdateConfig::default(),
        auth_proxy: AuthProxyConfig::default(),
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
    let pi_agent = config
        .mount_host_for_container("/home/dev/.pi")
        .unwrap()
        .join("agent");
    fs::remove_file(pi_agent.join("settings.json")).unwrap();
    let result = doctor::run(&config);
    assert!(!result);
}

#[test]
fn doctor_self_heals_missing_tmux_conf() {
    let tmp = tempfile::tempdir().unwrap();
    let config = minimal_config(tmp.path());
    let tmux_conf = config.sandbox.containerfile.with_file_name("tmux.conf");
    let result = doctor::run(&config);
    assert!(result);
    assert!(tmux_conf.exists());
}

#[test]
fn doctor_self_heals_missing_guard_extension() {
    let tmp = tempfile::tempdir().unwrap();
    let config = minimal_config(tmp.path());
    let pi_agent = config
        .mount_host_for_container("/home/dev/.pi")
        .unwrap()
        .join("agent");
    // Remove guard extension — doctor should recreate it from embedded asset
    fs::remove_file(pi_agent.join("extensions/guard.ts")).unwrap();
    let result = doctor::run(&config);
    assert!(result);
    assert!(pi_agent.join("extensions/guard.ts").exists());
}
