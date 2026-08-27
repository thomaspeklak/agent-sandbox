use std::process::Command;

#[test]
fn disabled_agent_is_rejected_before_launch_side_effects() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("owner's config.toml");
    let containerfile = dir.path().join("Containerfile");
    std::fs::write(
        &config,
        format!(
            r#"[sandbox]
image = "localhost/agent-sandbox:test"
containerfile = "{}"
cache_dir = "{}"
gitconfig_path = "{}"
auth_key = "{}"
sign_key = "{}"
enabled_agents = []
"#,
            containerfile.display(),
            dir.path().join("cache").display(),
            dir.path().join("gitconfig").display(),
            dir.path().join("auth").display(),
            dir.path().join("sign").display(),
        ),
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_ags"))
        .current_dir(dir.path())
        .args(["--agent", "pi", "--config", config.to_str().unwrap()])
        .output()
        .unwrap();

    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("agent 'pi' is disabled"), "got: {stderr}");
    assert!(
        stderr.contains(&config.display().to_string()),
        "got: {stderr}"
    );
    assert!(
        stderr.contains(&format!(
            "ags update-agents --config {}",
            ags::util::shell_quote(&config.display().to_string())
        )),
        "got: {stderr}"
    );
    assert!(!stderr.contains("`ags tools`"), "got: {stderr}");
    assert!(!containerfile.exists());
}

#[test]
fn shell_only_launch_does_not_write_pi_assets() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.toml");
    let runtime_blocker = dir.path().join("runtime-blocker");
    let pi_home = dir.path().join("pi-home");
    std::fs::write(&runtime_blocker, "not a directory").unwrap();
    std::fs::write(
        &config,
        format!(
            r#"[sandbox]
image = "localhost/agent-sandbox:test"
containerfile = "{}"
cache_dir = "{}"
gitconfig_path = "{}"
auth_key = "{}"
sign_key = "{}"
enabled_agents = []

[[agent_mount]]
host = "{}"
container = "/home/dev/.pi"
"#,
            dir.path().join("Containerfile").display(),
            dir.path().join("cache").display(),
            dir.path().join("gitconfig").display(),
            dir.path().join("auth").display(),
            dir.path().join("sign").display(),
            pi_home.display(),
        ),
    )
    .unwrap();

    let output = Command::new(env!("CARGO_BIN_EXE_ags"))
        .current_dir(dir.path())
        .env("XDG_RUNTIME_DIR", &runtime_blocker)
        .env("PATH", "")
        .args(["--agent", "shell", "--config", config.to_str().unwrap()])
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(
        !pi_home.exists(),
        "shell-only launch wrote disabled Pi assets"
    );
}
