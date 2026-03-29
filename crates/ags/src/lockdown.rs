use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use tempfile::TempDir;

use crate::assets;
use crate::cli::{Agent, RunOptions};
use crate::config::{MountMode, ValidatedConfig};
use crate::plan::PlanMount;

const COMMON_HOME_FILES: &[&str] = &[
    "auth.json",
    "config.json",
    "config.toml",
    "credentials.json",
    "models.json",
    "settings.json",
    "settings.local.json",
];
const COMMON_HOME_DIRS: &[&str] = &[
    "agent",
    "commands",
    "extensions",
    "packages",
    "plugins",
    "prompts",
    "skills",
    "templates",
    "themes",
];
const CLAUDE_HOME_FILES: &[&str] = &[
    ".credentials.json",
    ".mcp.json",
    "CLAUDE.md",
    "keybindings.json",
];
const CLAUDE_HOME_DIRS: &[&str] = &["agents", "mcp", "shared", "teams", "tools"];
const PNPM_RUNTIME_CONTAINER: &str = "/usr/local/pnpm";
const CLAUDE_RUNTIME_CONTAINER: &str = "/opt/claude-home";
const CLAUDE_GUARD_CONTAINER: &str = "/run/ags-claude-hooks";

#[derive(Debug)]
pub enum LockdownError {
    IncompatibleFlag(&'static str),
    MissingAgentMount {
        agent: Agent,
        container: &'static str,
    },
    MissingRuntime {
        path: PathBuf,
        context: &'static str,
    },
    StageIo {
        path: PathBuf,
        source: std::io::Error,
    },
}

impl fmt::Display for LockdownError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::IncompatibleFlag(flag) => {
                write!(f, "--lockdown cannot be combined with {flag}")
            }
            Self::MissingAgentMount { agent, container } => write!(
                f,
                "--lockdown requires a configured {} mount for {}",
                agent.as_str(),
                container
            ),
            Self::MissingRuntime { path, context } => write!(
                f,
                "--lockdown requires runtime staging source for {context}: {}",
                path.display()
            ),
            Self::StageIo { path, source } => {
                write!(
                    f,
                    "lockdown staging failed for {}: {source}",
                    path.display()
                )
            }
        }
    }
}

impl std::error::Error for LockdownError {}

pub struct LockdownSession {
    _tempdir: TempDir,
    pub extra_mounts: Vec<PlanMount>,
}

pub fn validate(opts: &RunOptions) -> Result<(), LockdownError> {
    if !opts.lockdown {
        return Ok(());
    }
    for (enabled, flag) in [
        (opts.browser, "--browser"),
        (opts.psp, "--psp"),
        (opts.psp_keep, "--psp-keep"),
        (opts.root, "--root"),
    ] {
        if enabled {
            return Err(LockdownError::IncompatibleFlag(flag));
        }
    }
    Ok(())
}

pub fn prepare(
    agent: Agent,
    config: &ValidatedConfig,
    guard_enabled: bool,
) -> Result<LockdownSession, LockdownError> {
    let tempdir = tempfile::Builder::new()
        .prefix(&format!("ags-lockdown-{}-", agent.as_str()))
        .tempdir()
        .map_err(|source| LockdownError::StageIo {
            path: std::env::temp_dir(),
            source,
        })?;
    let stage_root = tempdir.path();
    let mut extra_mounts = Vec::new();

    stage_agent_runtime(agent, config, guard_enabled, stage_root, &mut extra_mounts)?;
    stage_agent_home(agent, config, stage_root, &mut extra_mounts)?;

    Ok(LockdownSession {
        _tempdir: tempdir,
        extra_mounts,
    })
}

fn stage_agent_runtime(
    agent: Agent,
    config: &ValidatedConfig,
    guard_enabled: bool,
    stage_root: &Path,
    extra_mounts: &mut Vec<PlanMount>,
) -> Result<(), LockdownError> {
    match agent {
        Agent::Pi | Agent::Codex | Agent::Gemini | Agent::Opencode => {
            let src = config.sandbox.cache_dir.join("pnpm-home");
            stage_runtime_mount(&src, PNPM_RUNTIME_CONTAINER, stage_root, extra_mounts)?;
        }
        Agent::Claude => {
            let src = config.sandbox.cache_dir.join("claude-install");
            stage_runtime_mount(&src, CLAUDE_RUNTIME_CONTAINER, stage_root, extra_mounts)?;
            if guard_enabled {
                stage_claude_guard_mount(stage_root, extra_mounts).map_err(|source| {
                    LockdownError::StageIo {
                        path: stage_path(stage_root, CLAUDE_GUARD_CONTAINER),
                        source,
                    }
                })?;
            }
        }
        Agent::Shell => {}
    }
    Ok(())
}

fn stage_runtime_mount(
    src: &Path,
    container: &'static str,
    stage_root: &Path,
    extra_mounts: &mut Vec<PlanMount>,
) -> Result<(), LockdownError> {
    let src = canonical_source(src).ok_or_else(|| LockdownError::MissingRuntime {
        path: src.to_path_buf(),
        context: container,
    })?;
    let dst = stage_path(stage_root, container);
    copy_tree_with_symlinks(&src, &dst).map_err(|source| LockdownError::StageIo {
        path: dst.clone(),
        source,
    })?;
    extra_mounts.push(PlanMount {
        host: dst,
        container: container.to_string(),
        mode: MountMode::Ro,
    });
    Ok(())
}

fn stage_agent_home(
    agent: Agent,
    config: &ValidatedConfig,
    stage_root: &Path,
    extra_mounts: &mut Vec<PlanMount>,
) -> Result<(), LockdownError> {
    for container in home_containers(agent) {
        let src = config
            .mount_host_for_container(container)
            .ok_or(LockdownError::MissingAgentMount { agent, container })?;
        let src =
            canonical_source(src).ok_or(LockdownError::MissingAgentMount { agent, container })?;
        let dst = stage_path(stage_root, container);
        if src.is_file() {
            copy_regular_file(&src, &dst).map_err(|source| LockdownError::StageIo {
                path: dst.clone(),
                source,
            })?;
        } else {
            stage_filtered_home_dir(agent, &src, &dst).map_err(|source| {
                LockdownError::StageIo {
                    path: dst.clone(),
                    source,
                }
            })?;
        }
        ensure_staged_home_assets(agent, &dst).map_err(|source| LockdownError::StageIo {
            path: dst.clone(),
            source,
        })?;
        extra_mounts.push(PlanMount {
            host: dst,
            container: container.to_string(),
            mode: MountMode::Rw,
        });
    }
    Ok(())
}

fn home_containers(agent: Agent) -> &'static [&'static str] {
    match agent {
        Agent::Pi => &["/home/dev/.pi"],
        Agent::Claude => &["/home/dev/.claude.json", "/home/dev/.claude"],
        Agent::Codex => &["/home/dev/.codex"],
        Agent::Gemini => &["/home/dev/.gemini"],
        Agent::Opencode => &["/home/dev/.config/opencode"],
        Agent::Shell => &[],
    }
}

fn stage_claude_guard_mount(
    stage_root: &Path,
    extra_mounts: &mut Vec<PlanMount>,
) -> std::io::Result<()> {
    let dst = stage_path(stage_root, CLAUDE_GUARD_CONTAINER);
    assets::ensure_claude_guard_hook(&dst)?;
    assets::ensure_claude_guard_skill(&dst)?;
    extra_mounts.push(PlanMount {
        host: dst,
        container: CLAUDE_GUARD_CONTAINER.to_owned(),
        mode: MountMode::Ro,
    });
    Ok(())
}

fn ensure_staged_home_assets(agent: Agent, staged_home: &Path) -> std::io::Result<()> {
    if matches!(agent, Agent::Pi) {
        let pi_agent_dir = staged_home.join("agent");
        assets::ensure_guard_extension(&pi_agent_dir)?;
        assets::ensure_settings_template(&pi_agent_dir)?;
    }
    Ok(())
}

fn stage_filtered_home_dir(agent: Agent, src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let entry_path = entry.path();
        let file_name = entry.file_name();
        let Some(name) = file_name.to_str() else {
            continue;
        };
        let meta = fs::symlink_metadata(&entry_path)?;
        if meta.file_type().is_symlink() {
            continue;
        }
        let target = dst.join(name);
        if meta.is_file() {
            if allowed_home_file(agent, name) {
                copy_regular_file(&entry_path, &target)?;
            }
            continue;
        }
        if meta.is_dir() && allowed_home_dir(agent, name) {
            copy_tree_without_symlinks(&entry_path, &target)?;
        }
    }
    Ok(())
}

fn allowed_home_file(agent: Agent, name: &str) -> bool {
    COMMON_HOME_FILES.contains(&name)
        || matches!(agent, Agent::Claude) && CLAUDE_HOME_FILES.contains(&name)
}

fn allowed_home_dir(agent: Agent, name: &str) -> bool {
    COMMON_HOME_DIRS.contains(&name)
        || matches!(agent, Agent::Claude) && CLAUDE_HOME_DIRS.contains(&name)
}

fn copy_tree_without_symlinks(src: &Path, dst: &Path) -> std::io::Result<()> {
    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        let meta = fs::symlink_metadata(&src_path)?;
        if meta.file_type().is_symlink() {
            continue;
        }
        if meta.is_dir() {
            copy_tree_without_symlinks(&src_path, &dst_path)?;
        } else if meta.is_file() {
            copy_regular_file(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

fn copy_tree_with_symlinks(src: &Path, dst: &Path) -> std::io::Result<()> {
    let meta = fs::symlink_metadata(src)?;
    if meta.file_type().is_symlink() {
        return copy_symlink(src, dst);
    }
    if meta.is_file() {
        return copy_regular_file(src, dst);
    }

    fs::create_dir_all(dst)?;
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());
        copy_tree_with_symlinks(&src_path, &dst_path)?;
    }
    Ok(())
}

fn copy_regular_file(src: &Path, dst: &Path) -> std::io::Result<()> {
    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::copy(src, dst)?;
    let perms = fs::metadata(src)?.permissions();
    fs::set_permissions(dst, perms)
}

#[cfg(unix)]
fn copy_symlink(src: &Path, dst: &Path) -> std::io::Result<()> {
    use std::os::unix::fs as unix_fs;

    if let Some(parent) = dst.parent() {
        fs::create_dir_all(parent)?;
    }
    unix_fs::symlink(fs::read_link(src)?, dst)
}

#[cfg(not(unix))]
fn copy_symlink(src: &Path, dst: &Path) -> std::io::Result<()> {
    let _ = (src, dst);
    Err(std::io::Error::other(
        "symlink-preserving lockdown staging requires unix support",
    ))
}

fn stage_path(stage_root: &Path, container: &str) -> PathBuf {
    stage_root.join(container.trim_start_matches('/'))
}

fn canonical_source(path: &Path) -> Option<PathBuf> {
    if !path.exists() {
        return None;
    }
    path.canonicalize()
        .ok()
        .or_else(|| Some(path.to_path_buf()))
}
