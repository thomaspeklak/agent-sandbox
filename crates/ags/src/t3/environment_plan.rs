use super::registration::Registration;
use super::sidecars::Sidecars;
use crate::cli::Agent;
use crate::config::{MountMode, ValidatedConfig};
use crate::plan::{BuildLaunchPlanOptions, LaunchPlan, PlanMount};
use std::collections::HashMap;
use std::io;
use std::path::Path;

pub fn build(
    registration: &Registration,
    config: &ValidatedConfig,
    sidecars: &Sidecars,
    runtime: &Path,
    version: &str,
    assets: &Path,
    ssh_socket: Option<&Path>,
) -> io::Result<LaunchPlan> {
    let secrets = HashMap::new();
    let mut plan = crate::plan::build_launch_plan(
        config,
        &registration.repository.main,
        Agent::T3,
        BuildLaunchPlanOptions {
            browser_mode: registration.settings.browser,
            tmux_mode: false,
            guard_enabled: !registration.settings.yolo,
            lockdown: false,
            ssh_auth_sock: ssh_socket,
            resolved_secrets: &secrets,
            auth_proxy_runtime_dir: sidecars
                .auth
                .as_ref()
                .map(|guard| guard.runtime_dir.as_path()),
            clipboard_runtime_dir: sidecars
                .clipboard
                .as_ref()
                .map(|guard| guard.runtime_dir.as_path()),
            clipboard_mode: config.clipboard.effective_mode(),
            host_ui_runtime_dir: sidecars
                .ui
                .as_ref()
                .map(|guard| guard.runtime_dir.as_path()),
            host_ui_session_id: sidecars.ui.as_ref().map(|guard| guard.session_id.as_str()),
            webview_relay_runtime_dir: sidecars
                .relay
                .as_ref()
                .map(|guard| guard.runtime_dir.as_path()),
            psp_socket: sidecars
                .psp
                .as_ref()
                .map(|guard| guard.socket_path.as_path()),
            psp_session_id: sidecars.psp.as_ref().map(|_| "ags-t3"),
            extra_mounts: &[],
            extra_mount_dirs: &registration.settings.add_dirs,
            env: &[],
            stop_when_done: false,
            root_mode: registration.settings.root,
            wayland_passthrough: registration.settings.wayland
                || config.desktop_passthrough.wayland,
            payload_fd_count: 0,
            bootstrap_path: None,
            bootstrap_host_path: None,
        },
    )
    .map_err(io::Error::other)?;
    let selected = plan.runtime_lease.as_ref().unwrap().path.clone();
    for mount in &mut plan.mounts {
        if let Ok(relative) = mount.host.strip_prefix(&selected)
            && relative.components().next().is_some_and(|part| {
                crate::agent_runtime::RUNTIME_DIRS
                    .contains(&part.as_os_str().to_str().unwrap_or(""))
            })
        {
            mount.host = runtime.join(relative);
            mount.mode = MountMode::Ro;
        }
    }
    plan.runtime_lease = Some(crate::agent_runtime::pin_path(
        &config.sandbox.cache_dir,
        runtime,
    )?);
    let repository = registration.repository.validate()?;
    let first_added = plan.mounts.len();
    for path in repository
        .worktrees
        .iter()
        .chain(std::iter::once(&repository.common))
    {
        if !path.starts_with(&repository.main) && !path.starts_with(&registration.home) {
            add_mount(&mut plan, path, MountMode::Rw);
        }
        for metadata in crate::git::discover_external_git_mounts(path).paths {
            if !metadata.starts_with(&repository.main)
                && !metadata.starts_with(&repository.common)
                && !metadata.starts_with(&registration.home)
            {
                add_mount(&mut plan, &metadata, MountMode::Rw);
            }
        }
    }
    add_mount(&mut plan, &registration.home, MountMode::Rw);
    crate::plan::validate_protected_cache_mounts(
        &plan.mounts[first_added..],
        &config.sandbox.cache_dir,
        None,
    )
    .map_err(io::Error::other)?;
    plan.mounts.push(PlanMount {
        host: assets.to_owned(),
        container: "/run/ags-t3".into(),
        mode: MountMode::Ro,
    });
    plan.mounts.push(PlanMount {
        host: runtime.join("pnpm-home/ags-t3-runtime"),
        container: registration.home.join(".t3/runtime").display().to_string(),
        mode: MountMode::Ro,
    });
    for (name, value) in &mut plan.env.inline {
        if name == "HOME" {
            *value = registration.home.display().to_string();
        }
        if name == "PATH" {
            *value = format!("/run/ags-t3:{value}");
        }
    }
    plan.env.inline.extend([
        (
            "T3CODE_HOME".into(),
            registration.home.join(".t3").display().to_string(),
        ),
        ("T3CODE_NO_BROWSER".into(), "1".into()),
        ("CLAUDE_CONFIG_DIR".into(), "/home/dev/.claude".into()),
        ("CODEX_HOME".into(), "/home/dev/.codex".into()),
    ]);
    plan.env.env_file_entries.clear();
    plan.env.passthrough_names.clear();
    // Keep shared Node/bootstrap/browser/relay setup, replacing only the final CLI exec.
    let suffix = "exec env AGS_NODE_AGENT_BOOTSTRAP=1 /usr/local/pnpm/bin/t3 \"$@\"";
    let setup = plan
        .entrypoint
        .strip_suffix(suffix)
        .ok_or_else(|| io::Error::other("unexpected T3 launch entrypoint"))?;
    plan.entrypoint = format!("{setup}exec sleep infinity");
    plan.container_name = registration.name();
    if !super::compatibility::valid_version(version) {
        return Err(io::Error::other("invalid T3 runtime version"));
    }
    Ok(plan)
}

fn add_mount(plan: &mut LaunchPlan, path: &Path, mode: MountMode) {
    let container = path.display().to_string();
    if !plan
        .mounts
        .iter()
        .any(|mount| mount.host == path && mount.container == container)
    {
        plan.mounts.push(PlanMount {
            host: path.to_owned(),
            container: container.clone(),
            mode,
        });
        for roots in [
            &mut plan.env.read_roots_json,
            &mut plan.env.write_roots_json,
        ] {
            let mut values: Vec<String> = serde_json::from_str(roots).unwrap_or_default();
            if !values.contains(&container) {
                values.push(container.clone());
            }
            *roots = serde_json::to_string(&values).unwrap();
        }
    }
}
