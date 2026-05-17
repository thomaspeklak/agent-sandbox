use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use crate::config::{ClipboardMode, MountWhen, SecretSource, ValidatedConfig};

use super::doctor_util::{
    Checker, check_optional_cmd, check_required_cmd, file_non_empty, git_config_get, is_pid_alive,
    is_port_open, list_agent_keys, pub_key_path, read_agent_env, secret_tool_has_value,
    socket_exists,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DoctorSummary {
    pub ok_count: u32,
    pub warn_count: u32,
    pub fail_count: u32,
}

impl DoctorSummary {
    pub fn all_clear(self) -> bool {
        self.fail_count == 0 && self.warn_count == 0
    }
}

/// Run the doctor command: health-check the sandbox environment.
/// Returns `true` if no failures were found.
pub fn run(config: &ValidatedConfig) -> bool {
    let mut ck = Checker::new();
    run_checks(&mut ck, config);
    ck.print_summary();
    ck.fail_count == 0
}

pub fn summarize(config: &ValidatedConfig) -> DoctorSummary {
    let mut ck = Checker::quiet();
    run_checks(&mut ck, config);
    DoctorSummary {
        ok_count: ck.ok_count,
        warn_count: ck.warn_count,
        fail_count: ck.fail_count,
    }
}

fn run_checks(ck: &mut Checker, config: &ValidatedConfig) {
    check_tooling(ck);
    check_config_files(ck, config);
    check_integrations(ck, config);
    check_container_image(ck, config);
    check_keys_and_agent(ck, config);
    check_secrets(ck, config);
    check_sessions(ck, config);
    check_browser(ck, config);
    check_host_ui(ck, config);
    check_clipboard(ck, config);
}

fn check_tooling(ck: &mut Checker) {
    ck.section("Tooling");
    for cmd in &["podman", "git", "ssh-keygen", "ssh-add", "bash"] {
        check_required_cmd(ck, cmd);
    }
    check_optional_cmd(ck, "secret-tool");
    check_optional_cmd(ck, "curl");
}

fn check_config_files(ck: &mut Checker, config: &ValidatedConfig) {
    ck.section("Config");

    // Self-heal: write embedded image-build assets before checking
    let _ = crate::assets::ensure_image_build_context(&config.sandbox.containerfile);
    let tmux_conf = config.sandbox.containerfile.with_file_name("tmux.conf");
    let pi_host = config.mount_host_for_container("/home/dev/.pi");
    if let Some(pi_host) = pi_host {
        let _ = crate::assets::ensure_guard_extension(&pi_host.join("agent"));
    }
    let hooks_dir = config.sandbox.cache_dir.join("ags-hooks");
    let _ = crate::assets::ensure_claude_guard_hook(&hooks_dir);
    let _ = crate::assets::ensure_claude_guard_skill(&hooks_dir);

    check_file_exists(ck, &config.sandbox.containerfile, "Containerfile", true);
    check_file_exists(ck, &tmux_conf, "tmux config", true);
    if let Some(pi_host) = pi_host {
        let pi_agent_dir = pi_host.join("agent");
        let settings = pi_agent_dir.join("settings.json");
        check_file_exists(ck, &settings, "sandbox settings", true);
        let guard = pi_agent_dir.join("extensions/guard.ts");
        check_file_exists(ck, &guard, "Pi guard extension", true);
    } else {
        ck.fail("required mount missing for container path /home/dev/.pi");
    }
    let claude_guard = hooks_dir.join("guard.sh");
    check_file_exists(ck, &claude_guard, "Claude guard hook", true);
    let claude_plugin = hooks_dir.join(".claude-plugin/plugin.json");
    check_file_exists(ck, &claude_plugin, "Claude guard plugin manifest", true);
    let claude_skill = hooks_dir.join("skills/guard/SKILL.md");
    check_file_exists(ck, &claude_skill, "Claude guard skill", true);
    check_gitconfig(ck, &config.sandbox.gitconfig_path);
}

fn check_file_exists(ck: &mut Checker, path: &Path, label: &str, required: bool) {
    if path.exists() {
        ck.ok(&format!("{label} present: {}", path.display()));
    } else if required {
        ck.fail(&format!("missing {label}: {}", path.display()));
    } else {
        ck.warn(&format!("{label} missing: {}", path.display()));
    }
}

fn check_gitconfig(ck: &mut Checker, path: &Path) {
    if !path.exists() {
        ck.warn(&format!(
            "git signing config missing (created on first run): {}",
            path.display()
        ));
        return;
    }
    ck.ok(&format!("git signing config present: {}", path.display()));
    if let Some(val) = git_config_get(path, "gpg.format") {
        if val == "ssh" {
            ck.ok("git signing format is SSH");
        } else {
            ck.warn(&format!(
                "git signing format is not SSH in {}",
                path.display()
            ));
        }
    }
    if let Some(val) = git_config_get(path, "commit.gpgsign") {
        if val == "true" {
            ck.ok("commit signing enabled in sandbox git config");
        } else {
            ck.warn("commit signing not forced in sandbox git config");
        }
    }
}

fn check_integrations(ck: &mut Checker, config: &ValidatedConfig) {
    ck.section("Integrations");
    if config.tools.is_empty() {
        ck.warn("no tools configured in [[tool]]");
    }
    for tool in &config.tools {
        if tool.path.exists() && crate::util::is_executable(&tool.path) {
            ck.ok(&format!(
                "tool '{}' binary present: {}",
                tool.name,
                tool.path.display()
            ));
        } else if tool.optional {
            ck.warn(&format!(
                "optional tool '{}' missing: {}",
                tool.name,
                tool.path.display()
            ));
        } else {
            ck.fail(&format!(
                "required tool '{}' missing: {}",
                tool.name,
                tool.path.display()
            ));
        }
    }
    for mount in config.mounts.iter().filter(|m| m.when == MountWhen::Always) {
        check_mount(ck, mount);
    }
}

fn check_mount(ck: &mut Checker, mount: &crate::config::ValidatedMount) {
    let desc = format!("({}): {}", mount.source, mount.host.display());
    if mount.host.exists() {
        ck.ok(&format!("mount source present {desc}"));
    } else if mount.create {
        ck.warn(&format!(
            "mount source missing but will be created on run {desc}"
        ));
    } else if mount.optional {
        ck.warn(&format!("optional mount source missing {desc}"));
    } else {
        ck.fail(&format!("required mount source missing {desc}"));
    }
}

fn check_container_image(ck: &mut Checker, config: &ValidatedConfig) {
    ck.section("Container image/runtime");
    let image = &config.sandbox.image;
    if crate::podman::image_exists(image) {
        ck.ok(&format!("image exists: {image}"));
        match crate::podman::image_has_binary(image, "dcg") {
            Ok(true) => ck.ok("bundled dcg available inside sandbox image"),
            Ok(false) => ck.fail(
                "bundled dcg missing inside sandbox image; Pi/Claude Bash guards will fail open (run 'ags update-image')",
            ),
            Err(err) => ck.warn(&format!(
                "could not verify bundled dcg inside sandbox image: {err}"
            )),
        }
    } else {
        ck.warn(&format!(
            "image not built yet: {image} (run 'ags update-image' to build)"
        ));
        ck.warn("cannot verify bundled dcg until the sandbox image is built");
    }
}

fn check_keys_and_agent(ck: &mut Checker, config: &ValidatedConfig) {
    ck.section("Keys & ssh-agent");
    check_key_pair(ck, &config.sandbox.auth_key, "auth");
    check_key_pair(ck, &config.sandbox.sign_key, "signing");
    let agent_env = config.sandbox.cache_dir.join("ssh-agent.env");
    check_agent_status(
        ck,
        &agent_env,
        &config.sandbox.auth_key,
        &config.sandbox.sign_key,
    );
}

fn check_key_pair(ck: &mut Checker, key_path: &Path, label: &str) {
    if key_path.exists() && file_non_empty(key_path) {
        ck.ok(&format!(
            "{label} private key present: {}",
            key_path.display()
        ));
    } else if key_path.exists() {
        ck.warn(&format!(
            "{label} private key is empty: {} (rerun 'ags setup')",
            key_path.display()
        ));
    } else {
        ck.warn(&format!(
            "{label} private key missing: {} (run 'ags setup')",
            key_path.display()
        ));
    }
    let pub_path = pub_key_path(key_path);
    if pub_path.exists() && file_non_empty(&pub_path) {
        ck.ok(&format!(
            "{label} public key present: {}",
            pub_path.display()
        ));
    } else if pub_path.exists() {
        ck.warn(&format!(
            "{label} public key is empty: {}",
            pub_path.display()
        ));
    } else {
        ck.warn(&format!(
            "{label} public key missing: {}",
            pub_path.display()
        ));
    }
}

fn check_agent_status(ck: &mut Checker, env_path: &Path, auth_key: &Path, sign_key: &Path) {
    let Some((sock, pid)) = read_agent_env(env_path) else {
        ck.warn("dedicated ssh-agent not active yet (normal before first run)");
        return;
    };
    let sock_path = Path::new(&sock);
    if !(is_pid_alive(pid) && socket_exists(sock_path)) {
        ck.warn("dedicated ssh-agent not active yet (normal before first run)");
        return;
    }
    ck.ok("dedicated ags ssh-agent appears active");
    if let Some(loaded) = list_agent_keys(sock_path) {
        check_key_loaded(ck, &loaded, auth_key, "auth");
        check_key_loaded(ck, &loaded, sign_key, "signing");
    }
}

fn check_key_loaded(ck: &mut Checker, loaded: &str, key_path: &Path, label: &str) {
    let pub_path = pub_key_path(key_path);
    let Ok(pub_content) = fs::read_to_string(&pub_path) else {
        return;
    };
    let pub_content = pub_content.trim();
    if pub_content.is_empty() {
        return;
    }
    if loaded.lines().any(|l| l.trim() == pub_content) {
        ck.ok(&format!("{label} key loaded in dedicated ssh-agent"));
    } else {
        ck.warn(&format!("{label} key not loaded in dedicated ssh-agent"));
    }
}

fn check_secrets(ck: &mut Checker, config: &ValidatedConfig) {
    ck.section("Secrets");
    let env_names: BTreeSet<&str> = config.secrets.iter().map(|s| s.env.as_str()).collect();
    if env_names.is_empty() {
        ck.warn("no secrets configured");
        return;
    }
    for env_name in &env_names {
        if std::env::var(env_name).is_ok_and(|v| !v.is_empty()) {
            ck.ok(&format!("{env_name} available via environment"));
            continue;
        }
        let mut found = false;
        for secret in config.secrets.iter().filter(|s| s.env == *env_name) {
            match &secret.source {
                SecretSource::Env { from_env } => {
                    if std::env::var(from_env).is_ok_and(|v| !v.is_empty()) {
                        ck.ok(&format!(
                            "{env_name} available via source env var: {from_env}"
                        ));
                        found = true;
                        break;
                    }
                }
                SecretSource::SecretTool { attributes } => {
                    if secret_tool_has_value(attributes) {
                        ck.ok(&format!("{env_name} found in keyring"));
                        found = true;
                        break;
                    }
                }
            }
        }
        if !found {
            ck.warn(&format!("{env_name} not found in configured sources"));
        }
    }
}

fn check_sessions(ck: &mut Checker, config: &ValidatedConfig) {
    ck.section("Sessions / resume");
    let Some(pi_root) = config.mount_host_for_container("/home/dev/.pi") else {
        ck.fail("required mount missing for container path /home/dev/.pi");
        return;
    };
    let pi_dir = pi_root.join("agent");

    if pi_dir.is_dir() && pi_dir.metadata().is_ok_and(|m| !m.permissions().readonly()) {
        ck.ok(&format!("sandbox pi dir is writable: {}", pi_dir.display()));
    } else {
        ck.fail(&format!(
            "sandbox pi dir missing or not writable: {}",
            pi_dir.display()
        ));
    }
    let sessions_dir = pi_dir.join("sessions");
    if sessions_dir.is_dir() {
        ck.ok(&format!(
            "sessions directory present: {}",
            sessions_dir.display()
        ));
    } else {
        ck.warn("sessions directory not created yet (will appear after first session)");
    }
    if pi_dir.join("auth.json").exists() {
        ck.ok("sandbox auth.json present");
    } else {
        ck.warn("sandbox auth.json missing (login once inside sandbox if needed)");
    }
}

/// Check whether a binary (given as a command name or absolute path) is
/// available. Returns `true` when found, `false` otherwise.  On failure the
/// checker records either a `warn` or `fail` depending on `required`.
fn check_binary(ck: &mut Checker, binary: &str, label: &str, required: bool) -> bool {
    if binary.contains('/') {
        let path = Path::new(binary);
        if path.exists() && crate::util::is_executable(path) {
            ck.ok(&format!("{label} is executable: {binary}"));
            return true;
        }
        let msg = format!("{label} missing or not executable: {binary}");
        if required {
            ck.fail(&msg);
        } else {
            ck.warn(&msg);
        }
    } else if crate::util::has_command(binary) {
        ck.ok(&format!("{label} available on PATH: {binary}"));
        return true;
    } else {
        let msg = format!("{label} not found on PATH: {binary}");
        if required {
            ck.fail(&msg);
        } else {
            ck.warn(&msg);
        }
    }
    false
}

fn check_browser(ck: &mut Checker, config: &ValidatedConfig) {
    ck.section("Browser sidecar (optional)");
    if !config.browser.enabled {
        ck.warn("browser integration disabled in config");
        return;
    }
    check_binary(ck, &config.browser.command, "browser command", false);
    let port = config.browser.debug_port;
    if is_port_open(port) {
        ck.ok(&format!(
            "browser debug endpoint reachable on localhost:{port}"
        ));
    } else {
        ck.warn("browser debug endpoint not running (normal until browser mode start)");
    }
    if !config.browser.pi_skill_path.is_empty() {
        ck.ok(&format!(
            "browser pi skill path configured: {}",
            config.browser.pi_skill_path
        ));
    } else {
        ck.warn("browser pi skill path is empty; browser tooling skill won't auto-load");
    }
}

fn check_clipboard(ck: &mut Checker, config: &ValidatedConfig) {
    ck.section("Clipboard bridge");
    let mode = config.clipboard.effective_mode();
    if mode == ClipboardMode::Off {
        ck.warn("clipboard bridge disabled in config");
        return;
    }
    ck.ok(&format!("clipboard bridge mode: {mode}"));
    if mode.can_read() {
        check_optional_cmd(ck, "wl-paste");
    }
    if mode.can_write() {
        check_optional_cmd(ck, "wl-copy");
    }
    if config.clipboard.approval_required {
        ck.ok(&format!(
            "clipboard read approval window: {}s",
            config.clipboard.approval_seconds
        ));
        if !config.host_ui.enabled
            && !crate::util::has_command("zenity")
            && !crate::util::has_command("kdialog")
        {
            ck.warn("clipboard approval prompts need [host_ui] or zenity/kdialog on PATH");
        }
    } else {
        ck.warn("clipboard approval prompts disabled; reads are available for the whole session");
    }
}

fn check_host_ui(ck: &mut Checker, config: &ValidatedConfig) {
    ck.section("Host UI bridge (optional)");
    if !config.host_ui.enabled {
        ck.warn("host UI bridge disabled in config");
        return;
    }

    check_binary(ck, &config.host_ui.binary, "host UI binary", true);

    if config.host_ui.renderer.eq_ignore_ascii_case("process")
        || config.host_ui.renderer.eq_ignore_ascii_case("glimpse")
    {
        if let Some(renderer_bin) = &config.host_ui.renderer_bin {
            if renderer_bin.exists() && crate::util::is_executable(renderer_bin) {
                ck.ok(&format!(
                    "host UI renderer binary present: {}",
                    renderer_bin.display()
                ));
            } else {
                ck.fail(&format!(
                    "host UI renderer binary missing or not executable: {}",
                    renderer_bin.display()
                ));
            }
        } else {
            ck.fail("host UI renderer=process requires [host_ui].renderer_bin");
        }
    } else {
        ck.ok(&format!(
            "host UI renderer configured: {}",
            config.host_ui.renderer
        ));
    }
}
