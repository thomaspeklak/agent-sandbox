use std::collections::HashMap;
use std::ffi::OsString;
use std::fs;
use std::path::Path;
use std::sync::{Mutex, OnceLock};

use ags::cli::Agent;
use ags::config::{ClipboardMode, MountMode, parse_toml_str};
use ags::plan::{BuildLaunchPlanOptions, LaunchPlan, PlanError, build_launch_plan};

fn env_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

struct EnvVarGuard {
    name: &'static str,
    original: Option<OsString>,
}

impl EnvVarGuard {
    fn set(name: &'static str, value: impl AsRef<std::ffi::OsStr>) -> Self {
        let original = std::env::var_os(name);
        unsafe { std::env::set_var(name, value) };
        Self { name, original }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        match self.original.take() {
            Some(value) => unsafe { std::env::set_var(self.name, value) },
            None => unsafe { std::env::remove_var(self.name) },
        }
    }
}

/// Look up an inline env var by key from a launch plan.
fn find_plan_env(plan: &LaunchPlan, key: &str) -> Option<String> {
    plan.env
        .inline
        .iter()
        .find(|(k, _)| k == key)
        .map(|(_, v)| v.clone())
}

/// Default `BuildLaunchPlanOptions` with all optional fields set to `None`/defaults.
/// Tests override only the fields they care about via struct update syntax.
fn default_options(secrets: &HashMap<String, String>) -> BuildLaunchPlanOptions<'_> {
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
        podman_network: None,
    }
}

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
    build_launch_plan(&config, workdir, agent, default_options(&secrets)).unwrap()
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
fn clipboard_bridge_mounts_socket_and_shims_when_enabled() {
    let toml = minimal_config_toml();
    let workdir = tempfile::tempdir().unwrap();
    let runtime = tempfile::tempdir().unwrap();
    fs::write(runtime.path().join("clipboard-shim"), "#!/bin/sh\n").unwrap();
    let config = parse_toml_str(&toml, Path::new("/test/config.toml")).unwrap();
    let secrets = HashMap::new();
    let plan = build_launch_plan(
        &config,
        workdir.path(),
        Agent::Pi,
        BuildLaunchPlanOptions {
            clipboard_runtime_dir: Some(runtime.path()),
            clipboard_mode: ClipboardMode::ReadWrite,
            ..default_options(&secrets)
        },
    )
    .unwrap();

    assert!(
        plan.mounts
            .iter()
            .any(|m| m.container == "/run/ags-clipboard")
    );
    assert!(
        plan.mounts
            .iter()
            .any(|m| m.container == "/home/dev/.local/bin/wl-paste")
    );
    assert!(
        plan.mounts
            .iter()
            .any(|m| m.container == "/home/dev/.local/bin/wl-copy")
    );
    assert_eq!(
        find_plan_env(&plan, "AGS_CLIPBOARD_SOCK"),
        Some("/run/ags-clipboard/clipboard.sock".to_owned())
    );
    assert_eq!(
        find_plan_env(&plan, "AGS_CLIPBOARD_MODE"),
        Some("readwrite".to_owned())
    );
    assert_eq!(
        find_plan_env(&plan, "XDG_SESSION_TYPE"),
        Some("wayland".to_owned())
    );
}

#[test]
#[cfg(unix)]
fn wayland_socket_is_not_mounted_without_explicit_passthrough() {
    use std::os::unix::net::UnixListener;

    let _guard = env_lock().lock().unwrap();
    let runtime = tempfile::tempdir().unwrap();
    let socket_path = runtime.path().join("wayland-test");
    let _listener = UnixListener::bind(&socket_path).unwrap();
    let _xdg = EnvVarGuard::set("XDG_RUNTIME_DIR", runtime.path());
    let _display = EnvVarGuard::set("WAYLAND_DISPLAY", "wayland-test");

    let toml = minimal_config_toml();
    let workdir = tempfile::tempdir().unwrap();
    let plan = build_plan_from(&toml, workdir.path());

    assert!(
        plan.mounts
            .iter()
            .all(|m| m.container != "/tmp/wayland-test")
    );
    assert!(find_plan_env(&plan, "WAYLAND_DISPLAY").is_none());
}

#[test]
#[cfg(unix)]
fn explicit_wayland_passthrough_mounts_compositor_socket() {
    use std::os::unix::net::UnixListener;

    let _guard = env_lock().lock().unwrap();
    let runtime = tempfile::tempdir().unwrap();
    let socket_path = runtime.path().join("wayland-test");
    let _listener = UnixListener::bind(&socket_path).unwrap();
    let _xdg = EnvVarGuard::set("XDG_RUNTIME_DIR", runtime.path());
    let _display = EnvVarGuard::set("WAYLAND_DISPLAY", "wayland-test");

    let toml = minimal_config_toml();
    let workdir = tempfile::tempdir().unwrap();
    let config = parse_toml_str(&toml, Path::new("/test/config.toml")).unwrap();
    let secrets = HashMap::new();
    let plan = build_launch_plan(
        &config,
        workdir.path(),
        Agent::Pi,
        BuildLaunchPlanOptions {
            wayland_passthrough: true,
            ..default_options(&secrets)
        },
    )
    .unwrap();

    assert!(
        plan.mounts
            .iter()
            .any(|m| m.container == "/tmp/wayland-test")
    );
    assert_eq!(
        find_plan_env(&plan, "WAYLAND_DISPLAY"),
        Some("wayland-test".to_owned())
    );
}

#[test]
fn env_has_required_inline_vars() {
    let toml = minimal_config_toml();
    let workdir = tempfile::tempdir().unwrap();
    let plan = build_plan_from(&toml, workdir.path());

    assert_eq!(find_plan_env(&plan, "HOME"), Some("/home/dev".to_owned()));
    assert!(
        find_plan_env(&plan, "PI_CODING_AGENT_DIR").is_none(),
        "pi should not set PI_CODING_AGENT_DIR (uses $HOME/.pi/agent by default)"
    );
    assert_eq!(
        find_plan_env(&plan, "SSH_AUTH_SOCK"),
        Some("/ssh-agent".to_owned())
    );
    assert_eq!(find_plan_env(&plan, "AGS_SANDBOX"), Some("1".to_owned()));
    assert_eq!(
        find_plan_env(&plan, "NPM_CONFIG_STORE_DIR"),
        Some("/usr/local/pnpm/.store".to_owned())
    );
    assert_eq!(
        find_plan_env(&plan, "NPM_CONFIG_GLOBAL_BIN_DIR"),
        Some("/usr/local/pnpm".to_owned())
    );
    let path = find_plan_env(&plan, "PATH").expect("PATH should be set");
    assert!(
        path.find(":/usr/bin:").unwrap() < path.find(":/usr/local/pnpm:").unwrap(),
        "system pnpm should take precedence over stale pnpm shims in PNPM_HOME"
    );
    assert!(
        path.find(":/usr/local/pnpm:").unwrap() < path.find(":/home/dev/.npm-global/bin").unwrap(),
        "pnpm-managed agent shims should take precedence over stale npm-global agent shims"
    );
    assert_eq!(
        find_plan_env(&plan, "AGS_HOST_SERVICES_HOST"),
        Some("host.containers.internal".to_owned())
    );
    assert!(
        find_plan_env(&plan, "AGS_HOST_SERVICES_HINT")
            .is_some_and(|v| v.contains("localhost is container-local"))
    );
    assert!(find_plan_env(&plan, "PNPM_HOME").is_some());
    assert!(find_plan_env(&plan, "CARGO_HOME").is_some());
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
fn yolo_mode_sets_guard_escape_hatch_env() {
    let toml = minimal_config_toml();
    let workdir = tempfile::tempdir().unwrap();
    let config = parse_toml_str(&toml, Path::new("/test/config.toml")).unwrap();
    let secrets = HashMap::new();
    let plan = build_launch_plan(
        &config,
        workdir.path(),
        Agent::Pi,
        BuildLaunchPlanOptions {
            guard_enabled: false,
            ..default_options(&secrets)
        },
    )
    .unwrap();

    assert!(
        plan.env
            .inline
            .iter()
            .any(|(k, v)| k == "AGS_GUARD_YOLO" && v == "1"),
        "yolo mode should export AGS_GUARD_YOLO=1"
    );
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

    assert_eq!(plan.security.userns.as_deref(), Some("keep-id"));
    assert_eq!(plan.security.cap_drop.as_deref(), Some("all"));
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
fn root_mode_security_config() {
    let toml = minimal_config_toml();
    let workdir = tempfile::tempdir().unwrap();
    let config = parse_toml_str(&toml, Path::new("/test/config.toml")).unwrap();
    let secrets = HashMap::new();
    let plan = build_launch_plan(
        &config,
        workdir.path(),
        Agent::Claude,
        BuildLaunchPlanOptions {
            root_mode: true,
            ..default_options(&secrets)
        },
    )
    .unwrap();

    assert!(
        plan.security.userns.is_none(),
        "root mode should not set userns"
    );
    assert_eq!(
        plan.security.user.as_deref(),
        Some("root"),
        "root mode should set user to root"
    );
    assert!(
        plan.security.cap_drop.is_none(),
        "root mode should not drop capabilities"
    );
    assert!(
        !plan
            .security
            .security_opts
            .contains(&"no-new-privileges".to_owned()),
        "root mode should allow new privileges"
    );
    assert!(
        plan.security
            .security_opts
            .contains(&"label=disable".to_owned()),
        "root mode should still disable SELinux labels"
    );
}

#[test]
fn network_mode_without_browser() {
    let toml = minimal_config_toml();
    let workdir = tempfile::tempdir().unwrap();
    let plan = build_plan_from(&toml, workdir.path());
    assert_eq!(plan.network_mode, "pasta");
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
        BuildLaunchPlanOptions {
            browser_mode: true,
            ..default_options(&secrets)
        },
    )
    .unwrap();
    assert_eq!(plan.network_mode, "pasta:--map-host-loopback=169.254.1.2");
    assert!(
        plan.entrypoint
            .contains("TCP:host.containers.internal:9222"),
        "pasta browser bridge should use Podman host alias: {}",
        plan.entrypoint
    );
}

#[test]
fn network_mode_can_use_slirp4netns_from_config() {
    let toml = minimal_config_toml().replace(
        "containerfile =",
        "podman_network = \"slirp4netns\"\ncontainerfile =",
    );
    let workdir = tempfile::tempdir().unwrap();
    let plan = build_plan_from(&toml, workdir.path());
    assert_eq!(plan.network_mode, "slirp4netns:allow_host_loopback=false");
}

#[test]
fn network_mode_slirp_browser_preserves_host_loopback_bridge() {
    let toml = format!(
        "{}\n{}",
        minimal_config_toml().replace(
            "containerfile =",
            "podman_network = \"slirp4netns\"\ncontainerfile =",
        ),
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
        BuildLaunchPlanOptions {
            browser_mode: true,
            ..default_options(&secrets)
        },
    )
    .unwrap();

    assert_eq!(plan.network_mode, "slirp4netns:allow_host_loopback=true");
    assert!(
        plan.entrypoint.contains("TCP:10.0.2.2:9222"),
        "slirp browser bridge should use old slirp gateway: {}",
        plan.entrypoint
    );
}

#[test]
fn network_mode_cli_override_wins() {
    let toml = minimal_config_toml();
    let workdir = tempfile::tempdir().unwrap();
    let config = parse_toml_str(&toml, Path::new("/test/config.toml")).unwrap();
    let secrets = HashMap::new();
    let plan = build_launch_plan(
        &config,
        workdir.path(),
        Agent::Pi,
        BuildLaunchPlanOptions {
            podman_network: Some(ags::network::PodmanNetwork::Slirp4netns),
            ..default_options(&secrets)
        },
    )
    .unwrap();
    assert_eq!(plan.network_mode, "slirp4netns:allow_host_loopback=false");
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
    assert!(plan.entrypoint.contains("exec /usr/local/pnpm/pi -e"));
    assert!(
        !plan.entrypoint.contains("--no-extensions"),
        "pi should not disable extensions: {}",
        plan.entrypoint
    );
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
        BuildLaunchPlanOptions {
            tmux_mode: true,
            ..default_options(&secrets)
        },
    )
    .unwrap();

    assert!(plan.entrypoint.contains("command -v tmux"));
    assert!(plan.entrypoint.contains("Run `ags update-image`"));
    assert!(plan.entrypoint.contains("/tmp/ags-run-in-tmux.sh"));
    assert!(plan.entrypoint.contains("exec tmux new-session -A -s ags"));
    // Default (stop_when_done=false): agent runs without exec, shell kept alive
    assert!(
        plan.entrypoint.contains("pi -e"),
        "entrypoint should contain agent command: {}",
        plan.entrypoint
    );
    assert!(
        plan.entrypoint.contains("AGS_EXIT=$?"),
        "entrypoint should capture exit code: {}",
        plan.entrypoint
    );
    assert!(
        plan.entrypoint.contains("exec bash"),
        "entrypoint should drop to shell: {}",
        plan.entrypoint
    );
    assert!(
        !plan.entrypoint.contains("--no-extensions"),
        "pi should not disable extensions in tmux mode: {}",
        plan.entrypoint
    );
}

#[test]
fn tmux_stop_when_done_uses_exec() {
    let toml = minimal_config_toml();
    let workdir = tempfile::tempdir().unwrap();
    let config = parse_toml_str(&toml, Path::new("/test/config.toml")).unwrap();
    let secrets = HashMap::new();
    let plan = build_launch_plan(
        &config,
        workdir.path(),
        Agent::Pi,
        BuildLaunchPlanOptions {
            tmux_mode: true,
            stop_when_done: true,
            ..default_options(&secrets)
        },
    )
    .unwrap();

    assert!(
        plan.entrypoint.contains("exec /usr/local/pnpm/pi -e"),
        "stop_when_done should exec the agent: {}",
        plan.entrypoint
    );
    assert!(
        !plan.entrypoint.contains("AGS_EXIT"),
        "stop_when_done should not capture exit code: {}",
        plan.entrypoint
    );
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
        default_options(&secrets),
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
        default_options(&secrets),
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
        BuildLaunchPlanOptions {
            ssh_auth_sock: Some(sock),
            ..default_options(&secrets)
        },
    )
    .unwrap();

    let found = plan.mounts.iter().any(|m| m.container == "/ssh-agent");
    assert!(found, "SSH socket should be mounted");
}

#[test]
fn runtime_add_dir_mounts_are_included() {
    let toml = minimal_config_toml();
    let workdir = tempfile::tempdir().unwrap();
    let extra_dir = tempfile::tempdir().unwrap();
    let config = parse_toml_str(&toml, Path::new("/test/config.toml")).unwrap();
    let secrets = HashMap::new();
    let extra_dirs = vec![extra_dir.path().to_path_buf()];
    let plan = build_launch_plan(
        &config,
        workdir.path(),
        Agent::Pi,
        BuildLaunchPlanOptions {
            extra_mount_dirs: &extra_dirs,
            ..default_options(&secrets)
        },
    )
    .unwrap();

    let extra = extra_dir.path().to_string_lossy().to_string();
    assert!(plan.mounts.iter().any(|m| m.container == extra));
    assert!(plan.env.read_roots_json.contains(&extra));
    assert!(plan.env.write_roots_json.contains(&extra));
}

#[test]
fn runtime_add_dir_missing_path_is_error() {
    let toml = minimal_config_toml();
    let workdir = tempfile::tempdir().unwrap();
    let config = parse_toml_str(&toml, Path::new("/test/config.toml")).unwrap();
    let secrets = HashMap::new();
    let extra_dirs = vec![Path::new("/definitely/missing/ags-extra-dir").to_path_buf()];
    let result = build_launch_plan(
        &config,
        workdir.path(),
        Agent::Pi,
        BuildLaunchPlanOptions {
            extra_mount_dirs: &extra_dirs,
            ..default_options(&secrets)
        },
    );
    assert!(matches!(result, Err(PlanError::MountMissing { .. })));
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
        default_options(&secrets),
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
        BuildLaunchPlanOptions {
            browser_mode: true,
            ..default_options(&secrets)
        },
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
        plan.entrypoint
            .contains("--plugin-dir /home/dev/.config/ags/hooks"),
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

    assert!(
        find_plan_env(&plan, "CLAUDE_CONFIG_DIR").is_none(),
        "claude should not set CLAUDE_CONFIG_DIR (uses $HOME/.claude by default)"
    );
    assert!(
        find_plan_env(&plan, "PI_CODING_AGENT_DIR").is_none(),
        "claude should not have PI_CODING_AGENT_DIR"
    );
}

#[test]
fn claude_agent_entrypoint_setup() {
    let toml = minimal_config_toml();
    let workdir = tempfile::tempdir().unwrap();
    let plan = build_plan_from_agent(&toml, workdir.path(), Agent::Claude);

    // Guard skill is loaded via --plugin-dir.
    assert!(
        plan.entrypoint.contains("--plugin-dir"),
        "claude entrypoint should have --plugin-dir for guard skill: {}",
        plan.entrypoint
    );
    // Binary symlinked from persistent install dir so Claude's native-install
    // self-check finds the binary at $HOME/.local/bin/claude.
    assert!(
        plan.entrypoint
            .contains("ln -sf /opt/claude-home/.local/bin/claude /home/dev/.local/bin/claude"),
        "claude entrypoint should symlink binary to prevent startup warning: {}",
        plan.entrypoint
    );
}

#[test]
fn codex_agent_entrypoint() {
    let toml = minimal_config_toml();
    let workdir = tempfile::tempdir().unwrap();
    let plan = build_plan_from_agent(&toml, workdir.path(), Agent::Codex);

    assert!(
        plan.entrypoint.contains("exec /usr/local/pnpm/codex"),
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
        plan.entrypoint.contains("exec /usr/local/pnpm/gemini"),
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
        plan.entrypoint.contains("exec /usr/local/pnpm/opencode"),
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

// --- PSP integration ---

#[test]
fn psp_mode_injects_docker_host_env() {
    let toml = minimal_config_toml();
    let workdir = tempfile::tempdir().unwrap();
    let psp_dir = tempfile::tempdir().unwrap();
    let psp_sock = psp_dir.path().join("psp.sock");

    let config = parse_toml_str(&toml, Path::new("/test/config.toml")).unwrap();
    let secrets = HashMap::new();
    let plan = build_launch_plan(
        &config,
        workdir.path(),
        Agent::Pi,
        BuildLaunchPlanOptions {
            psp_socket: Some(&psp_sock),
            psp_session_id: Some("ags-pi-12345"),
            ..default_options(&secrets)
        },
    )
    .unwrap();

    assert_eq!(
        find_plan_env(&plan, "DOCKER_HOST"),
        Some("unix:///run/psp/psp.sock".to_owned()),
        "DOCKER_HOST should point to container-side PSP socket"
    );
    assert_eq!(
        find_plan_env(&plan, "PSP_SESSION_ID"),
        Some("ags-pi-12345".to_owned()),
        "PSP_SESSION_ID should be injected"
    );
    assert_eq!(
        find_plan_env(&plan, "TESTCONTAINERS_HOST_OVERRIDE"),
        Some("host.containers.internal".to_owned()),
        "TESTCONTAINERS_HOST_OVERRIDE should route to host"
    );
}

#[test]
fn psp_mode_mounts_socket_dir() {
    let toml = minimal_config_toml();
    let workdir = tempfile::tempdir().unwrap();
    let psp_dir = tempfile::tempdir().unwrap();
    let psp_sock = psp_dir.path().join("psp.sock");

    let config = parse_toml_str(&toml, Path::new("/test/config.toml")).unwrap();
    let secrets = HashMap::new();
    let plan = build_launch_plan(
        &config,
        workdir.path(),
        Agent::Pi,
        BuildLaunchPlanOptions {
            psp_socket: Some(&psp_sock),
            psp_session_id: Some("ags-pi-12345"),
            ..default_options(&secrets)
        },
    )
    .unwrap();

    let psp_mount = plan.mounts.iter().find(|m| m.container == "/run/psp");
    assert!(psp_mount.is_some(), "PSP socket dir should be mounted");
    assert_eq!(psp_mount.unwrap().mode, MountMode::Rw);
}

#[test]
fn no_psp_env_when_disabled() {
    let toml = minimal_config_toml();
    let workdir = tempfile::tempdir().unwrap();
    let plan = build_plan_from(&toml, workdir.path());

    assert!(
        find_plan_env(&plan, "DOCKER_HOST").is_none(),
        "DOCKER_HOST should not be set without PSP"
    );
    assert!(
        find_plan_env(&plan, "PSP_SESSION_ID").is_none(),
        "PSP_SESSION_ID should not be set without PSP"
    );
    assert!(
        find_plan_env(&plan, "TESTCONTAINERS_HOST_OVERRIDE").is_none(),
        "TESTCONTAINERS_HOST_OVERRIDE should not be set without PSP"
    );
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
        BuildLaunchPlanOptions {
            auth_proxy_runtime_dir: Some(auth_dir.path()),
            ..default_options(&secrets)
        },
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
    assert_eq!(
        find_plan_env(&plan, "AGS_AUTH_PROXY_SOCK"),
        Some("/run/ags-auth-proxy/auth-proxy.sock".to_owned())
    );
    assert_eq!(
        find_plan_env(&plan, "BROWSER"),
        Some("/home/dev/.local/bin/auth-proxy-shim".to_owned())
    );
}

#[test]
fn no_auth_proxy_env_when_disabled() {
    let toml = minimal_config_toml();
    let workdir = tempfile::tempdir().unwrap();
    let plan = build_plan_from(&toml, workdir.path());

    assert!(
        find_plan_env(&plan, "AGS_AUTH_PROXY_SOCK").is_none(),
        "no auth proxy env when disabled"
    );
    assert!(
        find_plan_env(&plan, "BROWSER").is_none(),
        "BROWSER should not be set without auth proxy"
    );
}

#[test]
fn host_ui_mounts_runtime_dir_and_env_when_enabled() {
    let toml = minimal_config_toml();
    let workdir = tempfile::tempdir().unwrap();
    let runtime_dir = tempfile::tempdir().unwrap();

    let config = parse_toml_str(&toml, Path::new("/test/config.toml")).unwrap();
    let secrets = HashMap::new();
    let plan = build_launch_plan(
        &config,
        workdir.path(),
        Agent::Pi,
        BuildLaunchPlanOptions {
            host_ui_runtime_dir: Some(runtime_dir.path()),
            host_ui_session_id: Some("ags-pi-test"),
            ..default_options(&secrets)
        },
    )
    .unwrap();

    let runtime_mount = plan
        .mounts
        .iter()
        .find(|m| m.container == "/run/ags-host-ui");
    assert!(
        runtime_mount.is_some(),
        "host UI runtime dir should be mounted"
    );
    assert_eq!(runtime_mount.unwrap().mode, MountMode::Rw);

    assert_eq!(
        find_plan_env(&plan, "AGS_HOST_UI_SOCK"),
        Some("/run/ags-host-ui/host-ui.sock".to_owned())
    );
    assert_eq!(
        find_plan_env(&plan, "GLIMPSE_BINARY_PATH"),
        Some("/opt/ags/glimpse-shim".to_owned())
    );
    assert_eq!(
        find_plan_env(&plan, "AGS_HOST_UI_PROTOCOL"),
        Some("1".to_owned())
    );
    assert_eq!(
        find_plan_env(&plan, "AGS_HOST_UI_TRANSPORT"),
        Some("socket".to_owned())
    );
    assert_eq!(
        find_plan_env(&plan, "AGS_HOST_UI_SESSION_ID"),
        Some("ags-pi-test".to_owned())
    );
}

#[test]
fn webview_relay_mounts_helper_and_env_when_enabled() {
    let toml = minimal_config_toml();
    let workdir = tempfile::tempdir().unwrap();
    let relay_dir = tempfile::tempdir().unwrap();

    fs::write(
        relay_dir.path().join("webview-relay-shim"),
        "#!/usr/bin/env python3\n",
    )
    .unwrap();
    fs::write(
        relay_dir.path().join("ags-webview-url"),
        "#!/usr/bin/env python3\n",
    )
    .unwrap();

    let config = parse_toml_str(&toml, Path::new("/test/config.toml")).unwrap();
    let secrets = HashMap::new();
    let plan = build_launch_plan(
        &config,
        workdir.path(),
        Agent::Pi,
        BuildLaunchPlanOptions {
            webview_relay_runtime_dir: Some(relay_dir.path()),
            ..default_options(&secrets)
        },
    )
    .unwrap();

    let runtime_mount = plan
        .mounts
        .iter()
        .find(|m| m.container == "/run/ags-webview-relay");
    assert!(
        runtime_mount.is_some(),
        "webview relay runtime dir should be mounted"
    );
    assert_eq!(runtime_mount.unwrap().mode, MountMode::Rw);

    let helper_mount = plan
        .mounts
        .iter()
        .find(|m| m.container == "/home/dev/.local/bin/ags-webview-url");
    assert!(
        helper_mount.is_some(),
        "ags-webview-url helper should be mounted"
    );
    assert_eq!(helper_mount.unwrap().mode, MountMode::Ro);

    assert_eq!(
        find_plan_env(&plan, "AGS_WEBVIEW_RELAY_SOCKET"),
        Some("/run/ags-webview-relay/relay.sock".to_owned())
    );
    assert_eq!(
        find_plan_env(&plan, "AGS_WEBVIEW_RELAY_UPSTREAM_SOCKET"),
        Some("/run/ags-webview-relay/upstream.sock".to_owned())
    );
    assert_eq!(
        find_plan_env(&plan, "AGS_WEBVIEW_URL_HELPER"),
        Some("/home/dev/.local/bin/ags-webview-url".to_owned())
    );
}

#[test]
fn no_webview_relay_env_when_disabled() {
    let toml = minimal_config_toml();
    let workdir = tempfile::tempdir().unwrap();
    let plan = build_plan_from(&toml, workdir.path());

    assert!(find_plan_env(&plan, "AGS_HOST_UI_SOCK").is_none());
    assert!(find_plan_env(&plan, "GLIMPSE_BINARY_PATH").is_none());
    assert!(find_plan_env(&plan, "AGS_HOST_UI_PROTOCOL").is_none());
    assert!(find_plan_env(&plan, "AGS_HOST_UI_TRANSPORT").is_none());
    assert!(find_plan_env(&plan, "AGS_HOST_UI_SESSION_ID").is_none());
    assert!(find_plan_env(&plan, "AGS_WEBVIEW_RELAY_SOCKET").is_none());
    assert!(find_plan_env(&plan, "AGS_WEBVIEW_RELAY_UPSTREAM_SOCKET").is_none());
    assert!(find_plan_env(&plan, "AGS_WEBVIEW_URL_HELPER").is_none());
}
