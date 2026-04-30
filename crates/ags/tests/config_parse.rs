use std::path::Path;

use ags::config::{
    MountKind, MountMode, MountWhen, SecretSource, ValidatedConfig,
    parse_and_validate_with_overlay, parse_toml_str,
};
use tempfile::tempdir;

fn minimal_sandbox_toml() -> &'static str {
    r#"
[sandbox]
image = "localhost/agent-sandbox:latest"
containerfile = "/tmp/Containerfile"
cache_dir = "/tmp/cache"
gitconfig_path = "/tmp/gitconfig"
auth_key = "/tmp/auth"
sign_key = "/tmp/sign"
"#
}

fn parse_minimal(extra: &str) -> ValidatedConfig {
    let toml = format!("{}\n{extra}", minimal_sandbox_toml());
    parse_toml_str(&toml, Path::new("/test/config.toml")).unwrap()
}

fn parse_err(extra: &str) -> String {
    let toml = format!("{}\n{extra}", minimal_sandbox_toml());
    parse_toml_str(&toml, Path::new("/test/config.toml"))
        .unwrap_err()
        .to_string()
}

#[test]
fn minimal_config_parses() {
    let cfg = parse_minimal("");
    assert_eq!(cfg.sandbox.image, "localhost/agent-sandbox:latest");
    assert!(cfg.mounts.is_empty());
    assert!(cfg.tools.is_empty());
    assert!(cfg.secrets.is_empty());
    assert!(!cfg.browser.enabled);
    assert_eq!(cfg.update.pi_spec, "@mariozechner/pi-coding-agent");
    assert_eq!(cfg.update.minimum_release_age, 1440);
}

#[test]
fn sandbox_paths_are_absolute() {
    let cfg = parse_minimal("");
    assert!(cfg.sandbox.containerfile.is_absolute());
    assert!(cfg.sandbox.cache_dir.is_absolute());
}

#[test]
fn tilde_expansion_produces_absolute_path() {
    let toml = r#"
[sandbox]
image = "test:latest"
containerfile = "~/Containerfile"
cache_dir = "~/cache"
gitconfig_path = "~/gitconfig"
auth_key = "~/auth"
sign_key = "~/sign"
"#;
    let cfg = parse_toml_str(toml, Path::new("/test/config.toml")).unwrap();
    assert!(cfg.sandbox.containerfile.is_absolute());
    assert!(!cfg.sandbox.containerfile.to_string_lossy().contains('~'));
}

#[test]
fn mount_validation() {
    let cfg = parse_minimal(
        r#"
[[mount]]
host = "/data"
container = "/mnt/data"
mode = "rw"
kind = "dir"
create = true
optional = true
when = "browser"
"#,
    );
    assert_eq!(cfg.mounts.len(), 1);
    let m = &cfg.mounts[0];
    assert_eq!(m.mode, MountMode::Rw);
    assert_eq!(m.kind, MountKind::Dir);
    assert_eq!(m.when, MountWhen::Browser);
    assert!(m.create);
    assert!(m.optional);
    assert_eq!(m.source, "config");
}

#[test]
fn mount_defaults() {
    let cfg = parse_minimal(
        r#"
[[mount]]
host = "/data"
container = "/mnt/data"
mode = "ro"
"#,
    );
    let m = &cfg.mounts[0];
    assert_eq!(m.kind, MountKind::Dir);
    assert_eq!(m.when, MountWhen::Always);
    assert!(!m.create);
    assert!(!m.optional);
}

#[test]
fn agent_mount_is_required_rw_always() {
    let cfg = parse_minimal(
        r#"
[[agent_mount]]
host = "/tmp/claude"
container = "/home/dev/.claude"
"#,
    );
    let m = &cfg.mounts[0];
    assert_eq!(m.mode, MountMode::Rw);
    assert_eq!(m.when, MountWhen::Always);
    assert!(!m.create);
    assert!(!m.optional);
    assert_eq!(m.source, "agent_mount");
}

#[test]
fn invalid_mode_rejected() {
    let err = parse_err(
        r#"
[[mount]]
host = "/data"
container = "/mnt/data"
mode = "rw+"
"#,
    );
    assert!(err.contains("must be 'ro' or 'rw'"), "got: {err}");
}

#[test]
fn invalid_kind_rejected() {
    let err = parse_err(
        r#"
[[mount]]
host = "/data"
container = "/mnt/data"
mode = "ro"
kind = "symlink"
"#,
    );
    assert!(err.contains("must be 'dir' or 'file'"), "got: {err}");
}

#[test]
fn invalid_when_rejected() {
    let err = parse_err(
        r#"
[[mount]]
host = "/data"
container = "/mnt/data"
mode = "ro"
when = "never"
"#,
    );
    assert!(err.contains("must be 'always' or 'browser'"), "got: {err}");
}

#[test]
fn secret_from_env() {
    let cfg = parse_minimal(
        r#"
[[secret]]
env = "GH_TOKEN"
from_env = "GH_TOKEN"
"#,
    );
    assert_eq!(cfg.secrets.len(), 1);
    let s = &cfg.secrets[0];
    assert_eq!(s.env, "GH_TOKEN");
    match &s.source {
        SecretSource::Env { from_env } => assert_eq!(from_env, "GH_TOKEN"),
        _ => panic!("expected Env source"),
    }
    assert!(s.tool.is_none());
}

#[test]
fn secret_store() {
    let cfg = parse_minimal(
        r#"
[[secret]]
env = "GH_TOKEN"
secret_store = { service = "github", username = "user" }
"#,
    );
    assert_eq!(cfg.secrets.len(), 1);
    match &cfg.secrets[0].source {
        SecretSource::SecretTool { attributes } => {
            assert_eq!(attributes.get("service"), Some(&"github".to_owned()));
            assert_eq!(attributes.get("username"), Some(&"user".to_owned()));
        }
        _ => panic!("expected SecretTool source"),
    }
}

#[test]
fn secret_multiple_sources_same_env() {
    let cfg = parse_minimal(
        r#"
[[secret]]
env = "TOKEN"
from_env = "TOKEN"
secret_store = { service = "vault", username = "me" }
"#,
    );
    assert_eq!(cfg.secrets.len(), 2);
    assert!(matches!(&cfg.secrets[0].source, SecretSource::Env { .. }));
    assert!(matches!(
        &cfg.secrets[1].source,
        SecretSource::SecretTool { .. }
    ));
}

#[test]
fn secret_no_source_rejected() {
    let err = parse_err(
        r#"
[[secret]]
env = "TOKEN"
"#,
    );
    assert!(
        err.contains("must define at least one source"),
        "got: {err}"
    );
}

#[test]
fn secret_legacy_provider_env() {
    let cfg = parse_minimal(
        r#"
[[secret]]
env = "TOKEN"
provider = "env"
var = "MY_TOKEN"
"#,
    );
    assert_eq!(cfg.secrets.len(), 1);
    match &cfg.secrets[0].source {
        SecretSource::Env { from_env } => assert_eq!(from_env, "MY_TOKEN"),
        _ => panic!("expected Env source"),
    }
}

#[test]
fn secret_legacy_provider_secret_tool() {
    let cfg = parse_minimal(
        r#"
[[secret]]
env = "TOKEN"
provider = "secret-tool"
attributes = { service = "vault", username = "me" }
"#,
    );
    assert_eq!(cfg.secrets.len(), 1);
    assert!(matches!(
        &cfg.secrets[0].source,
        SecretSource::SecretTool { .. }
    ));
}

#[test]
fn secret_legacy_invalid_provider_rejected() {
    let err = parse_err(
        r#"
[[secret]]
env = "TOKEN"
provider = "keychain"
"#,
    );
    assert!(err.contains("must be 'env' or 'secret-tool'"), "got: {err}");
}

#[test]
fn tool_generates_binary_mount() {
    let cfg = parse_minimal(
        r#"
[[tool]]
name = "kno"
path = "/usr/bin/kno"
container_path = "/usr/local/bin/kno"
optional = true
"#,
    );
    assert_eq!(cfg.tools.len(), 1);
    assert_eq!(cfg.tools[0].name, "kno");

    // Tool generates a binary mount
    assert_eq!(cfg.mounts.len(), 1);
    let m = &cfg.mounts[0];
    assert_eq!(m.kind, MountKind::File);
    assert_eq!(m.source, "tool:kno:binary");
    assert_eq!(m.mode, MountMode::Ro); // default
    assert!(m.optional);
}

#[test]
fn tool_generates_directory_mounts() {
    let cfg = parse_minimal(
        r#"
[[tool]]
name = "kno"
path = "/usr/bin/kno"
container_path = "/usr/local/bin/kno"

[[tool.directory]]
host = "/home/user/.kno"
container = "/home/dev/.kno"
mode = "rw"
kind = "dir"
create = true
"#,
    );
    // binary mount + directory mount
    assert_eq!(cfg.mounts.len(), 2);
    assert_eq!(cfg.mounts[0].source, "tool:kno:binary");
    assert_eq!(cfg.mounts[1].source, "tool:kno:directory");
    assert_eq!(cfg.mounts[1].mode, MountMode::Rw);
    assert!(cfg.mounts[1].create);
}

#[test]
fn tool_generates_secrets_with_tool_tag() {
    let cfg = parse_minimal(
        r#"
[[tool]]
name = "qwk"
path = "/usr/bin/qwk"
container_path = "/usr/local/bin/qwk"

[[tool.secret]]
env = "QWK_TOKEN"
from_env = "QWK_TOKEN"
"#,
    );
    assert_eq!(cfg.secrets.len(), 1);
    assert_eq!(cfg.secrets[0].env, "QWK_TOKEN");
    assert_eq!(cfg.secrets[0].tool.as_deref(), Some("qwk"));
}

#[test]
fn browser_disabled_by_default() {
    let cfg = parse_minimal("");
    assert!(!cfg.browser.enabled);
    assert!(cfg.browser.command.is_empty());
    assert_eq!(cfg.browser.debug_port, 0);
}

#[test]
fn browser_enabled_validated() {
    let cfg = parse_minimal(
        r#"
[browser]
enabled = true
command = "google-chrome"
profile_dir = "/tmp/chrome"
debug_port = 9222
pi_skill_path = "/home/dev/browser-tools"
command_args = ["--no-sandbox"]
"#,
    );
    assert!(cfg.browser.enabled);
    assert_eq!(cfg.browser.command, "google-chrome");
    assert_eq!(cfg.browser.debug_port, 9222);
    assert_eq!(cfg.browser.pi_skill_path, "/home/dev/browser-tools");
    assert_eq!(cfg.browser.command_args, vec!["--no-sandbox"]);
}

#[test]
fn browser_path_command_expanded() {
    let cfg = parse_minimal(
        r#"
[browser]
enabled = true
command = "/usr/bin/chromium"
profile_dir = "/tmp/chrome"
debug_port = 9222
"#,
    );
    assert!(cfg.browser.command.starts_with('/'));
}

#[test]
fn browser_enabled_missing_command_rejected() {
    let err = parse_err(
        r#"
[browser]
enabled = true
profile_dir = "/tmp/chrome"
debug_port = 9222
"#,
    );
    assert!(err.contains("[browser].command"), "got: {err}");
}

#[test]
fn browser_enabled_missing_port_rejected() {
    let err = parse_err(
        r#"
[browser]
enabled = true
command = "chrome"
profile_dir = "/tmp/chrome"
"#,
    );
    assert!(err.contains("debug_port"), "got: {err}");
}

#[test]
fn update_defaults() {
    let cfg = parse_minimal("");
    assert_eq!(cfg.update.pi_spec, "@mariozechner/pi-coding-agent");
    assert_eq!(cfg.update.minimum_release_age, 1440);
}

#[test]
fn update_overrides() {
    let cfg = parse_minimal(
        r#"
[update]
pi_spec = "@custom/agent"
minimum_release_age = 60
"#,
    );
    assert_eq!(cfg.update.pi_spec, "@custom/agent");
    assert_eq!(cfg.update.minimum_release_age, 60);
}

#[test]
fn empty_update_pi_spec_rejected() {
    let err = parse_err(
        r#"
[update]
pi_spec = ""
"#,
    );
    assert!(err.contains("[update].pi_spec"), "got: {err}");
}

#[test]
fn invalid_toml_produces_toml_error() {
    let result = parse_toml_str("not valid [[ toml", Path::new("/test/config.toml"));
    let err = result.unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("invalid TOML"), "got: {msg}");
}

#[test]
fn empty_image_rejected() {
    let toml = r#"
[sandbox]
image = ""
containerfile = "/tmp/Containerfile"
cache_dir = "/tmp/cache"
gitconfig_path = "/tmp/gitconfig"
auth_key = "/tmp/auth"
sign_key = "/tmp/sign"
"#;
    let err = parse_toml_str(toml, Path::new("/test/config.toml"))
        .unwrap_err()
        .to_string();
    assert!(err.contains("[sandbox].image"), "got: {err}");
}

#[test]
fn passthrough_env_preserved() {
    let toml = r#"
[sandbox]
image = "test:latest"
containerfile = "/tmp/cf"
cache_dir = "/tmp/cache"
gitconfig_path = "/tmp/gc"
auth_key = "/tmp/a"
sign_key = "/tmp/s2"
passthrough_env = ["API_KEY", "OTHER_KEY"]
"#;
    let cfg = parse_toml_str(toml, Path::new("/test/config.toml")).unwrap();
    assert_eq!(cfg.sandbox.passthrough_env, vec!["API_KEY", "OTHER_KEY"]);
}

#[test]
fn config_file_path_stored() {
    let cfg = parse_minimal("");
    assert_eq!(cfg.config_file, Path::new("/test/config.toml"));
}

#[test]
fn overlay_config_overrides_tables_and_appends_repeatable_sections() {
    let dir = tempdir().unwrap();
    let base_path = dir.path().join("base.toml");
    let overlay_path = dir.path().join("overlay.toml");

    std::fs::write(
        &base_path,
        r#"
[sandbox]
image = "base:latest"
containerfile = "/tmp/Containerfile"
cache_dir = "/tmp/cache"
gitconfig_path = "/tmp/gitconfig"
auth_key = "/tmp/auth"
sign_key = "/tmp/sign"
passthrough_env = ["BASE_TOKEN"]

[[mount]]
host = "/base"
container = "/mnt/base"
mode = "ro"

[update]
pi_spec = "@base/pi"
minimum_release_age = 1440

[auth_proxy]
auto_allow_domains = ["base.example"]
"#,
    )
    .unwrap();

    std::fs::write(
        &overlay_path,
        r#"
[sandbox]
image = "repo:latest"
passthrough_env = ["REPO_TOKEN"]

[[mount]]
host = "/repo"
container = "/mnt/repo"
mode = "rw"

[update]
pi_spec = "@repo/pi"

[auth_proxy]
auto_allow_domains = ["repo.example"]
"#,
    )
    .unwrap();

    let cfg = parse_and_validate_with_overlay(&base_path, Some(&overlay_path)).unwrap();

    assert_eq!(cfg.sandbox.image, "repo:latest");
    assert_eq!(cfg.sandbox.passthrough_env, vec!["REPO_TOKEN"]);
    assert_eq!(cfg.update.pi_spec, "@repo/pi");
    assert_eq!(cfg.update.minimum_release_age, 1440);
    assert_eq!(cfg.auth_proxy.auto_allow_domains, vec!["repo.example"]);
    assert_eq!(cfg.mounts.len(), 2);
    assert_eq!(cfg.mounts[0].host, Path::new("/base"));
    assert_eq!(cfg.mounts[1].host, Path::new("/repo"));
    assert_eq!(cfg.mounts[1].mode, MountMode::Rw);
}

#[test]
fn overlay_config_reports_overlay_toml_errors() {
    let dir = tempdir().unwrap();
    let base_path = dir.path().join("base.toml");
    let overlay_path = dir.path().join("overlay.toml");

    std::fs::write(
        &base_path,
        r#"
[sandbox]
image = "base:latest"
containerfile = "/tmp/Containerfile"
cache_dir = "/tmp/cache"
gitconfig_path = "/tmp/gitconfig"
auth_key = "/tmp/auth"
sign_key = "/tmp/sign"
"#,
    )
    .unwrap();
    std::fs::write(&overlay_path, "not valid [[ toml").unwrap();

    let err = parse_and_validate_with_overlay(&base_path, Some(&overlay_path))
        .unwrap_err()
        .to_string();
    assert!(
        err.contains(overlay_path.to_string_lossy().as_ref()),
        "got: {err}"
    );
}

#[test]
fn file_not_found_produces_io_error() {
    let result = ags::config::parse_and_validate(Path::new("/nonexistent/config.toml"));
    let err = result.unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("failed to read"), "got: {msg}");
}
