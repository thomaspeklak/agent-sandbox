use std::fs;
use std::io::{self, Write};
use std::path::Path;

pub const CONTAINERFILE: &str = include_str!("../../../config/Containerfile");
pub const DEFAULT_AGENT_PROVIDERS_LOCK: &str =
    include_str!("../../../config/default-agent-providers.lock.json");
pub const DEFAULT_TOOL_DOWNLOADS_LOCK: &str =
    include_str!("../../../config/default-tool-downloads.lock.json");
pub const TMUX_CONF: &str = include_str!("../../../config/tmux.conf");
pub const UV_TOML: &str = include_str!("../../../config/uv.toml");
pub const GUARD_TS: &str = include_str!("../../../agent/extensions/guard.ts");
pub const GUARD_SH: &str = include_str!("../../../agent/hooks/guard.sh");
pub const GUARD_SKILL_MD: &str = include_str!("../../../agent/hooks/skills/guard/SKILL.md");
pub const GUARD_PLUGIN_JSON: &str = include_str!("../../../agent/hooks/.claude-plugin/plugin.json");
pub const SETTINGS_EXAMPLE: &str = include_str!("../../../agent/settings.example.json");
pub const AUTH_PROXY_SHIM: &str = include_str!("../../../agent/auth-proxy-shim");
pub const CLIPBOARD_SHIM: &str = include_str!("../../../agent/clipboard-shim");
pub const WEBVIEW_RELAY_SHIM: &str = include_str!("../../../agent/webview-relay-shim");
pub const WEBVIEW_URL_HELPER: &str = include_str!("../../../agent/webview-url-helper");
pub const ONEPASSWORD_BOOTSTRAP: &str = include_str!("../../../agent/onepassword-bootstrap");
pub const ONEPASSWORD_BOOTSTRAP_NAME: &str = "onepassword-bootstrap";
pub const GLIMPSE_SHIM_CARGO_TOML: &str = include_str!("../../../crates/glimpse-shim/Cargo.toml");
pub const GLIMPSE_SHIM_MAIN: &str = include_str!("../../../crates/glimpse-shim/src/main.rs");
pub const GLIMPSE_SHIM_SOCKET: &str = include_str!("../../../crates/glimpse-shim/src/socket.rs");
pub const GLIMPSE_SHIM_BRIDGE: &str = include_str!("../../../crates/glimpse-shim/src/bridge.rs");
/// Standalone lockfile for the copied Glimpse crate. The workspace lock cannot
/// be used unchanged outside the workspace; this is its pruned subset.
pub const GLIMPSE_SHIM_CARGO_LOCK: &str =
    include_str!("../../../config/image/glimpse-shim.Cargo.lock");

/// Component recipes, written below `image/` in every build snapshot.
pub const IMAGE_RECIPES: &[(&str, &str)] = &[
    (
        "os-baseline.Containerfile",
        include_str!("../../../config/image/os-baseline.Containerfile"),
    ),
    (
        "os-refresh.Containerfile",
        include_str!("../../../config/image/os-refresh.Containerfile"),
    ),
    (
        "build-foundation.Containerfile",
        include_str!("../../../config/image/build-foundation.Containerfile"),
    ),
    (
        "rust.Containerfile",
        include_str!("../../../config/image/rust.Containerfile"),
    ),
    (
        "rust-install.sh",
        include_str!("../../../config/image/rust-install.sh"),
    ),
    (
        "pnpm.Containerfile",
        include_str!("../../../config/image/pnpm.Containerfile"),
    ),
    (
        "vendor-tool.Containerfile",
        include_str!("../../../config/image/vendor-tool.Containerfile"),
    ),
    (
        "glimpse.Containerfile",
        include_str!("../../../config/image/glimpse.Containerfile"),
    ),
    (
        "verify-image.sh",
        include_str!("../../../config/image/verify-image.sh"),
    ),
];

/// Return the embedded component recipe named `name`.
pub fn image_recipe(name: &str) -> &'static str {
    IMAGE_RECIPES
        .iter()
        .find_map(|(recipe, content)| (*recipe == name).then_some(*content))
        .unwrap_or_else(|| panic!("unknown embedded image recipe {name}"))
}

/// Write `content` into `dir/name`, creating `dir` if needed, and optionally
/// setting permissions to `mode` on Unix.
fn write_asset(dir: &Path, name: &str, content: &str, mode: Option<u32>) -> io::Result<()> {
    let target = dir.join(name);
    write_asset_at(&target, content)?;
    if let Some(m) = mode {
        set_permissions(&target, m);
    }
    Ok(())
}

/// Write `content` to an exact `path`, creating its parent directory if needed.
///
/// Unchanged files are left untouched, and changed files are replaced
/// atomically, so a concurrent reader never observes a partial asset.
fn write_asset_at(path: &Path, content: &str) -> io::Result<()> {
    if fs::read(path).is_ok_and(|existing| existing == content.as_bytes()) {
        return Ok(());
    }
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)?;
    let mut temp = tempfile::NamedTempFile::new_in(parent)?;
    temp.write_all(content.as_bytes())?;
    set_permissions(temp.path(), 0o644);
    temp.persist(path).map_err(|error| error.error)?;
    Ok(())
}

/// Write the embedded final-assembly Containerfile to `path`.
pub fn ensure_containerfile(path: &Path) -> io::Result<()> {
    write_asset_at(path, CONTAINERFILE)
}

/// Write the embedded tmux config alongside the configured Containerfile.
pub fn ensure_tmux_conf(path: &Path) -> io::Result<()> {
    write_asset_at(path, TMUX_CONF)
}

/// Write the image-wide uv policy alongside the configured Containerfile.
pub fn ensure_uv_config(path: &Path) -> io::Result<()> {
    write_asset_at(path, UV_TOML)
}

/// Write a reference copy of the image recipes next to the configured
/// Containerfile. Builds never read these files: `ags update-image` builds from
/// a private snapshot of the embedded recipes (see [`write_image_build_snapshot`]).
pub fn ensure_image_build_context(containerfile: &Path) -> io::Result<()> {
    ensure_containerfile(containerfile)?;
    let dir = containerfile.parent().unwrap_or_else(|| Path::new("."));
    write_recipe_set(dir)
}

/// Write the complete embedded recipe set into `dir` as a build snapshot:
/// `Containerfile` (final assembly), `tmux.conf`, `uv.toml`, `image/*`, and the
/// Glimpse crate with its standalone lockfile.
pub fn write_image_build_snapshot(dir: &Path) -> io::Result<()> {
    ensure_containerfile(&dir.join("Containerfile"))?;
    write_recipe_set(dir)
}

fn write_recipe_set(dir: &Path) -> io::Result<()> {
    ensure_tmux_conf(&dir.join("tmux.conf"))?;
    ensure_uv_config(&dir.join("uv.toml"))?;
    for (name, content) in IMAGE_RECIPES {
        write_asset(&dir.join("image"), name, content, None)?;
    }
    ensure_glimpse_shim(dir)
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

/// Write the embedded clipboard shim to `<dir>/clipboard-shim`.
pub fn ensure_clipboard_shim(dir: &Path) -> io::Result<()> {
    write_asset(dir, "clipboard-shim", CLIPBOARD_SHIM, Some(0o755))
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

/// Write the final-process 1Password bootstrap into a private runtime directory.
/// Callers mount this exact file read-only rather than relying on the image cache.
pub fn ensure_onepassword_bootstrap(dir: &Path) -> io::Result<()> {
    write_asset(
        dir,
        ONEPASSWORD_BOOTSTRAP_NAME,
        ONEPASSWORD_BOOTSTRAP,
        Some(0o755),
    )
}

/// Write the glimpse-shim crate source into `<dir>/glimpse-shim/`.
pub fn ensure_glimpse_shim(dir: &Path) -> io::Result<()> {
    let shim = dir.join("glimpse-shim");
    write_asset(&shim, "Cargo.toml", GLIMPSE_SHIM_CARGO_TOML, None)?;
    write_asset(&shim, "Cargo.lock", GLIMPSE_SHIM_CARGO_LOCK, None)?;
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
