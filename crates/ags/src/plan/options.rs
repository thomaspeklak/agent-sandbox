use super::PlanMount;
use crate::config::ClipboardMode;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub struct BuildLaunchPlanOptions<'a> {
    pub browser_mode: bool,
    pub tmux_mode: bool,
    pub guard_enabled: bool,
    pub lockdown: bool,
    pub ssh_auth_sock: Option<&'a Path>,
    pub resolved_secrets: &'a HashMap<String, String>,
    pub auth_proxy_runtime_dir: Option<&'a Path>,
    pub clipboard_runtime_dir: Option<&'a Path>,
    pub clipboard_mode: ClipboardMode,
    pub host_ui_runtime_dir: Option<&'a Path>,
    pub host_ui_session_id: Option<&'a str>,
    pub webview_relay_runtime_dir: Option<&'a Path>,
    pub psp_socket: Option<&'a Path>,
    pub psp_session_id: Option<&'a str>,
    pub extra_mounts: &'a [PlanMount],
    pub extra_mount_dirs: &'a [PathBuf],
    pub env: &'a [(String, String)],
    pub stop_when_done: bool,
    pub root_mode: bool,
    pub wayland_passthrough: bool,
    /// Anonymous item FDs prepared for the final-process bootstrap.
    pub payload_fd_count: usize,
    /// Container path of the mounted bootstrap. Must be present with payload FDs.
    pub bootstrap_path: Option<&'a str>,
    /// Exact host path of the private, per-run bootstrap asset.
    pub bootstrap_host_path: Option<&'a Path>,
}
