use std::collections::HashMap;
use std::fs;
use std::path::Path;

use ags::cli::Agent;
use ags::config::{ClipboardMode, MountMode, parse_toml_str};
use ags::plan::{
    BuildLaunchPlanOptions, ONEPASSWORD_BOOTSTRAP_CONTAINER_PATH, PlanError, PlanMount,
    build_launch_plan,
};

fn config_toml(base: &Path) -> String {
    let containerfile = base.join("Containerfile");
    fs::write(&containerfile, "FROM scratch\n").unwrap();
    for path in ["pi", "claude", "codex", "gemini", "opencode"] {
        fs::create_dir_all(base.join(path)).unwrap();
    }
    fs::write(base.join(".claude.json"), "{}\n").unwrap();
    format!(
        r#"
[sandbox]
image = "localhost/agent-sandbox:latest"
containerfile = "{containerfile}"
cache_dir = "{base}/cache"
gitconfig_path = "{base}/gitconfig"
auth_key = "{base}/auth"
sign_key = "{base}/sign"

[browser]
enabled = true
command = "google-chrome"
profile_dir = "/tmp/chrome"
debug_port = 9222

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

fn bootstrap_asset(root: &Path) -> std::path::PathBuf {
    let asset = root.join("onepassword-bootstrap");
    fs::write(&asset, ags::assets::ONEPASSWORD_BOOTSTRAP).unwrap();
    asset
}

fn options(secrets: &HashMap<String, String>) -> BuildLaunchPlanOptions<'_> {
    BuildLaunchPlanOptions {
        browser_mode: false,
        tmux_mode: false,
        guard_enabled: true,
        lockdown: false,
        ssh_auth_sock: None,
        resolved_secrets: secrets,
        auth_proxy_runtime_dir: None,
        clipboard_runtime_dir: None,
        clipboard_mode: ClipboardMode::Off,
        host_ui_runtime_dir: None,
        host_ui_session_id: None,
        webview_relay_runtime_dir: None,
        psp_socket: None,
        psp_session_id: None,
        extra_mounts: &[],
        extra_mount_dirs: &[],
        stop_when_done: false,
        root_mode: false,
        wayland_passthrough: false,
        payload_fd_count: 0,
        bootstrap_path: None,
        bootstrap_host_path: None,
    }
}

#[test]
fn no_payload_leaves_the_entrypoint_and_transport_unconfigured() {
    let root = tempfile::tempdir().unwrap();
    let workdir = tempfile::tempdir().unwrap();
    let config = parse_toml_str(&config_toml(root.path()), Path::new("/test/config.toml")).unwrap();
    let secrets = HashMap::new();
    let plan = build_launch_plan(&config, workdir.path(), Agent::Pi, options(&secrets)).unwrap();

    assert_eq!(plan.payload_fd_count, 0);
    assert_eq!(plan.bootstrap_path, None);
    assert!(!plan.entrypoint.contains("--fd-count"));
    assert!(
        !plan
            .mounts
            .iter()
            .any(|mount| mount.container == ONEPASSWORD_BOOTSTRAP_CONTAINER_PATH)
    );
}

#[test]
fn payload_plan_rejects_unowned_bootstrap_path() {
    let root = tempfile::tempdir().unwrap();
    let workdir = tempfile::tempdir().unwrap();
    let config = parse_toml_str(&config_toml(root.path()), Path::new("/test/config.toml")).unwrap();
    let secrets = HashMap::new();
    let error = build_launch_plan(
        &config,
        workdir.path(),
        Agent::Pi,
        BuildLaunchPlanOptions {
            payload_fd_count: 1,
            bootstrap_path: Some("/tmp/untrusted-bootstrap"),
            ..options(&secrets)
        },
    )
    .unwrap_err();

    assert!(matches!(error, PlanError::PayloadBootstrapInvalid));
}

#[test]
fn trusted_bootstrap_mount_is_rendered_after_every_other_mount() {
    let root = tempfile::tempdir().unwrap();
    let workdir = tempfile::tempdir().unwrap();
    let config = parse_toml_str(&config_toml(root.path()), Path::new("/test/config.toml")).unwrap();
    let secrets = HashMap::new();
    let bootstrap = bootstrap_asset(root.path());
    let extra_mounts =
        ["/run", "/var/run", ONEPASSWORD_BOOTSTRAP_CONTAINER_PATH].map(|destination| PlanMount {
            host: root.path().to_owned(),
            container: destination.to_owned(),
            mode: MountMode::Ro,
        });

    let plan = build_launch_plan(
        &config,
        workdir.path(),
        Agent::Pi,
        BuildLaunchPlanOptions {
            extra_mounts: &extra_mounts,
            payload_fd_count: 1,
            bootstrap_path: Some(ONEPASSWORD_BOOTSTRAP_CONTAINER_PATH),
            bootstrap_host_path: Some(&bootstrap),
            ..options(&secrets)
        },
    )
    .unwrap();

    let trusted = plan.mounts.last().unwrap();
    assert_eq!(trusted.host, bootstrap);
    assert_eq!(trusted.container, ONEPASSWORD_BOOTSTRAP_CONTAINER_PATH);
    assert_eq!(trusted.mode, MountMode::Ro);
}

#[test]
fn bootstrap_wraps_direct_agent_and_closes_fds_for_browser_helper() {
    let root = tempfile::tempdir().unwrap();
    let workdir = tempfile::tempdir().unwrap();
    let config = parse_toml_str(&config_toml(root.path()), Path::new("/test/config.toml")).unwrap();
    let secrets = HashMap::new();
    let bootstrap = bootstrap_asset(root.path());
    let plan = build_launch_plan(
        &config,
        workdir.path(),
        Agent::Pi,
        BuildLaunchPlanOptions {
            browser_mode: true,
            payload_fd_count: 2,
            bootstrap_path: Some(ONEPASSWORD_BOOTSTRAP_CONTAINER_PATH),
            bootstrap_host_path: Some(&bootstrap),
            ..options(&secrets)
        },
    )
    .unwrap();

    assert_eq!(
        plan.bootstrap_path.as_deref(),
        Some(ONEPASSWORD_BOOTSTRAP_CONTAINER_PATH)
    );
    let bootstrap_mount = plan
        .mounts
        .iter()
        .find(|mount| mount.container == ONEPASSWORD_BOOTSTRAP_CONTAINER_PATH)
        .expect("payload-enabled plans mount the embedded bootstrap");
    assert_eq!(bootstrap_mount.mode, MountMode::Ro);
    assert_eq!(
        fs::read_to_string(&bootstrap_mount.host).unwrap(),
        ags::assets::ONEPASSWORD_BOOTSTRAP
    );
    assert!(
        plan.entrypoint
            .contains("exec /run/ags/onepassword-bootstrap --fd-count 2 -- /usr/local/pnpm/pi")
    );
    assert!(
        plan.entrypoint
            .contains("(for fd in $(seq 3 4); do eval \"exec $fd>&-\"; done; exec socat")
    );
}

#[test]
fn webview_relay_child_closes_payload_descriptors_before_exec() {
    let root = tempfile::tempdir().unwrap();
    let workdir = tempfile::tempdir().unwrap();
    let config = parse_toml_str(&config_toml(root.path()), Path::new("/test/config.toml")).unwrap();
    let secrets = HashMap::new();
    let bootstrap = bootstrap_asset(root.path());
    let plan = build_launch_plan(
        &config,
        workdir.path(),
        Agent::Pi,
        BuildLaunchPlanOptions {
            payload_fd_count: 2,
            bootstrap_path: Some(ONEPASSWORD_BOOTSTRAP_CONTAINER_PATH),
            bootstrap_host_path: Some(&bootstrap),
            webview_relay_runtime_dir: Some(root.path()),
            ..options(&secrets)
        },
    )
    .unwrap();

    assert!(plan.entrypoint.contains(
        "(for fd in $(seq 3 4); do eval \"exec $fd>&-\"; done; exec python3 /run/ags-webview-relay/webview-relay-shim"
    ));
}

#[test]
fn prebootstrap_commands_close_payload_descriptors() {
    let root = tempfile::tempdir().unwrap();
    let workdir = tempfile::tempdir().unwrap();
    let config = parse_toml_str(&config_toml(root.path()), Path::new("/test/config.toml")).unwrap();
    let secrets = HashMap::new();
    let bootstrap = bootstrap_asset(root.path());
    for agent in [Agent::Claude, Agent::Shell] {
        let plan = build_launch_plan(
            &config,
            workdir.path(),
            agent,
            BuildLaunchPlanOptions {
                payload_fd_count: 1,
                bootstrap_path: Some(ONEPASSWORD_BOOTSTRAP_CONTAINER_PATH),
                bootstrap_host_path: Some(&bootstrap),
                ..options(&secrets)
            },
        )
        .unwrap();
        assert!(
            plan.entrypoint
                .contains("(for fd in $(seq 3 3); do eval \"exec $fd>&-\"; done;"),
            "{agent} setup must run in an FD-closing subshell"
        );
    }
}

#[test]
fn bootstrap_wraps_tmux_process_tree_in_root_stop_when_done_mode() {
    let root = tempfile::tempdir().unwrap();
    let workdir = tempfile::tempdir().unwrap();
    let config = parse_toml_str(&config_toml(root.path()), Path::new("/test/config.toml")).unwrap();
    let secrets = HashMap::new();
    let bootstrap = bootstrap_asset(root.path());
    let plan = build_launch_plan(
        &config,
        workdir.path(),
        Agent::Pi,
        BuildLaunchPlanOptions {
            tmux_mode: true,
            stop_when_done: true,
            root_mode: true,
            payload_fd_count: 1,
            bootstrap_path: Some(ONEPASSWORD_BOOTSTRAP_CONTAINER_PATH),
            bootstrap_host_path: Some(&bootstrap),
            ..options(&secrets)
        },
    )
    .unwrap();

    assert!(
        plan.entrypoint
            .contains("exec /run/ags/onepassword-bootstrap --fd-count 1 -- tmux new-session")
    );
    assert!(
        plan.entrypoint
            .contains("#!/usr/bin/env bash\nexec /usr/local/pnpm/pi")
    );
    assert!(
        !plan.entrypoint.contains("--user=root"),
        "security belongs to Podman args"
    );
    assert_eq!(plan.security.user.as_deref(), Some("root"));
}
