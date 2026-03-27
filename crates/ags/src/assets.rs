use std::fs;
use std::io;
use std::path::Path;

pub const CONTAINERFILE: &str = include_str!("../../../config/Containerfile");
pub const TMUX_CONF: &str = include_str!("../../../config/tmux.conf");
pub const GUARD_TS: &str = include_str!("../../../agent/extensions/guard.ts");
pub const GUARD_SH: &str = include_str!("../../../agent/hooks/guard.sh");
pub const GUARD_SKILL_MD: &str = include_str!("../../../agent/hooks/skills/guard/SKILL.md");
pub const GUARD_PLUGIN_JSON: &str = include_str!("../../../agent/hooks/.claude-plugin/plugin.json");
pub const SETTINGS_EXAMPLE: &str = include_str!("../../../agent/settings.example.json");
pub const AUTH_PROXY_SHIM: &str = include_str!("../../../agent/auth-proxy-shim");
pub const WEBVIEW_RELAY_SHIM: &str = include_str!("../../../agent/webview-relay-shim");
pub const WEBVIEW_URL_HELPER: &str = include_str!("../../../agent/webview-url-helper");
pub const GLIMPSE_SHIM_CARGO_TOML: &str = include_str!("../../../crates/glimpse-shim/Cargo.toml");
pub const GLIMPSE_SHIM_MAIN: &str = include_str!("../../../crates/glimpse-shim/src/main.rs");
pub const GLIMPSE_SHIM_SOCKET: &str = include_str!("../../../crates/glimpse-shim/src/socket.rs");
pub const GLIMPSE_SHIM_BRIDGE: &str = include_str!("../../../crates/glimpse-shim/src/bridge.rs");

/// Write `content` into `dir/name`, creating `dir` if needed, and optionally
/// setting permissions to `mode` on Unix.
fn write_asset(dir: &Path, name: &str, content: &str, mode: Option<u32>) -> io::Result<()> {
    fs::create_dir_all(dir)?;
    let target = dir.join(name);
    fs::write(&target, content)?;
    if let Some(m) = mode {
        set_permissions(&target, m);
    }
    Ok(())
}

/// Write `content` to an exact `path`, creating its parent directory if needed.
fn write_asset_at(path: &Path, content: &str) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, content)
}

/// Write the embedded Containerfile to `path`, always overwriting.
pub fn ensure_containerfile(path: &Path) -> io::Result<()> {
    write_asset_at(path, CONTAINERFILE)
}

/// Write the embedded tmux config alongside the configured Containerfile.
pub fn ensure_tmux_conf(path: &Path) -> io::Result<()> {
    write_asset_at(path, TMUX_CONF)
}

/// Materialize all files needed for `podman build` from the configured
/// Containerfile directory.
pub fn ensure_image_build_context(containerfile: &Path) -> io::Result<()> {
    ensure_containerfile(containerfile)?;
    ensure_tmux_conf(&containerfile.with_file_name("tmux.conf"))?;
    if let Some(context_dir) = containerfile.parent() {
        ensure_glimpse_shim(context_dir)?;
    }
    Ok(())
}

/// Write the embedded guard.ts to `<pi_sandbox>/extensions/guard.ts`, always overwriting.
pub fn ensure_guard_extension(pi_sandbox: &Path) -> io::Result<()> {
    write_asset(&pi_sandbox.join("extensions"), "guard.ts", GUARD_TS, None)
}

/// Write the embedded settings template to `<pi_sandbox>/settings.json`,
/// only if it doesn't already exist (user may have customized).
pub fn ensure_settings_template(pi_sandbox: &Path) -> io::Result<()> {
    let target = pi_sandbox.join("settings.json");
    if target.exists() {
        return Ok(());
    }
    write_asset(pi_sandbox, "settings.json", SETTINGS_EXAMPLE, Some(0o600))
}

/// Write the embedded guard.sh hook for Claude to `<hooks_dir>/guard.sh`, always overwriting.
pub fn ensure_claude_guard_hook(hooks_dir: &Path) -> io::Result<()> {
    write_asset(hooks_dir, "guard.sh", GUARD_SH, Some(0o755))
}

/// Write the embedded guard skill and plugin manifest for Claude to `<hooks_dir>/`, always overwriting.
///
/// Layout produced:
///   hooks_dir/.claude-plugin/plugin.json
///   hooks_dir/skills/guard/SKILL.md
///
/// Claude loads these via `--plugin-dir <hooks_dir>`.
pub fn ensure_claude_guard_skill(hooks_dir: &Path) -> io::Result<()> {
    write_asset(
        &hooks_dir.join(".claude-plugin"),
        "plugin.json",
        GUARD_PLUGIN_JSON,
        None,
    )?;
    write_asset(
        &hooks_dir.join("skills/guard"),
        "SKILL.md",
        GUARD_SKILL_MD,
        None,
    )
}

/// Write the embedded auth proxy shim to `<dir>/auth-proxy-shim`, always overwriting.
///
/// The shim is made executable (mode 0755).
pub fn ensure_auth_proxy_shim(dir: &Path) -> io::Result<()> {
    write_asset(dir, "auth-proxy-shim", AUTH_PROXY_SHIM, Some(0o755))
}

/// Write the embedded sandbox-side webview relay shim and helper into `dir`.
///
/// Files written:
///   <dir>/webview-relay-shim
///   <dir>/ags-webview-url
pub fn ensure_webview_relay_assets(dir: &Path) -> io::Result<()> {
    write_asset(dir, "webview-relay-shim", WEBVIEW_RELAY_SHIM, Some(0o755))?;
    write_asset(dir, "ags-webview-url", WEBVIEW_URL_HELPER, Some(0o755))
}

/// Write the glimpse-shim crate source into `<dir>/glimpse-shim/`.
pub fn ensure_glimpse_shim(dir: &Path) -> io::Result<()> {
    let shim = dir.join("glimpse-shim");
    write_asset(&shim, "Cargo.toml", GLIMPSE_SHIM_CARGO_TOML, None)?;
    write_asset(&shim.join("src"), "main.rs", GLIMPSE_SHIM_MAIN, None)?;
    write_asset(&shim.join("src"), "socket.rs", GLIMPSE_SHIM_SOCKET, None)?;
    write_asset(&shim.join("src"), "bridge.rs", GLIMPSE_SHIM_BRIDGE, None)
}

fn set_permissions(path: &Path, mode: u32) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(mode));
    }
    #[cfg(not(unix))]
    {
        let _ = (path, mode);
    }
}
