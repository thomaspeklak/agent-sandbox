use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::agent::{self, AgentProfile};
use crate::auth_proxy::host::AuthProxyGuard;
use crate::cli::Agent;
use crate::config::{
    BrowserConfig, MountKind, MountMode, MountWhen, ValidatedConfig, ValidatedMount,
};
use crate::git;
use crate::host_ui::HostUiGuard;
use crate::plan::types::*;
use crate::util::shell_quote;
use crate::webview_relay::WebviewRelayGuard;

// Container-side path constants.
const CONTAINER_HOME: &str = "/home/dev";
const CONTAINER_GITCONFIG: &str = "/home/dev/.config/ags/gitconfig";
const CONTAINER_SSH_SOCK: &str = "/ssh-agent";
const HOST_SERVICES_HOST: &str = "host.containers.internal";
const HOST_SERVICES_HINT: &str =
    "[ags] Host services: use host.containers.internal (localhost is container-local)";

pub struct BuildLaunchPlanOptions<'a> {
    pub browser_mode: bool,
    pub tmux_mode: bool,
    pub guard_enabled: bool,
    pub ssh_auth_sock: Option<&'a Path>,
    pub resolved_secrets: &'a HashMap<String, String>,
    pub auth_proxy_runtime_dir: Option<&'a Path>,
    pub host_ui_runtime_dir: Option<&'a Path>,
    pub host_ui_session_id: Option<&'a str>,
    pub webview_relay_runtime_dir: Option<&'a Path>,
    pub psp_socket: Option<&'a Path>,
    pub psp_session_id: Option<&'a str>,
    pub extra_mount_dirs: &'a [PathBuf],
    pub stop_when_done: bool,
    pub root_mode: bool,
}

/// Intermediate env-assembly context. Sidecar fields mirror
/// [`BuildLaunchPlanOptions`] as `Option` references so `build_env` can derive
/// enabled-flags via `.is_some()` rather than receiving pre-computed booleans.
struct BuildEnvContext<'a> {
    wayland: &'a Option<WaylandInfo>,
    read_roots: &'a [String],
    write_roots: &'a [String],
    resolved_secrets: &'a HashMap<String, String>,
    auth_proxy_runtime_dir: Option<&'a Path>,
    host_ui_runtime_dir: Option<&'a Path>,
    host_ui_session_id: Option<&'a str>,
    webview_relay_runtime_dir: Option<&'a Path>,
    psp_socket: Option<&'a Path>,
    psp_session_id: Option<&'a str>,
    guard_enabled: bool,
}

/// Cache volume mappings: (host_suffix under cache_dir, container_path, env_var).
/// An empty env_var means no environment variable is emitted for that mount.
const CACHE_MOUNTS: &[(&str, &str, &str)] = &[
    ("pnpm-home", "/usr/local/pnpm", "PNPM_HOME"),
    ("claude-install", "/opt/claude-home", ""),
    ("cargo-home", "/home/dev/.cargo", "CARGO_HOME"),
    ("go-path", "/home/dev/go", "GOPATH"),
    ("go-build", "/home/dev/.cache/go-build", "GOCACHE"),
    ("sccache", "/home/dev/.cache/sccache", "SCCACHE_DIR"),
    ("cachepot", "/home/dev/.cache/cachepot", "CACHEPOT_DIR"),
    ("ags-hooks", "/home/dev/.config/ags/hooks", ""),
    ("npm-global", "/home/dev/.npm-global", "NPM_CONFIG_PREFIX"),
];

/// Build a complete launch plan from validated config and runtime context.
pub fn build_launch_plan(
    config: &ValidatedConfig,
    workdir: &Path,
    agent: Agent,
    options: BuildLaunchPlanOptions<'_>,
) -> Result<LaunchPlan, PlanError> {
    let BuildLaunchPlanOptions {
        browser_mode,
        tmux_mode,
        guard_enabled,
        ssh_auth_sock,
        resolved_secrets,
        auth_proxy_runtime_dir,
        host_ui_runtime_dir,
        host_ui_session_id,
        webview_relay_runtime_dir,
        psp_socket,
        psp_session_id,
        extra_mount_dirs,
        stop_when_done,
        root_mode,
    } = options;
    let profile = agent::profile_for_with_guards(agent, config, guard_enabled, root_mode);
    let workdir_mapping = resolve_workdir(workdir)?;
    let container_name = build_container_name(&workdir_mapping.host);
    let cache_dir = &config.sandbox.cache_dir;

    // Ensure host directories exist
    ensure_dir(cache_dir)?;
    for (suffix, _, _) in CACHE_MOUNTS {
        ensure_dir(&cache_dir.join(suffix))?;
    }

    let mut mounts = Vec::new();
    let mut read_roots = vec![workdir_mapping.container.clone(), "/tmp".to_owned()];
    let mut write_roots = read_roots.clone();

    // Workdir mount (added first, rendered separately by podman builder as -v + -w)
    mounts.push(PlanMount {
        host: workdir_mapping.host.clone(),
        container: workdir_mapping.container.clone(),
        mode: MountMode::Rw,
    });

    // Infrastructure mounts
    add_infrastructure_mounts(&mut mounts, config, cache_dir);

    // Wayland clipboard
    let wayland = detect_wayland()?;
    if let Some(ref w) = wayland {
        mounts.push(PlanMount {
            host: w.socket_path.clone(),
            container: format!("/tmp/{}", w.display_name),
            mode: MountMode::Ro,
        });
    }

    // Git metadata mounts (external worktree/submodule dirs)
    let git_mounts = git::discover_external_git_mounts(&workdir_mapping.host);
    for path in &git_mounts.paths {
        let container = path.to_string_lossy().to_string();
        mounts.push(PlanMount {
            host: path.clone(),
            container: container.clone(),
            mode: MountMode::Rw,
        });
        read_roots.push(container.clone());
        write_roots.push(container);
    }

    // Config mounts (filtered by when, optional/create handled)
    expand_config_mounts(
        &config.mounts,
        browser_mode,
        &mut mounts,
        &mut read_roots,
        &mut write_roots,
    )?;

    // Extra runtime directory mounts from CLI flags.
    add_runtime_dir_mounts(
        extra_mount_dirs,
        &mut mounts,
        &mut read_roots,
        &mut write_roots,
    )?;

    // SSH agent socket
    if let Some(sock) = ssh_auth_sock {
        mounts.push(PlanMount {
            host: sock.to_owned(),
            container: CONTAINER_SSH_SOCK.to_owned(),
            mode: MountMode::Rw,
        });
    }

    // Auth proxy runtime dir + shim
    if let Some(runtime_dir) = auth_proxy_runtime_dir {
        mounts.push(PlanMount {
            host: runtime_dir.to_owned(),
            container: AuthProxyGuard::container_runtime_dir().to_owned(),
            mode: MountMode::Rw,
        });

        // Mount the shim script into the container
        let shim_host = runtime_dir.join("auth-proxy-shim");
        let shim_container = format!("{CONTAINER_HOME}/.local/bin/auth-proxy-shim");
        mounts.push(PlanMount {
            host: shim_host,
            container: shim_container,
            mode: MountMode::Ro,
        });
    }

    // Host UI service runtime dir mount
    if let Some(runtime_dir) = host_ui_runtime_dir {
        mounts.push(PlanMount {
            host: runtime_dir.to_owned(),
            container: HostUiGuard::container_runtime_dir().to_owned(),
            mode: MountMode::Rw,
        });
    }

    // Webview relay runtime dir + helper for sandbox-local app servers
    if let Some(runtime_dir) = webview_relay_runtime_dir {
        mounts.push(PlanMount {
            host: runtime_dir.to_owned(),
            container: WebviewRelayGuard::container_runtime_dir().to_owned(),
            mode: MountMode::Rw,
        });

        let helper_host = runtime_dir.join("ags-webview-url");
        let helper_container = format!("{CONTAINER_HOME}/.local/bin/ags-webview-url");
        mounts.push(PlanMount {
            host: helper_host,
            container: helper_container,
            mode: MountMode::Ro,
        });
    }

    // PSP socket mount
    if let Some(sock) = psp_socket
        && let Some(sock_dir) = sock.parent()
    {
        mounts.push(PlanMount {
            host: sock_dir.to_owned(),
            container: crate::psp::PspGuard::container_socket_dir().to_owned(),
            mode: MountMode::Rw,
        });
    }

    // Public key files
    add_pub_key_mount(&mut mounts, &config.sandbox.auth_key, "ags-agent-auth");
    add_pub_key_mount(&mut mounts, &config.sandbox.sign_key, "ags-agent-signing");

    // Environment
    let env = build_env(
        config,
        &profile,
        BuildEnvContext {
            wayland: &wayland,
            read_roots: &read_roots,
            write_roots: &write_roots,
            resolved_secrets,
            auth_proxy_runtime_dir,
            host_ui_runtime_dir,
            host_ui_session_id,
            webview_relay_runtime_dir,
            psp_socket,
            psp_session_id,
            guard_enabled,
        },
    );

    // Network mode
    let network_mode = if browser_mode {
        "slirp4netns:allow_host_loopback=true"
    } else {
        "slirp4netns:allow_host_loopback=false"
    }
    .to_owned();

    // Entrypoint bash script
    let entrypoint = build_entrypoint(
        &config.sandbox.container_boot_dirs,
        &profile,
        &config.browser,
        browser_mode,
        tmux_mode,
        webview_relay_runtime_dir.is_some(),
        stop_when_done,
    );

    Ok(LaunchPlan {
        image: config.sandbox.image.clone(),
        containerfile: config.sandbox.containerfile.clone(),
        container_name,
        workdir: workdir_mapping,
        mounts,
        env,
        security: if root_mode {
            SecurityConfig::root()
        } else {
            SecurityConfig::default()
        },
        network_mode,
        boot_dirs: config.sandbox.container_boot_dirs.clone(),
        entrypoint,
    })
}

// --- workdir ---

include!("build_workdir.rs");
include!("build_env.rs");
