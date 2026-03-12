use std::collections::HashMap;
use std::fs;
use std::path::Path;

use ags::cli::Agent;
use ags::config::{MountMode, parse_toml_str};
use ags::plan::{PlanError, build_launch_plan};

fn minimal_config_toml() -> String {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.keep();
    // Create required paths that the plan builder will canonicalize/check
    let containerfile = base.join("Containerfile");
    fs::write(&containerfile, "FROM scratch\n").unwrap();
    fs::create_dir_all(base.join("pi")).unwrap();
    fs::create_dir_all(base.join("claude")).unwrap();
    fs::write(base.join(".claude.json"), "{}\n").unwrap();
    fs::create_dir_all(base.join("codex")).unwrap();
    fs::create_dir_all(base.join("gemini")).unwrap();
    fs::create_dir_all(base.join("opencode")).unwrap();

    format!(
        r#"
[sandbox]
image = "localhost/agent-sandbox:latest"
containerfile = "{containerfile}"
cache_dir = "{base}/cache"
gitconfig_path = "{base}/gitconfig"
auth_key = "{base}/auth"
sign_key = "{base}/sign"
container_boot_dirs = ["/home/dev/.ssh", "/home/dev/.cache/kno"]
passthrough_env = ["ANTHROPIC_API_KEY"]

[[agent_mount]]
host = "{base}/.claude.json"
container = "/home/dev/.claude.json"
kind = "file"

[[agent_mount]]
host = "{base}/claude"
container = "/home/dev/.claude"

[[agent_mount]]
host = "{base}/codex"
container = "/home/dev/.codex"

[[agent_mount]]
host = "{base}/pi"
container = "/home/dev/.pi"

[[agent_mount]]
host = "{base}/opencode"
container = "/home/dev/.config/opencode"

[[agent_mount]]
host = "{base}/gemini"
container = "/home/dev/.gemini"
"#,
        containerfile = containerfile.display(),
        base = base.display(),
    )
}

fn build_plan_from(toml: &str, workdir: &Path) -> ags::plan::LaunchPlan {
    build_plan_from_agent(toml, workdir, Agent::Pi)
}

fn build_plan_from_agent(toml: &str, workdir: &Path, agent: Agent) -> ags::plan::LaunchPlan {
    let config = parse_toml_str(toml, Path::new("/test/config.toml")).unwrap();
    let secrets = HashMap::new();
    build_launch_plan(&config, workdir, agent, false, false, None, &secrets, None).unwrap()
}

#[test]
fn minimal_plan_has_correct_image() {
    let toml = minimal_config_toml();
    let workdir = tempfile::tempdir().unwrap();
    let plan = build_plan_from(&toml, workdir.path());
    assert_eq!(plan.image, "localhost/agent-sandbox:latest");
}

#[test]
fn container_name_has_expected_format() {
    let toml = minimal_config_toml();
    let workdir = tempfile::tempdir().unwrap();
    let plan = build_plan_from(&toml, workdir.path());

    assert!(plan.container_name.starts_with("ags-"));
    let parts: Vec<&str> = plan.container_name.split('-').collect();
    assert!(parts.len() >= 3, "name should have prefix, path, and id");
    let id = parts.last().unwrap();
    assert_eq!(id.len(), 4);
    assert!(id.chars().all(|c| c.is_ascii_hexdigit()));
}

#[test]
fn workdir_is_first_mount() {
    let toml = minimal_config_toml();
    let workdir = tempfile::tempdir().unwrap();
    let plan = build_plan_from(&toml, workdir.path());

    assert!(!plan.mounts.is_empty());
    let first = &plan.mounts[0];
    assert_eq!(first.mode, MountMode::Rw);
    // Host should be canonicalized
    assert!(first.host.is_absolute());
}

#[test]
fn infrastructure_mounts_present() {
    let toml = minimal_config_toml();
    let workdir = tempfile::tempdir().unwrap();
    let plan = build_plan_from(&toml, workdir.path());

    // Should have sandbox pi dir mount
    let pi_mount = plan.mounts.iter().find(|m| m.container == "/home/dev/.pi");
    assert!(pi_mount.is_some());

    // Should have gitconfig mount
    let gc_mount = plan
        .mounts
        .iter()
        .find(|m| m.container == "/home/dev/.config/ags/gitconfig");
    assert!(gc_mount.is_some());
    assert_eq!(gc_mount.unwrap().mode, MountMode::Ro);
}

#[test]
fn cache_mounts_created() {
    let toml = minimal_config_toml();
    let workdir = tempfile::tempdir().unwrap();
    let plan = build_plan_from(&toml, workdir.path());

    let cache_containers: Vec<&str> = vec![
        "/usr/local/pnpm",
        "/opt/claude-home",
        "/home/dev/.cargo",
        "/home/dev/go",
        "/home/dev/.cache/go-build",
        "/home/dev/.cache/sccache",
        "/home/dev/.cache/cachepot",
    ];
    for container in cache_containers {
        let found = plan.mounts.iter().any(|m| m.container == container);
        assert!(found, "missing cache mount: {container}");
    }
}

#[test]
fn env_has_required_inline_vars() {
    let toml = minimal_config_toml();
    let workdir = tempfile::tempdir().unwrap();
    let plan = build_plan_from(&toml, workdir.path());

    let find_env = |key: &str| -> Option<String> {
        plan.env
            .inline
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
    };

    assert_eq!(find_env("HOME"), Some("/home/dev".to_owned()));
    assert!(
        find_env("PI_CODING_AGENT_DIR").is_none(),
        "pi should not set PI_CODING_AGENT_DIR (uses $HOME/.pi/agent by default)"
    );
    assert_eq!(find_env("SSH_AUTH_SOCK"), Some("/ssh-agent".to_owned()));
    assert_eq!(find_env("AGS_SANDBOX"), Some("1".to_owned()));
    assert_eq!(
        find_env("AGS_HOST_SERVICES_HOST"),
        Some("host.containers.internal".to_owned())
    );
    assert!(
        find_env("AGS_HOST_SERVICES_HINT")
            .is_some_and(|v| v.contains("localhost is container-local"))
    );
    assert!(find_env("PNPM_HOME").is_some());
    assert!(find_env("CARGO_HOME").is_some());
}

#[test]
fn empty_env_var_not_emitted() {
    let toml = minimal_config_toml();
    let workdir = tempfile::tempdir().unwrap();
    let plan = build_plan_from(&toml, workdir.path());

    let has_empty_key = plan.env.inline.iter().any(|(k, _)| k.is_empty());
    assert!(!has_empty_key, "empty env var key should not be emitted");
}

#[test]
fn env_passthrough_names() {
    let toml = minimal_config_toml();
    let workdir = tempfile::tempdir().unwrap();
    let plan = build_plan_from(&toml, workdir.path());

    assert!(plan.env.passthrough_names.contains(&"TERM".to_owned()));
    assert!(plan.env.passthrough_names.contains(&"EDITOR".to_owned()));
}

#[test]
fn security_defaults() {
    let toml = minimal_config_toml();
    let workdir = tempfile::tempdir().unwrap();
    let plan = build_plan_from(&toml, workdir.path());

    assert_eq!(plan.security.userns, "keep-id");
    assert_eq!(plan.security.cap_drop, "all");
    assert_eq!(plan.security.pids_limit, 4096);
    assert!(
        plan.security
            .security_opts
            .contains(&"no-new-privileges".to_owned())
    );
    assert!(
        plan.security
            .security_opts
            .contains(&"label=disable".to_owned())
    );
}

#[test]
fn network_mode_without_browser() {
    let toml = minimal_config_toml();
    let workdir = tempfile::tempdir().unwrap();
    let plan = build_plan_from(&toml, workdir.path());
    assert_eq!(plan.network_mode, "slirp4netns:allow_host_loopback=false");
}

#[test]
fn network_mode_with_browser() {
    let toml = format!(
        "{}\n{}",
        minimal_config_toml(),
        r#"
[browser]
enabled = true
command = "google-chrome"
profile_dir = "/tmp/chrome"
debug_port = 9222
"#
    );
    let workdir = tempfile::tempdir().unwrap();
    let config = parse_toml_str(&toml, Path::new("/test/config.toml")).unwrap();
    let secrets = HashMap::new();
    let plan = build_launch_plan(
        &config,
        workdir.path(),
        Agent::Pi,
        true,
        false,
        None,
        &secrets,
        None,
    )
    .unwrap();
    assert_eq!(plan.network_mode, "slirp4netns:allow_host_loopback=true");
}

#[test]
fn boot_dirs_in_entrypoint() {
    let toml = minimal_config_toml();
    let workdir = tempfile::tempdir().unwrap();
    let plan = build_plan_from(&toml, workdir.path());

    assert!(
        plan.entrypoint.starts_with("mkdir -p"),
        "entrypoint should start with mkdir: {}",
        plan.entrypoint
    );
    assert!(plan.entrypoint.contains("/home/dev/.ssh"));
    assert!(plan.entrypoint.contains("exec pi --no-extensions"));
}

#[test]
fn entrypoint_has_guard_extension() {
    let toml = minimal_config_toml();
    let workdir = tempfile::tempdir().unwrap();
    let plan = build_plan_from(&toml, workdir.path());

    assert!(
        plan.entrypoint
            .contains("/home/dev/.pi/agent/extensions/guard.ts")
    );
    assert!(
        plan.entrypoint.contains("--append-system-prompt"),
        "pi should append a short host-service hint in system prompt: {}",
        plan.entrypoint
    );
    assert!(
        plan.entrypoint.contains("host.containers.internal"),
        "pi host-service system hint missing: {}",
        plan.entrypoint
    );
    assert!(plan.entrypoint.contains("\"$@\""));
}

#[test]
fn tmux_mode_wraps_agent_command() {
    let toml = minimal_config_toml();
    let workdir = tempfile::tempdir().unwrap();
    let config = parse_toml_str(&toml, Path::new("/test/config.toml")).unwrap();
    let secrets = HashMap::new();
    let plan = build_launch_plan(
        &config,
        workdir.path(),
        Agent::Pi,
        false,
        true,
        None,
        &secrets,
        None,
    )
    .unwrap();

    assert!(plan.entrypoint.contains("command -v tmux"));
    assert!(plan.entrypoint.contains("Run `ags update`"));
    assert!(plan.entrypoint.contains("/tmp/ags-run-in-tmux.sh"));
    assert!(plan.entrypoint.contains("exec tmux new-session -A -s ags"));
    assert!(plan.entrypoint.contains("exec pi --no-extensions"));
}

#[test]
fn entrypoint_prints_host_services_hint_for_tty_sessions() {
    let toml = minimal_config_toml();
    let workdir = tempfile::tempdir().unwrap();
    let plan = build_plan_from(&toml, workdir.path());

    assert!(
        plan.entrypoint
            .contains("host.containers.internal (localhost is container-local)"),
        "entrypoint missing host services hint: {}",
        plan.entrypoint
    );
    assert!(plan.entrypoint.contains("if [ -t 1 ]; then echo"));
}

#[test]
fn optional_mount_skipped_when_missing() {
    let dir = tempfile::tempdir().unwrap();
    let toml = format!(
        "{}\n\
[[mount]]\n\
host = \"{}/nonexistent-dir\"\n\
container = \"/data\"\n\
mode = \"ro\"\n\
optional = true\n",
        minimal_config_toml(),
        dir.path().display()
    );
    let workdir = tempfile::tempdir().unwrap();
    let plan = build_plan_from(&toml, workdir.path());

    let found = plan.mounts.iter().any(|m| m.container == "/data");
    assert!(!found, "optional missing mount should be skipped");
}

#[test]
fn required_mount_missing_is_error() {
    let dir = tempfile::tempdir().unwrap();
    let toml = format!(
        "{}\n\
[[mount]]\n\
host = \"{}/nonexistent-dir\"\n\
container = \"/data\"\n\
mode = \"ro\"\n",
        minimal_config_toml(),
        dir.path().display()
    );
    let workdir = tempfile::tempdir().unwrap();
    let config = parse_toml_str(&toml, Path::new("/test/config.toml")).unwrap();
    let secrets = HashMap::new();
    let result = build_launch_plan(
        &config,
        workdir.path(),
        Agent::Pi,
        false,
        false,
        None,
        &secrets,
        None,
    );
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(err.contains("required mount"), "got: {err}");
}

#[test]
fn create_mount_creates_directory() {
    let dir = tempfile::tempdir().unwrap();
    let mount_host = dir.path().join("auto-created");
    let toml = format!(
        "{}\n\
[[mount]]\n\
host = \"{}\"\n\
container = \"/created\"\n\
mode = \"rw\"\n\
kind = \"dir\"\n\
create = true\n",
        minimal_config_toml(),
        mount_host.display()
    );
    let workdir = tempfile::tempdir().unwrap();
    let plan = build_plan_from(&toml, workdir.path());

    assert!(mount_host.exists(), "create=true should create the dir");
    let found = plan.mounts.iter().any(|m| m.container == "/created");
    assert!(found, "created mount should be in the plan");
}

#[test]
fn browser_mount_skipped_without_browser_mode() {
    let dir = tempfile::tempdir().unwrap();
    let mount_host = dir.path().join("browser-dir");
    fs::create_dir_all(&mount_host).unwrap();
    let toml = format!(
        "{}\n\
[[mount]]\n\
host = \"{}\"\n\
container = \"/browser-data\"\n\
mode = \"ro\"\n\
when = \"browser\"\n",
        minimal_config_toml(),
        mount_host.display()
    );
    let workdir = tempfile::tempdir().unwrap();
    // browser_mode = false
    let plan = build_plan_from(&toml, workdir.path());

    let found = plan.mounts.iter().any(|m| m.container == "/browser-data");
    assert!(
        !found,
        "browser mount should be skipped without browser mode"
    );
}

#[test]
fn read_write_roots_json_valid() {
    let toml = minimal_config_toml();
    let workdir = tempfile::tempdir().unwrap();
    let plan = build_plan_from(&toml, workdir.path());

    assert!(
        plan.env.read_roots_json.starts_with('['),
        "read roots should be JSON array"
    );
    assert!(
        plan.env.write_roots_json.starts_with('['),
        "write roots should be JSON array"
    );
    // Should contain /tmp and /home/dev/.pi
    assert!(plan.env.read_roots_json.contains("/tmp"));
    assert!(plan.env.read_roots_json.contains("/home/dev/.pi"));
}

#[test]
fn secrets_in_env_file() {
    let toml = minimal_config_toml();
    let workdir = tempfile::tempdir().unwrap();
    let config = parse_toml_str(&toml, Path::new("/test/config.toml")).unwrap();
    let mut secrets = HashMap::new();
    secrets.insert("GH_TOKEN".to_owned(), "ghp_test123".to_owned());
    let plan = build_launch_plan(
        &config,
        workdir.path(),
        Agent::Pi,
        false,
        false,
        None,
        &secrets,
        None,
    )
    .unwrap();

    let found = plan
        .env
        .env_file_entries
        .iter()
        .any(|(k, v)| k == "GH_TOKEN" && v == "ghp_test123");
    assert!(found, "resolved secrets should be in env_file_entries");
}

#[test]
fn ssh_socket_mounted_when_provided() {
    let toml = minimal_config_toml();
    let workdir = tempfile::tempdir().unwrap();
    let config = parse_toml_str(&toml, Path::new("/test/config.toml")).unwrap();
    let secrets = HashMap::new();
    let sock = Path::new("/tmp/test-ssh-agent.sock");
    let plan = build_launch_plan(
        &config,
        workdir.path(),
        Agent::Pi,
        false,
        false,
        Some(sock),
        &secrets,
        None,
    )
    .unwrap();

    let found = plan.mounts.iter().any(|m| m.container == "/ssh-agent");
    assert!(found, "SSH socket should be mounted");
}

#[test]
fn nonexistent_workdir_is_error() {
    let toml = minimal_config_toml();
    let config = parse_toml_str(&toml, Path::new("/test/config.toml")).unwrap();
    let secrets = HashMap::new();
    let result = build_launch_plan(
        &config,
        Path::new("/nonexistent/workdir"),
        Agent::Pi,
        false,
        false,
        None,
        &secrets,
        None,
    );
    assert!(matches!(result, Err(PlanError::WorkdirResolve(_))));
}

#[test]
fn entrypoint_browser_mode_has_socat() {
    let toml = format!(
        "{}\n{}",
        minimal_config_toml(),
        r#"
[browser]
enabled = true
command = "google-chrome"
profile_dir = "/tmp/chrome"
debug_port = 9222
pi_skill_path = "/home/dev/browser-tools"
"#
    );
    let workdir = tempfile::tempdir().unwrap();
    let config = parse_toml_str(&toml, Path::new("/test/config.toml")).unwrap();
    let secrets = HashMap::new();
    let plan = build_launch_plan(
        &config,
        workdir.path(),
        Agent::Pi,
        true,
        false,
        None,
        &secrets,
        None,
    )
    .unwrap();

    assert!(
        plan.entrypoint.contains("socat TCP-LISTEN:9222"),
        "browser mode entrypoint should have socat: {}",
        plan.entrypoint
    );
    assert!(
        plan.entrypoint.contains("--skill /home/dev/browser-tools"),
        "should have --skill flag: {}",
        plan.entrypoint
    );
}

// --- Agent-specific tests ---

#[test]
fn claude_agent_entrypoint() {
    let toml = minimal_config_toml();
    let workdir = tempfile::tempdir().unwrap();
    let plan = build_plan_from_agent(&toml, workdir.path(), Agent::Claude);

    assert!(
        plan.entrypoint.contains("exec claude"),
        "claude entrypoint should exec claude: {}",
        plan.entrypoint
    );
    assert!(
        plan.entrypoint.contains("--dangerously-skip-permissions"),
        "claude should disable internal sandbox/prompts in ags: {}",
        plan.entrypoint
    );
    assert!(
        plan.entrypoint.contains("--settings") && plan.entrypoint.contains("\"enabled\":false"),
        "claude should disable builtin bash sandbox in ags: {}",
        plan.entrypoint
    );
    assert!(
        plan.entrypoint.contains("--append-system-prompt"),
        "claude should append host-service hint in system prompt: {}",
        plan.entrypoint
    );
    assert!(
        plan.entrypoint.contains("host.containers.internal"),
        "claude host-service system hint missing: {}",
        plan.entrypoint
    );
    assert!(
        !plan.entrypoint.contains("guard.ts"),
        "claude should not have guard.ts: {}",
        plan.entrypoint
    );
    assert!(
        !plan.entrypoint.contains("--no-extensions"),
        "claude should not have --no-extensions: {}",
        plan.entrypoint
    );
    assert!(
        plan.entrypoint.contains("--plugin-dir /home/dev/.config/ags/hooks"),
        "claude should load guard skill via --plugin-dir: {}",
        plan.entrypoint
    );
}

#[test]
fn claude_agent_has_config_mount() {
    let toml = minimal_config_toml();
    let workdir = tempfile::tempdir().unwrap();
    let plan = build_plan_from_agent(&toml, workdir.path(), Agent::Claude);

    let claude_mount = plan
        .mounts
        .iter()
        .find(|m| m.container == "/home/dev/.claude");
    assert!(claude_mount.is_some(), "claude should have .claude mount");
    assert_eq!(claude_mount.unwrap().mode, MountMode::Rw);
}

#[test]
fn claude_agent_has_config_env() {
    let toml = minimal_config_toml();
    let workdir = tempfile::tempdir().unwrap();
    let plan = build_plan_from_agent(&toml, workdir.path(), Agent::Claude);

    let find_env = |key: &str| -> Option<String> {
        plan.env
            .inline
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
    };

    assert!(
        find_env("CLAUDE_CONFIG_DIR").is_none(),
        "claude should not set CLAUDE_CONFIG_DIR (uses $HOME/.claude by default)"
    );
    assert!(
        find_env("PI_CODING_AGENT_DIR").is_none(),
        "claude should not have PI_CODING_AGENT_DIR"
    );
}

#[test]
fn claude_agent_no_extra_boot_dirs() {
    let toml = minimal_config_toml();
    let workdir = tempfile::tempdir().unwrap();
    let plan = build_plan_from_agent(&toml, workdir.path(), Agent::Claude);

    // Claude should not add extra_boot_dirs. Guard skill is loaded via --plugin-dir,
    // not via symlink in the entrypoint.
    assert!(
        plan.entrypoint.contains("--plugin-dir"),
        "claude entrypoint should have --plugin-dir for guard skill: {}",
        plan.entrypoint
    );
    assert!(
        !plan.entrypoint.contains("ln -sf"),
        "claude should not have symlink setup (uses --plugin-dir): {}",
        plan.entrypoint
    );
}

#[test]
fn codex_agent_entrypoint() {
    let toml = minimal_config_toml();
    let workdir = tempfile::tempdir().unwrap();
    let plan = build_plan_from_agent(&toml, workdir.path(), Agent::Codex);

    assert!(
        plan.entrypoint.contains("exec codex"),
        "codex entrypoint should exec codex: {}",
        plan.entrypoint
    );
    assert!(
        plan.entrypoint.contains("developer_instructions="),
        "codex should inject host-service developer hint: {}",
        plan.entrypoint
    );
    assert!(
        plan.entrypoint.contains("host.containers.internal"),
        "codex host-service hint missing: {}",
        plan.entrypoint
    );
    assert!(
        !plan.entrypoint.contains("guard.ts"),
        "codex should not have guard.ts"
    );
}

#[test]
fn gemini_agent_has_sandbox_mount() {
    let toml = minimal_config_toml();
    let workdir = tempfile::tempdir().unwrap();
    let plan = build_plan_from_agent(&toml, workdir.path(), Agent::Gemini);

    assert!(
        plan.entrypoint.contains("exec gemini"),
        "gemini entrypoint: {}",
        plan.entrypoint
    );
    let gemini_mount = plan
        .mounts
        .iter()
        .find(|m| m.container == "/home/dev/.gemini");
    assert!(gemini_mount.is_some(), "gemini should have .gemini mount");
    assert_eq!(gemini_mount.unwrap().mode, MountMode::Rw);
}

#[test]
fn opencode_agent_has_sandbox_mount() {
    let toml = minimal_config_toml();
    let workdir = tempfile::tempdir().unwrap();
    let plan = build_plan_from_agent(&toml, workdir.path(), Agent::Opencode);

    assert!(
        plan.entrypoint.contains("exec opencode"),
        "opencode entrypoint: {}",
        plan.entrypoint
    );
    let oc_mount = plan
        .mounts
        .iter()
        .find(|m| m.container == "/home/dev/.config/opencode");
    assert!(oc_mount.is_some(), "opencode should have config mount");
    assert_eq!(oc_mount.unwrap().mode, MountMode::Rw);
}

#[test]
fn different_agents_have_different_entrypoints() {
    let toml = minimal_config_toml();
    let workdir = tempfile::tempdir().unwrap();

    let pi_plan = build_plan_from_agent(&toml, workdir.path(), Agent::Pi);
    let claude_plan = build_plan_from_agent(&toml, workdir.path(), Agent::Claude);
    let codex_plan = build_plan_from_agent(&toml, workdir.path(), Agent::Codex);

    assert_ne!(pi_plan.entrypoint, claude_plan.entrypoint);
    assert_ne!(claude_plan.entrypoint, codex_plan.entrypoint);
    assert_ne!(pi_plan.entrypoint, codex_plan.entrypoint);
}

#[test]
fn non_pi_agent_still_has_explicit_agent_mounts() {
    let toml = minimal_config_toml();
    let workdir = tempfile::tempdir().unwrap();
    let plan = build_plan_from_agent(&toml, workdir.path(), Agent::Codex);

    let pi_mount = plan.mounts.iter().find(|m| m.container == "/home/dev/.pi");
    assert!(
        pi_mount.is_some(),
        "explicit config mounts should be present for all agents"
    );

    let codex_mount = plan
        .mounts
        .iter()
        .find(|m| m.container == "/home/dev/.codex");
    assert!(codex_mount.is_some(), "codex should have .codex mount");
}

#[test]
fn non_pi_agent_no_pi_env() {
    let toml = minimal_config_toml();
    let workdir = tempfile::tempdir().unwrap();
    let plan = build_plan_from_agent(&toml, workdir.path(), Agent::Codex);

    let has_pi_env = plan
        .env
        .inline
        .iter()
        .any(|(k, _)| k == "PI_CODING_AGENT_DIR");
    assert!(!has_pi_env, "codex should not have PI_CODING_AGENT_DIR");
}

// --- Auth proxy integration ---

#[test]
fn auth_proxy_mounts_and_env_when_enabled() {
    let toml = minimal_config_toml();
    let workdir = tempfile::tempdir().unwrap();
    let auth_dir = tempfile::tempdir().unwrap();

    // Write a dummy shim so the mount source exists
    let shim_path = auth_dir.path().join("auth-proxy-shim");
    fs::write(&shim_path, "#!/bin/sh\n").unwrap();

    let config = parse_toml_str(&toml, Path::new("/test/config.toml")).unwrap();
    let secrets = HashMap::new();
    let plan = build_launch_plan(
        &config,
        workdir.path(),
        Agent::Claude,
        false,
        false,
        None,
        &secrets,
        Some(auth_dir.path()),
    )
    .unwrap();

    // Should have runtime dir mount
    let runtime_mount = plan
        .mounts
        .iter()
        .find(|m| m.container == "/run/ags-auth-proxy");
    assert!(
        runtime_mount.is_some(),
        "auth proxy runtime dir should be mounted"
    );
    assert_eq!(runtime_mount.unwrap().mode, MountMode::Rw);

    // Should have shim mount
    let shim_mount = plan
        .mounts
        .iter()
        .find(|m| m.container == "/home/dev/.local/bin/auth-proxy-shim");
    assert!(shim_mount.is_some(), "auth proxy shim should be mounted");
    assert_eq!(shim_mount.unwrap().mode, MountMode::Ro);

    // Should have BROWSER and AGS_AUTH_PROXY_SOCK env vars
    let find_env = |key: &str| -> Option<String> {
        plan.env
            .inline
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
    };
    assert_eq!(
        find_env("AGS_AUTH_PROXY_SOCK"),
        Some("/run/ags-auth-proxy/auth-proxy.sock".to_owned())
    );
    assert_eq!(
        find_env("BROWSER"),
        Some("/home/dev/.local/bin/auth-proxy-shim".to_owned())
    );
}

#[test]
fn no_auth_proxy_env_when_disabled() {
    let toml = minimal_config_toml();
    let workdir = tempfile::tempdir().unwrap();
    let plan = build_plan_from(&toml, workdir.path());

    let find_env = |key: &str| -> Option<String> {
        plan.env
            .inline
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
    };
    assert!(
        find_env("AGS_AUTH_PROXY_SOCK").is_none(),
        "no auth proxy env when disabled"
    );
    assert!(
        find_env("BROWSER").is_none(),
        "BROWSER should not be set without auth proxy"
    );
}
