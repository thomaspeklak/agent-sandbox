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
sandbox_pi_dir = "{base}/sandbox"
host_pi_dir = "{base}/host"
host_claude_dir = "{base}/claude"
agent_sandbox_base = "{base}/agent-sandboxes"
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
        vec!["--no-extensions", "-e", "/home/dev/.pi/extensions/guard.ts"]
    );
}

#[test]
fn pi_profile_has_env() {
    let config = minimal_config();
    let profile = profile_for(Agent::Pi, &config);
    assert!(
        profile
            .extra_env
            .iter()
            .any(|(k, v)| k == "PI_CODING_AGENT_DIR" && v == "/home/dev/.pi")
    );
}

#[test]
fn pi_profile_has_sandbox_mount() {
    let config = minimal_config();
    let profile = profile_for(Agent::Pi, &config);
    assert_eq!(profile.extra_mounts.len(), 1);
    assert_eq!(profile.extra_mounts[0].container, "/home/dev/.pi");
    assert_eq!(
        profile.extra_mounts[0].host,
        config.sandbox.sandbox_dir_for(Agent::Pi)
    );
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
    assert_eq!(
        profile.command_args,
        vec![
            "--dangerously-skip-permissions",
            "--settings",
            "{\"sandbox\":{\"enabled\":false}}"
        ]
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
fn claude_profile_has_sandbox_mount() {
    let config = minimal_config();
    let profile = profile_for(Agent::Claude, &config);
    assert_eq!(profile.extra_mounts.len(), 1);
    assert_eq!(profile.extra_mounts[0].container, "/home/dev/.claude");
    // Should point to host_claude_dir for auth access
    let expected = config.sandbox.host_claude_dir.clone();
    assert_eq!(profile.extra_mounts[0].host, expected);
    assert_eq!(profile.extra_mounts[0].mode, ags::config::MountMode::Rw);
}

#[test]
fn claude_profile_has_optional_json_mount() {
    let config = minimal_config();
    let profile = profile_for(Agent::Claude, &config);
    assert_eq!(profile.optional_file_mounts.len(), 1);
    assert_eq!(
        profile.optional_file_mounts[0].container,
        "/home/dev/.claude.json"
    );
    // Should be sibling of host_claude_dir (e.g. ~/.claude -> ~/.claude.json)
    let expected = config
        .sandbox
        .host_claude_dir
        .parent()
        .unwrap()
        .join(".claude.json");
    assert_eq!(profile.optional_file_mounts[0].host, expected);
}

#[test]
fn claude_profile_no_extra_boot_dirs() {
    let config = minimal_config();
    let profile = profile_for(Agent::Claude, &config);
    assert!(profile.extra_boot_dirs.is_empty());
}

#[test]
fn claude_profile_no_browser_skill() {
    let config = minimal_config();
    let profile = profile_for(Agent::Claude, &config);
    assert!(profile.browser_skill_flag.is_none());
}

#[test]
fn codex_profile_has_sandbox_mount() {
    let config = minimal_config();
    let profile = profile_for(Agent::Codex, &config);
    assert_eq!(profile.command, "codex");
    assert!(profile.command_args.is_empty());
    assert!(profile.extra_env.is_empty());
    assert_eq!(profile.extra_mounts.len(), 1);
    assert_eq!(profile.extra_mounts[0].container, "/home/dev/.codex");
    assert!(profile.extra_boot_dirs.is_empty());
    assert!(profile.browser_skill_flag.is_none());
}

#[test]
fn gemini_profile_has_sandbox_mount() {
    let config = minimal_config();
    let profile = profile_for(Agent::Gemini, &config);
    assert_eq!(profile.command, "gemini");
    assert_eq!(profile.extra_mounts.len(), 1);
    assert_eq!(profile.extra_mounts[0].container, "/home/dev/.gemini");
    let expected = config.sandbox.sandbox_dir_for(Agent::Gemini);
    assert_eq!(profile.extra_mounts[0].host, expected);
    assert!(profile.extra_boot_dirs.is_empty());
}

#[test]
fn opencode_profile_has_sandbox_mount() {
    let config = minimal_config();
    let profile = profile_for(Agent::Opencode, &config);
    assert_eq!(profile.command, "opencode");
    assert_eq!(profile.extra_mounts.len(), 1);
    assert_eq!(
        profile.extra_mounts[0].container,
        "/home/dev/.config/opencode"
    );
    let expected = config.sandbox.sandbox_dir_for(Agent::Opencode);
    assert_eq!(profile.extra_mounts[0].host, expected);
    assert_eq!(
        profile.extra_boot_dirs,
        vec![
            "/home/dev/.local/share/opencode",
            "/home/dev/.cache/opencode"
        ]
    );
}

#[test]
fn sandbox_dir_for_all_agents_uses_base() {
    let config = minimal_config();
    let base = &config.sandbox.agent_sandbox_base;
    assert_eq!(config.sandbox.sandbox_dir_for(Agent::Pi), base.join("pi"));
    assert_eq!(
        config.sandbox.sandbox_dir_for(Agent::Claude),
        base.join("claude")
    );
    assert_eq!(
        config.sandbox.sandbox_dir_for(Agent::Codex),
        base.join("codex")
    );
    assert_eq!(
        config.sandbox.sandbox_dir_for(Agent::Gemini),
        base.join("gemini")
    );
    assert_eq!(
        config.sandbox.sandbox_dir_for(Agent::Opencode),
        base.join("opencode")
    );
}
