use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use ags::cli::{Agent, RunOptions};
use ags::config::{MountMode, parse_toml_str};
use ags::lockdown::{prepare, validate};
use ags::plan::{BuildLaunchPlanOptions, PlanMount, build_launch_plan};

fn write_file(path: &Path, contents: &str) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, contents).unwrap();
}

fn config_toml(base: &Path, host_ui_enabled: bool) -> String {
    let host_ui = if host_ui_enabled { "true" } else { "false" };
    format!(
        r#"
[sandbox]
image = "localhost/agent-sandbox:latest"
containerfile = "{base}/Containerfile"
cache_dir = "{base}/cache"
gitconfig_path = "{base}/gitconfig"
auth_key = "{base}/auth"
sign_key = "{base}/sign"
container_boot_dirs = ["/home/dev/.ssh"]
passthrough_env = ["HOST_TOKEN"]

[host_ui]
enabled = {host_ui}

[[mount]]
host = "{base}/shared"
container = "/opt/shared"
mode = "rw"

[[agent_mount]]
host = "{base}/pi"
container = "/home/dev/.pi"

[[agent_mount]]
host = "{base}/codex"
container = "/home/dev/.codex"

[[agent_mount]]
host = "{base}/claude"
container = "/home/dev/.claude"

[[agent_mount]]
host = "{base}/claude.json"
container = "/home/dev/.claude.json"
kind = "file"

[[secret]]
env = "HOST_TOKEN"
from_env = "HOST_TOKEN"
"#,
        base = base.display(),
        host_ui = host_ui,
    )
}

fn setup_base_dirs(base: &Path) {
    write_file(&base.join("Containerfile"), "FROM scratch\n");
    fs::create_dir_all(base.join("shared")).unwrap();
    fs::create_dir_all(base.join("pi/agent/extensions")).unwrap();
    fs::create_dir_all(base.join("pi/history")).unwrap();
    fs::create_dir_all(base.join("pi/prompts")).unwrap();
    fs::create_dir_all(base.join("codex")).unwrap();
    fs::create_dir_all(base.join("claude/agents")).unwrap();
    fs::create_dir_all(base.join("claude/sessions")).unwrap();
    write_file(&base.join("claude.json"), "{}\n");
    write_file(&base.join("claude/.credentials.json"), "token\n");
    write_file(&base.join("claude/.mcp.json"), "{}\n");
    write_file(&base.join("claude/agents/review.md"), "review agent\n");
    write_file(&base.join("claude/sessions/old.json"), "old session\n");
    write_file(&base.join("pi/auth.json"), "auth\n");
    write_file(&base.join("pi/models.json"), "models\n");
    write_file(&base.join("pi/transcript.log"), "old transcript\n");
    write_file(&base.join("pi/history/old.txt"), "old history\n");
    write_file(&base.join("pi/prompts/review.md"), "prompt\n");
    write_file(&base.join("codex/config.toml"), "model='x'\n");
    write_file(&base.join("cache/pnpm-home/pi"), "#!/usr/bin/env bash\n");
    write_file(&base.join("cache/pnpm-home/codex"), "#!/usr/bin/env bash\n");
    write_file(
        &base.join("cache/claude-install/.local/bin/claude"),
        "#!/usr/bin/env bash\n",
    );
}

fn run_opts(agent: Agent) -> RunOptions {
    RunOptions {
        agent,
        browser: false,
        tmux: false,
        psp: false,
        psp_keep: false,
        yolo: false,
        root: false,
        lockdown: true,
        stop_when_done: false,
        config_path: None,
        add_dirs: Vec::new(),
        passthrough_args: Vec::new(),
    }
}

fn lockdown_options<'a>(
    secrets: &'a HashMap<String, String>,
    extra_mounts: &'a [PlanMount],
    extra_mount_dirs: &'a [PathBuf],
) -> BuildLaunchPlanOptions<'a> {
    BuildLaunchPlanOptions {
        browser_mode: false,
        tmux_mode: false,
        guard_enabled: true,
        lockdown: true,
        ssh_auth_sock: None,
        resolved_secrets: secrets,
        auth_proxy_runtime_dir: None,
        host_ui_runtime_dir: None,
        host_ui_session_id: None,
        webview_relay_runtime_dir: None,
        psp_socket: None,
        psp_session_id: None,
        extra_mounts,
        extra_mount_dirs,
        stop_when_done: false,
        root_mode: false,
    }
}

#[test]
fn validate_rejects_incompatible_lockdown_flags() {
    let temp = tempfile::tempdir().unwrap();
    setup_base_dirs(temp.path());
    for mut opts in [
        {
            let mut opts = run_opts(Agent::Pi);
            opts.browser = true;
            opts
        },
        {
            let mut opts = run_opts(Agent::Pi);
            opts.psp = true;
            opts
        },
        {
            let mut opts = run_opts(Agent::Pi);
            opts.psp_keep = true;
            opts
        },
        {
            let mut opts = run_opts(Agent::Pi);
            opts.root = true;
            opts
        },
    ] {
        let err = validate(&opts).expect_err("expected lockdown validation failure");
        assert!(err.to_string().contains("--lockdown cannot be combined"));
        opts.lockdown = false;
        validate(&opts).expect("non-lockdown run should pass");
    }
}

#[test]
fn validate_allows_enabled_host_ui_because_lockdown_disables_it() {
    let temp = tempfile::tempdir().unwrap();
    setup_base_dirs(temp.path());
    validate(&run_opts(Agent::Pi)).expect("lockdown should ignore enabled host_ui");
}

#[test]
fn prepare_stages_claude_auth_but_not_sessions() {
    let temp = tempfile::tempdir().unwrap();
    setup_base_dirs(temp.path());
    let config = parse_toml_str(
        &config_toml(temp.path(), false),
        Path::new("/test/config.toml"),
    )
    .unwrap();

    let session = prepare(Agent::Claude, &config, true).expect("lockdown staging should succeed");
    let staged_home = session
        .extra_mounts
        .iter()
        .find(|m| m.container == "/home/dev/.claude")
        .map(|m| m.host.clone())
        .expect("staged claude home mount");
    let staged_hooks = session
        .extra_mounts
        .iter()
        .find(|m| m.container == "/run/ags-claude-hooks")
        .map(|m| m.host.clone())
        .expect("staged claude hook mount");

    assert!(staged_home.join(".credentials.json").exists());
    assert!(staged_home.join(".mcp.json").exists());
    assert!(staged_home.join("agents/review.md").exists());
    assert!(!staged_home.join("sessions").exists());
    assert!(staged_hooks.join("guard.sh").exists());
    assert!(staged_hooks.join("skills/guard/SKILL.md").exists());
}

#[test]
fn prepare_stages_selected_agent_only_and_discards_history() {
    let temp = tempfile::tempdir().unwrap();
    setup_base_dirs(temp.path());
    let config = parse_toml_str(
        &config_toml(temp.path(), false),
        Path::new("/test/config.toml"),
    )
    .unwrap();

    let session = prepare(Agent::Pi, &config, true).expect("lockdown staging should succeed");
    let staged_home = session
        .extra_mounts
        .iter()
        .find(|m| m.container == "/home/dev/.pi")
        .map(|m| m.host.clone())
        .expect("staged pi home mount");
    let staged_runtime = session
        .extra_mounts
        .iter()
        .find(|m| m.container == "/usr/local/pnpm")
        .map(|m| m.host.clone())
        .expect("staged pnpm runtime");

    assert_eq!(
        session
            .extra_mounts
            .iter()
            .find(|m| m.container == "/home/dev/.pi")
            .unwrap()
            .mode,
        MountMode::Rw
    );
    assert_eq!(
        session
            .extra_mounts
            .iter()
            .find(|m| m.container == "/usr/local/pnpm")
            .unwrap()
            .mode,
        MountMode::Ro
    );
    assert!(
        !session
            .extra_mounts
            .iter()
            .any(|m| m.container == "/home/dev/.codex")
    );

    assert!(staged_home.join("auth.json").exists());
    assert!(staged_home.join("models.json").exists());
    assert!(staged_home.join("agent/extensions/guard.ts").exists());
    assert!(staged_home.join("agent/settings.json").exists());
    assert!(staged_home.join("prompts/review.md").exists());
    assert!(!staged_home.join("transcript.log").exists());
    assert!(!staged_home.join("history").exists());
    assert!(staged_runtime.join("pi").exists());
    assert!(staged_runtime.join("codex").exists());

    drop(session);
    assert!(
        !staged_home.exists(),
        "staged lockdown home should be deleted"
    );
    assert!(
        !staged_runtime.exists(),
        "staged lockdown runtime should be deleted"
    );
}

#[test]
fn lockdown_plan_filters_mounts_and_env() {
    let temp = tempfile::tempdir().unwrap();
    setup_base_dirs(temp.path());
    let config = parse_toml_str(
        &config_toml(temp.path(), false),
        Path::new("/test/config.toml"),
    )
    .unwrap();
    let workdir = tempfile::tempdir().unwrap();
    let extra_dir = tempfile::tempdir().unwrap();
    let staged_home = tempfile::tempdir().unwrap();
    let staged_runtime = tempfile::tempdir().unwrap();
    let secrets = HashMap::from([(String::from("HOST_TOKEN"), String::from("secret"))]);
    let extra_mounts = vec![
        PlanMount {
            host: staged_home.path().to_path_buf(),
            container: "/home/dev/.pi".to_owned(),
            mode: MountMode::Rw,
        },
        PlanMount {
            host: staged_runtime.path().to_path_buf(),
            container: "/usr/local/pnpm".to_owned(),
            mode: MountMode::Ro,
        },
    ];
    let extra_mount_dirs = vec![extra_dir.path().to_path_buf()];

    let plan = build_launch_plan(
        &config,
        workdir.path(),
        Agent::Pi,
        lockdown_options(&secrets, &extra_mounts, &extra_mount_dirs),
    )
    .unwrap();

    assert!(plan.mounts.iter().any(|m| m.container == "/home/dev/.pi"));
    assert!(plan.mounts.iter().any(|m| m.container == "/usr/local/pnpm"));
    assert!(
        plan.mounts
            .iter()
            .any(|m| m.container == extra_dir.path().to_string_lossy())
    );
    assert!(!plan.mounts.iter().any(|m| m.container == "/opt/shared"));
    assert!(
        !plan
            .mounts
            .iter()
            .any(|m| m.container == "/home/dev/.config/ags/gitconfig")
    );
    assert!(
        !plan
            .mounts
            .iter()
            .any(|m| m.container == "/opt/claude-home")
    );
    assert!(
        !plan
            .mounts
            .iter()
            .any(|m| m.container == "/home/dev/.codex")
    );

    let inline_keys: Vec<&str> = plan.env.inline.iter().map(|(k, _)| k.as_str()).collect();
    assert!(!inline_keys.contains(&"SSH_AUTH_SOCK"));
    assert!(!inline_keys.contains(&"GIT_CONFIG_GLOBAL"));
    assert!(!inline_keys.contains(&"AGS_HOST_SERVICES_HOST"));
    assert!(plan.env.passthrough_names.is_empty());
    assert!(plan.env.env_file_entries.is_empty());
    assert!(plan.env.read_roots_json.contains("/home/dev/.pi"));
    assert!(
        plan.env
            .write_roots_json
            .contains(&extra_dir.path().to_string_lossy().to_string())
    );
    assert!(
        plan.env
            .inline
            .iter()
            .any(|(k, v)| k == "AGS_LOCKDOWN" && v == "1")
    );
    assert!(plan.security.tmpfs.iter().any(|v| v.starts_with("/tmp:")));
    assert!(
        plan.security
            .tmpfs
            .iter()
            .any(|v| v.starts_with("/var/tmp:"))
    );
    assert!(plan.security.tmpfs.iter().any(|v| v.starts_with("/run:")));
}

#[test]
fn lockdown_plan_ignores_host_bridge_inputs() {
    let temp = tempfile::tempdir().unwrap();
    setup_base_dirs(temp.path());
    let config = parse_toml_str(
        &config_toml(temp.path(), true),
        Path::new("/test/config.toml"),
    )
    .unwrap();
    let workdir = tempfile::tempdir().unwrap();
    let auth_proxy = tempfile::tempdir().unwrap();
    let host_ui = tempfile::tempdir().unwrap();
    let webview = tempfile::tempdir().unwrap();
    let psp = tempfile::tempdir().unwrap();
    let psp_socket = psp.path().join("podman.sock");
    write_file(&psp_socket, "");
    let secrets = HashMap::new();

    let plan = build_launch_plan(
        &config,
        workdir.path(),
        Agent::Pi,
        BuildLaunchPlanOptions {
            browser_mode: true,
            auth_proxy_runtime_dir: Some(auth_proxy.path()),
            host_ui_runtime_dir: Some(host_ui.path()),
            host_ui_session_id: Some("host-ui-session"),
            webview_relay_runtime_dir: Some(webview.path()),
            psp_socket: Some(&psp_socket),
            psp_session_id: Some("psp-session"),
            ssh_auth_sock: Some(&psp_socket),
            ..lockdown_options(&secrets, &[], &[])
        },
    )
    .unwrap();

    assert_eq!(plan.network_mode, "slirp4netns:allow_host_loopback=false");
    assert!(!plan.entrypoint.contains("socat TCP-LISTEN"));
    assert!(!plan.entrypoint.contains("webview-relay-shim"));

    for denied in [
        auth_proxy.path(),
        host_ui.path(),
        webview.path(),
        psp.path(),
        psp_socket.as_path(),
    ] {
        assert!(
            !plan.mounts.iter().any(|m| m.host == denied),
            "lockdown should not mount {}",
            denied.display()
        );
    }

    let inline_keys: Vec<&str> = plan.env.inline.iter().map(|(k, _)| k.as_str()).collect();
    for key in [
        "BROWSER",
        "AGS_AUTH_PROXY_SOCK",
        "AGS_HOST_UI_SOCK",
        "AGS_HOST_UI_SESSION_ID",
        "AGS_WEBVIEW_RELAY_SOCKET",
        "DOCKER_HOST",
        "TESTCONTAINERS_HOST_OVERRIDE",
        "PSP_SESSION_ID",
    ] {
        assert!(
            !inline_keys.contains(&key),
            "{key} should be suppressed in lockdown"
        );
    }
}
