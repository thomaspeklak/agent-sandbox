use std::fs;
use std::io;
use std::path::Path;

pub const CONTAINERFILE: &str = include_str!("../../../config/Containerfile");
pub const TMUX_CONF: &str = include_str!("../../../config/tmux.conf");
pub const GUARD_TS: &str = include_str!("../../../agent/extensions/guard.ts");
pub const GUARD_SH: &str = include_str!("../../../agent/hooks/guard.sh");
pub const GUARD_SKILL_MD: &str = include_str!("../../../agent/hooks/skills/guard/SKILL.md");
pub const GUARD_PLUGIN_JSON: &str =
    include_str!("../../../agent/hooks/.claude-plugin/plugin.json");
pub const SETTINGS_EXAMPLE: &str = include_str!("../../../agent/settings.example.json");

/// Write the embedded Containerfile to `path`, always overwriting.
pub fn ensure_containerfile(path: &Path) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, CONTAINERFILE)
}

/// Write the embedded tmux config alongside the configured Containerfile.
pub fn ensure_tmux_conf(path: &Path) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, TMUX_CONF)
}

/// Write the embedded guard.ts to `<pi_sandbox>/extensions/guard.ts`, always overwriting.
pub fn ensure_guard_extension(pi_sandbox: &Path) -> io::Result<()> {
    let dir = pi_sandbox.join("extensions");
    fs::create_dir_all(&dir)?;
    fs::write(dir.join("guard.ts"), GUARD_TS)
}

/// Write the embedded settings template to `<pi_sandbox>/settings.json`,
/// only if it doesn't already exist (user may have customized).
pub fn ensure_settings_template(pi_sandbox: &Path) -> io::Result<()> {
    let target = pi_sandbox.join("settings.json");
    if target.exists() {
        return Ok(());
    }
    fs::create_dir_all(pi_sandbox)?;
    fs::write(&target, SETTINGS_EXAMPLE)?;
    set_permissions_600(&target);
    Ok(())
}

/// Write the embedded guard.sh hook for Claude to `<hooks_dir>/guard.sh`, always overwriting.
pub fn ensure_claude_guard_hook(hooks_dir: &Path) -> io::Result<()> {
    fs::create_dir_all(hooks_dir)?;
    let path = hooks_dir.join("guard.sh");
    fs::write(&path, GUARD_SH)?;
    set_permissions_755(&path);
    Ok(())
}

/// Write the embedded guard skill and plugin manifest for Claude to `<hooks_dir>/`, always overwriting.
///
/// Layout produced:
///   hooks_dir/.claude-plugin/plugin.json
///   hooks_dir/skills/guard/SKILL.md
///
/// Claude loads these via `--plugin-dir <hooks_dir>`.
pub fn ensure_claude_guard_skill(hooks_dir: &Path) -> io::Result<()> {
    let plugin_dir = hooks_dir.join(".claude-plugin");
    fs::create_dir_all(&plugin_dir)?;
    fs::write(plugin_dir.join("plugin.json"), GUARD_PLUGIN_JSON)?;

    let skill_dir = hooks_dir.join("skills/guard");
    fs::create_dir_all(&skill_dir)?;
    fs::write(skill_dir.join("SKILL.md"), GUARD_SKILL_MD)
}

fn set_permissions_600(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o600));
    }
}

fn set_permissions_755(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(0o755));
    }
}
