use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use crate::config::{
    BrowserConfig, MountKind, MountMode, MountWhen, ValidatedConfig, ValidatedMount,
};
use crate::git;
use crate::plan::types::*;

// Container-side path constants (pi agent image layout).
const CONTAINER_HOME: &str = "/home/dev";
const CONTAINER_PI_DIR: &str = "/home/dev/.pi/agent";
const CONTAINER_GITCONFIG: &str = "/home/dev/.config/pi-sandbox/gitconfig";
const CONTAINER_SSH_SOCK: &str = "/ssh-agent";

/// Cache volume mappings: (host_suffix under cache_dir, container_path, env_var).
const CACHE_MOUNTS: &[(&str, &str, &str)] = &[
    ("pnpm-home", "/home/dev/.local/share/pnpm", "PNPM_HOME"),
    ("cargo-home", "/home/dev/.cargo", "CARGO_HOME"),
    ("go-path", "/home/dev/go", "GOPATH"),
    ("go-build", "/home/dev/.cache/go-build", "GOCACHE"),
    ("sccache", "/home/dev/.cache/sccache", "SCCACHE_DIR"),
    ("cachepot", "/home/dev/.cache/cachepot", "CACHEPOT_DIR"),
];

/// Build a complete launch plan from validated config and runtime context.
pub fn build_launch_plan(
    config: &ValidatedConfig,
    workdir: &Path,
    browser_mode: bool,
    ssh_auth_sock: Option<&Path>,
    resolved_secrets: &HashMap<String, String>,
) -> Result<LaunchPlan, PlanError> {
    let workdir_mapping = resolve_workdir(workdir)?;
    let cache_dir = &config.sandbox.cache_dir;

    // Ensure host directories exist
    ensure_dir(cache_dir)?;
    ensure_dir(&config.sandbox.sandbox_pi_dir.join("extensions"))?;
    for (suffix, _, _) in CACHE_MOUNTS {
        ensure_dir(&cache_dir.join(suffix))?;
    }

    let mut mounts = Vec::new();
    let mut read_roots = vec![
        workdir_mapping.container.clone(),
        "/tmp".to_owned(),
        CONTAINER_PI_DIR.to_owned(),
    ];
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
        let path_str = path.to_string_lossy().to_string();
        mounts.push(PlanMount {
            host: path.clone(),
            container: path_str.clone(),
            mode: MountMode::Rw,
        });
        read_roots.push(path_str.clone());
        write_roots.push(path_str);
    }

    // Config mounts (filtered by when, optional/create handled)
    expand_config_mounts(
        &config.mounts,
        browser_mode,
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

    // Public key files
    add_pub_key_mount(&mut mounts, &config.sandbox.auth_key, "pi-agent-auth");
    add_pub_key_mount(&mut mounts, &config.sandbox.sign_key, "pi-agent-signing");

    // Environment
    let env = build_env(
        config,
        &wayland,
        &read_roots,
        &write_roots,
        resolved_secrets,
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
        &config.browser,
        browser_mode,
    );

    Ok(LaunchPlan {
        image: config.sandbox.image.clone(),
        containerfile: config.sandbox.containerfile.clone(),
        workdir: workdir_mapping,
        mounts,
        env,
        security: SecurityConfig::default(),
        network_mode,
        boot_dirs: config.sandbox.container_boot_dirs.clone(),
        entrypoint,
    })
}

// --- workdir ---

fn resolve_workdir(workdir: &Path) -> Result<WorkdirMapping, PlanError> {
    let host = fs::canonicalize(workdir)
        .map_err(|e| PlanError::WorkdirResolve(format!("{}: {e}", workdir.display())))?;
    // If caller passed an absolute path, preserve it as the container workdir;
    // otherwise use the resolved path.
    let container = if workdir.is_absolute() {
        workdir.to_string_lossy().to_string()
    } else {
        host.to_string_lossy().to_string()
    };
    Ok(WorkdirMapping { host, container })
}

// --- directory helpers ---

fn ensure_dir(path: &Path) -> Result<(), PlanError> {
    fs::create_dir_all(path).map_err(|e| PlanError::DirCreate {
        path: path.to_owned(),
        source: e,
    })
}

fn create_mount_host(path: &Path, kind: MountKind) -> Result<(), PlanError> {
    match kind {
        MountKind::Dir => ensure_dir(path)?,
        MountKind::File => {
            if let Some(parent) = path.parent() {
                ensure_dir(parent)?;
            }
            if !path.exists() {
                fs::File::create(path).map_err(|e| PlanError::DirCreate {
                    path: path.to_owned(),
                    source: e,
                })?;
            }
        }
    }
    Ok(())
}

// --- mount assembly ---

fn add_infrastructure_mounts(
    mounts: &mut Vec<PlanMount>,
    config: &ValidatedConfig,
    cache_dir: &Path,
) {
    // Sandbox pi dir
    mounts.push(PlanMount {
        host: config.sandbox.sandbox_pi_dir.clone(),
        container: CONTAINER_PI_DIR.to_owned(),
        mode: MountMode::Rw,
    });

    // Gitconfig
    mounts.push(PlanMount {
        host: config.sandbox.gitconfig_path.clone(),
        container: CONTAINER_GITCONFIG.to_owned(),
        mode: MountMode::Ro,
    });

    // Cache volumes
    for (suffix, container_path, _) in CACHE_MOUNTS {
        mounts.push(PlanMount {
            host: cache_dir.join(suffix),
            container: container_path.to_string(),
            mode: MountMode::Rw,
        });
    }
}

fn expand_config_mounts(
    config_mounts: &[ValidatedMount],
    browser_mode: bool,
    mounts: &mut Vec<PlanMount>,
    read_roots: &mut Vec<String>,
    write_roots: &mut Vec<String>,
) -> Result<(), PlanError> {
    for m in config_mounts {
        // Filter by when
        if m.when == MountWhen::Browser && !browser_mode {
            continue;
        }

        // Check host path existence
        if !m.host.exists() {
            if m.create {
                create_mount_host(&m.host, m.kind)?;
            } else if m.optional {
                continue;
            } else {
                return Err(PlanError::MountMissing {
                    host: m.host.clone(),
                    context: m.source.clone(),
                });
            }
        }

        mounts.push(PlanMount {
            host: m.host.clone(),
            container: m.container.clone(),
            mode: m.mode,
        });

        read_roots.push(m.container.clone());
        if m.mode == MountMode::Rw {
            write_roots.push(m.container.clone());
        }
    }
    Ok(())
}

fn add_pub_key_mount(mounts: &mut Vec<PlanMount>, key_path: &Path, container_name: &str) {
    let mut pub_os = key_path.as_os_str().to_owned();
    pub_os.push(".pub");
    let pub_path = PathBuf::from(pub_os);

    let is_nonempty = pub_path
        .metadata()
        .map(|m| m.is_file() && m.len() > 0)
        .unwrap_or(false);

    if is_nonempty {
        mounts.push(PlanMount {
            host: pub_path,
            container: format!("{CONTAINER_HOME}/.ssh/{container_name}.pub"),
            mode: MountMode::Ro,
        });
    }
}

// --- wayland detection ---

struct WaylandInfo {
    socket_path: PathBuf,
    display_name: String,
}

fn detect_wayland() -> Result<Option<WaylandInfo>, PlanError> {
    if !clipboard_enabled()? {
        return Ok(None);
    }

    let runtime_dir = match std::env::var("XDG_RUNTIME_DIR") {
        Ok(d) if !d.is_empty() => d,
        _ => return Ok(None),
    };
    let display = match std::env::var("WAYLAND_DISPLAY") {
        Ok(d) if !d.is_empty() => d,
        _ => return Ok(None),
    };

    let socket_path = PathBuf::from(&runtime_dir).join(&display);

    #[cfg(unix)]
    {
        use std::os::unix::fs::FileTypeExt;
        let is_socket = socket_path
            .symlink_metadata()
            .map(|m| m.file_type().is_socket())
            .unwrap_or(false);
        if !is_socket {
            return Ok(None);
        }
    }
    #[cfg(not(unix))]
    if !socket_path.exists() {
        return Ok(None);
    }

    Ok(Some(WaylandInfo {
        socket_path,
        display_name: display,
    }))
}

fn clipboard_enabled() -> Result<bool, PlanError> {
    let raw = std::env::var("PI_SBOX_ENABLE_CLIPBOARD").unwrap_or_else(|_| "1".to_owned());
    match raw.to_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        _ => Err(PlanError::InvalidEnv {
            var: "PI_SBOX_ENABLE_CLIPBOARD".to_owned(),
            value: raw,
        }),
    }
}

// --- environment assembly ---

fn build_env(
    config: &ValidatedConfig,
    wayland: &Option<WaylandInfo>,
    read_roots: &[String],
    write_roots: &[String],
    resolved_secrets: &HashMap<String, String>,
) -> PlanEnv {
    let mut inline = vec![
        ("HOME".to_owned(), CONTAINER_HOME.to_owned()),
        (
            "PI_CODING_AGENT_DIR".to_owned(),
            CONTAINER_PI_DIR.to_owned(),
        ),
        (
            "GIT_CONFIG_GLOBAL".to_owned(),
            CONTAINER_GITCONFIG.to_owned(),
        ),
        ("SSH_AUTH_SOCK".to_owned(), CONTAINER_SSH_SOCK.to_owned()),
        ("RUSTUP_HOME".to_owned(), "/usr/local/rustup".to_owned()),
    ];

    for (_, container_path, env_var) in CACHE_MOUNTS {
        inline.push((env_var.to_string(), container_path.to_string()));
    }

    if let Some(w) = wayland {
        inline.push(("WAYLAND_DISPLAY".to_owned(), w.display_name.clone()));
        inline.push(("XDG_RUNTIME_DIR".to_owned(), "/tmp".to_owned()));
    }

    let passthrough_names = vec![
        "TERM".to_owned(),
        "COLORTERM".to_owned(),
        "EDITOR".to_owned(),
        "VISUAL".to_owned(),
    ];

    // Env file: resolved secrets + host passthrough vars
    let mut env_file_entries: Vec<(String, String)> = Vec::new();
    for (key, value) in resolved_secrets {
        env_file_entries.push((key.clone(), value.clone()));
    }
    for env_name in &config.sandbox.passthrough_env {
        if resolved_secrets.contains_key(env_name) {
            continue;
        }
        if let Ok(val) = std::env::var(env_name)
            && !val.is_empty()
        {
            env_file_entries.push((env_name.clone(), val));
        }
    }

    PlanEnv {
        inline,
        passthrough_names,
        env_file_entries,
        read_roots_json: json_string_array(read_roots),
        write_roots_json: json_string_array(write_roots),
    }
}

// --- entrypoint ---

fn build_entrypoint(boot_dirs: &[String], browser: &BrowserConfig, browser_mode: bool) -> String {
    let mut script = String::new();

    if !boot_dirs.is_empty() {
        let dirs: Vec<String> = boot_dirs.iter().map(|d| shell_quote(d)).collect();
        script.push_str(&format!("mkdir -p {}; ", dirs.join(" ")));
    }

    if browser_mode && browser.enabled {
        script.push_str(&format!(
            "socat TCP-LISTEN:{port},fork,reuseaddr,bind=127.0.0.1 \
             TCP:10.0.2.2:{port} >/tmp/pi-sbox-socat.log 2>&1 & ",
            port = browser.debug_port
        ));
    }

    script.push_str("exec pi --no-extensions -e /home/dev/.pi/agent/extensions/guard.ts");

    if browser_mode && browser.enabled && !browser.pi_skill_path.is_empty() {
        script.push_str(&format!(" --skill {}", shell_quote(&browser.pi_skill_path)));
    }

    script.push_str(" \"$@\"");
    script
}

fn shell_quote(s: &str) -> String {
    if s.bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'/' | b'.' | b'-' | b'_'))
    {
        s.to_owned()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

// --- JSON helpers ---

fn json_string_array(items: &[String]) -> String {
    let mut unique: Vec<&str> = items.iter().map(|s| s.as_str()).collect();
    unique.sort();
    unique.dedup();
    unique.retain(|s| !s.is_empty());
    let escaped: Vec<String> = unique
        .iter()
        .map(|s| format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"")))
        .collect();
    format!("[{}]", escaped.join(","))
}
