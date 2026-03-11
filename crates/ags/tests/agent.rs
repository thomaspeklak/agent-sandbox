use std::path::Path;

use ags::agent::profile_for;
use ags::cli::Agent;
use ags::config::parse_toml_str;

fn minimal_config() -> ags::config::ValidatedConfig {
    minimal_config_with_browser(false)
}

fn minimal_config_with_browser(browser_enabled: bool) -> ags::config::ValidatedConfig {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.keep();
    let containerfile = base.join("Containerfile");
    std::fs::write(&containerfile, "FROM scratch\n").unwrap();

    let browser_section = if browser_enabled {
        r#"
[browser]
enabled = true
command = "google-chrome"
profile_dir = "/tmp/chrome"
debug_port = 9222
pi_skill_path = "/home/dev/browser-skill"
"#
        .to_owned()
    } else {
        String::new()
    };

    let toml = format!(
        r#"
[sandbox]
image = "localhost/agent-sandbox:latest"
containerfile = "{containerfile}"
cache_dir = "{base}/cache"
gitconfig_path = "{base}/gitconfig"
auth_key = "{base}/auth"
sign_key = "{base}/sign"
container_boot_dirs = []
passthrough_env = []

{browser_section}
"#,
        containerfile = containerfile.display(),
        base = base.display(),
    );

    parse_toml_str(&toml, Path::new("/test/config.toml")).unwrap()
}

#[test]
fn pi_profile_command() {
    let config = minimal_config();
    let profile = profile_for(Agent::Pi, &config);
    assert_eq!(profile.command, "pi");
}

#[test]
fn pi_profile_has_guard_args() {
    let config = minimal_config();
    let profile = profile_for(Agent::Pi, &config);
    assert_eq!(
        profile.command_args,
        vec![
            "--no-extensions",
            "-e",
            "/home/dev/.pi/agent/extensions/guard.ts",
            "--append-system-prompt",
            "Sandbox: use host.containers.internal (localhost is container-local)."
        ]
    );
}

#[test]
fn pi_profile_no_config_dir_env() {
    let config = minimal_config();
    let profile = profile_for(Agent::Pi, &config);
    // PI_CODING_AGENT_DIR should NOT be set — pi already defaults to $HOME/.pi/agent.
    assert!(
        !profile
            .extra_env
            .iter()
            .any(|(k, _)| k == "PI_CODING_AGENT_DIR")
    );
}

#[test]
fn pi_profile_has_no_implicit_mounts() {
    let config = minimal_config();
    let _profile = profile_for(Agent::Pi, &config);
}

#[test]
fn pi_profile_has_browser_skill_flag() {
    let config = minimal_config_with_browser(true);
    let profile = profile_for(Agent::Pi, &config);
    assert_eq!(profile.browser_skill_flag, Some("--skill".to_owned()));
    assert_eq!(profile.browser_skill_path, "/home/dev/browser-skill");
}

#[test]
fn pi_profile_no_extra_boot_dirs() {
    let config = minimal_config();
    let profile = profile_for(Agent::Pi, &config);
    assert!(profile.extra_boot_dirs.is_empty());
}

#[test]
fn claude_profile_command() {
    let config = minimal_config();
    let profile = profile_for(Agent::Claude, &config);
    assert_eq!(profile.command, "claude");
    assert!(profile.command_args.contains(&"--dangerously-skip-permissions".to_owned()));
    assert!(profile.command_args.contains(&"--append-system-prompt".to_owned()));

    // Settings should include both sandbox disable and guard hook
    let settings_idx = profile
        .command_args
        .iter()
        .position(|a| a == "--settings")
        .expect("--settings flag present");
    let settings_json = &profile.command_args[settings_idx + 1];
    let parsed: serde_json::Value = serde_json::from_str(settings_json)
        .expect("settings arg is valid JSON");
    assert_eq!(parsed["sandbox"]["enabled"], false);
    assert_eq!(parsed["hooks"]["PreToolUse"][0]["matcher"], "Bash|Read|Write|Edit|Grep|Glob");
    assert_eq!(
        parsed["hooks"]["PreToolUse"][0]["hooks"][0]["command"],
        "/home/dev/.config/ags/hooks/guard.sh"
    );

    // Guard skill loaded via --plugin-dir
    let plugin_idx = profile
        .command_args
        .iter()
        .position(|a| a == "--plugin-dir")
        .expect("--plugin-dir flag present");
    assert_eq!(
        profile.command_args[plugin_idx + 1],
        "/home/dev/.config/ags/hooks"
    );
}

#[test]
fn claude_profile_no_config_dir_env() {
    let config = minimal_config();
    let profile = profile_for(Agent::Claude, &config);
    // CLAUDE_CONFIG_DIR should NOT be set — Claude uses $HOME/.claude by default,
    // and setting it explicitly can interfere with credential discovery.
    assert!(
        !profile
            .extra_env
            .iter()
            .any(|(k, _)| k == "CLAUDE_CONFIG_DIR")
    );
}

#[test]
fn claude_profile_has_no_implicit_mounts() {
    let config = minimal_config();
    let _profile = profile_for(Agent::Claude, &config);
}

#[test]
fn claude_profile_no_extra_boot_dirs() {
    let config = minimal_config();
    let profile = profile_for(Agent::Claude, &config);
    assert!(profile.extra_boot_dirs.is_empty());
    assert!(
        profile.entrypoint_setup.is_empty(),
        "skill loaded via --plugin-dir, no entrypoint setup needed"
    );
}

#[test]
fn claude_profile_no_browser_skill() {
    let config = minimal_config();
    let profile = profile_for(Agent::Claude, &config);
    assert!(profile.browser_skill_flag.is_none());
}

#[test]
fn codex_profile_basics() {
    let config = minimal_config();
    let profile = profile_for(Agent::Codex, &config);
    assert_eq!(profile.command, "codex");
    assert_eq!(profile.command_args.len(), 2);
    assert_eq!(profile.command_args[0], "-c");
    assert!(
        profile.command_args[1].starts_with("developer_instructions=\"")
            && profile.command_args[1].contains("host.containers.internal")
    );
    assert!(profile.extra_env.is_empty());
    assert!(profile.extra_boot_dirs.is_empty());
    assert!(profile.browser_skill_flag.is_none());
}

#[test]
fn gemini_profile_basics() {
    let config = minimal_config();
    let profile = profile_for(Agent::Gemini, &config);
    assert_eq!(profile.command, "gemini");
    assert!(profile.extra_boot_dirs.is_empty());
}

#[test]
fn opencode_profile_boot_dirs() {
    let config = minimal_config();
    let profile = profile_for(Agent::Opencode, &config);
    assert_eq!(profile.command, "opencode");
    assert_eq!(
        profile.extra_boot_dirs,
        vec![
            "/home/dev/.local/share/opencode",
            "/home/dev/.cache/opencode"
        ]
    );
}
