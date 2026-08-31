use std::path::Path;

use ags::cli::Agent;
use ags::config::{
    DEFAULT_EXTRA_DNF_PACKAGES, DEFAULT_PI_SPEC, MountKind, MountMode, MountWhen, SecretSource,
    ValidatedConfig, parse_and_validate_with_overlay, parse_toml_str,
};
use sha2::{Digest, Sha256};
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
    assert_eq!(cfg.sandbox.extra_dnf_packages, DEFAULT_EXTRA_DNF_PACKAGES);
    assert!(cfg.sandbox.tool_download_lock.is_none());
    assert_eq!(
        cfg.sandbox
            .tool_downloads
            .iter()
            .map(|tool| tool.id.as_str())
            .collect::<Vec<_>>(),
        ["br", "bv", "dcg"]
    );
    assert!(cfg.mounts.is_empty());
    assert!(cfg.tools.is_empty());
    assert!(cfg.secrets.is_empty());
    assert!(!cfg.browser.enabled);
    assert_eq!(cfg.clipboard.mode.to_string(), "readwrite");
    assert!(cfg.clipboard.enabled);
    assert!(cfg.clipboard.approval_required);
    assert_eq!(cfg.clipboard.approval_seconds, 300);
    assert!(!cfg.clipboard.approve_writes);
    assert!(!cfg.desktop_passthrough.wayland);
    assert_eq!(cfg.sandbox.enabled_agents, Agent::INSTALLABLE);
    assert!(cfg.sandbox.agent_provider_lock.is_none());
    assert_eq!(
        cfg.sandbox
            .agent_providers
            .iter()
            .map(|entry| entry.agent)
            .collect::<Vec<_>>(),
        Agent::INSTALLABLE
    );
    assert!(cfg.sandbox.is_agent_enabled(Agent::Shell));
    assert_eq!(cfg.update.pi_spec, DEFAULT_PI_SPEC);
    assert_eq!(cfg.update.minimum_release_age, 1440);
}

#[test]
fn sandbox_enabled_agents_can_be_subset_empty_and_canonicalized() {
    let cfg = parse_minimal(r#"enabled_agents = ["opencode", "pi"]"#);
    assert_eq!(cfg.sandbox.enabled_agents, vec![Agent::Pi, Agent::Opencode]);

    let cfg = parse_minimal("enabled_agents = []");
    assert!(cfg.sandbox.enabled_agents.is_empty());
    assert!(cfg.sandbox.is_agent_enabled(Agent::Shell));
    assert!(!cfg.sandbox.is_agent_enabled(Agent::Pi));
}

#[test]
fn sandbox_enabled_agents_reject_invalid_duplicate_and_shell_entries() {
    let invalid = parse_err(r#"enabled_agents = ["other"]"#);
    assert!(invalid.contains("must be one of"), "got: {invalid}");

    let duplicate = parse_err(r#"enabled_agents = ["pi", "pi"]"#);
    assert!(
        duplicate.contains("duplicate agent 'pi'"),
        "got: {duplicate}"
    );

    let shell = parse_err(r#"enabled_agents = ["shell"]"#);
    assert!(shell.contains("shell is always available"), "got: {shell}");
}

#[test]
fn sandbox_dnf_packages_can_be_explicitly_empty() {
    let cfg = parse_minimal("extra_dnf_packages = []");
    assert!(cfg.sandbox.extra_dnf_packages.is_empty());
}

#[test]
fn sandbox_dnf_packages_reject_options_and_shell_expressions() {
    for package in ["--setopt=tsflags=nodocs", "two packages", "python3*"] {
        let err = parse_err(&format!(
            "extra_dnf_packages = [{}]",
            toml::Value::String(package.to_owned())
        ));
        assert!(
            err.contains("must be a package name, not an option or shell expression"),
            "got: {err}"
        );
    }
}

#[test]
fn sandbox_loads_and_validates_tool_download_lock() {
    let dir = tempdir().unwrap();
    let lock = dir.path().join("tool-downloads.lock.json");
    std::fs::write(
        &lock,
        format!(
            r#"[
  {{
    "id": "terraform",
    "download": {{
      "version": "1.0.0",
      "archive": "zip",
      "member": "terraform",
      "install_as": "terraform",
      "artifacts": {{
        "x86_64": {{"url": "https://example.com/terraform-amd64.zip", "sha256": "{}"}},
        "aarch64": {{"url": "https://example.com/terraform-arm64.zip", "sha256": "{}"}}
      }}
    }}
  }}
]"#,
            "a".repeat(64),
            "b".repeat(64)
        ),
    )
    .unwrap();

    let cfg = parse_minimal(&format!(
        "tool_download_lock = {}",
        toml::Value::String(lock.display().to_string())
    ));

    assert_eq!(
        cfg.sandbox.tool_download_lock.as_deref(),
        Some(lock.as_path())
    );
    assert_eq!(cfg.sandbox.tool_downloads.len(), 1);
    assert_eq!(cfg.sandbox.tool_downloads[0].id, "terraform");
}

#[test]
fn sandbox_verifies_content_addressed_lock_digest() {
    let dir = tempdir().unwrap();
    let content = "[]\n";
    let digest = format!("{:x}", Sha256::digest(content.as_bytes()));
    let lock = dir
        .path()
        .join(format!("tool-downloads.{digest}.lock.json"));
    std::fs::write(&lock, content).unwrap();

    let cfg = parse_minimal(&format!(
        "tool_download_lock = {}",
        toml::Value::String(lock.display().to_string())
    ));

    assert!(cfg.sandbox.tool_downloads.is_empty());
}

#[test]
fn explicit_empty_tool_download_lock_disables_embedded_defaults() {
    let dir = tempdir().unwrap();
    let content = "[]\n";
    let digest = format!("{:x}", Sha256::digest(content.as_bytes()));
    let lock = dir
        .path()
        .join(format!("tool-downloads.{digest}.lock.json"));
    std::fs::write(&lock, content).unwrap();

    let cfg = parse_minimal(&format!(
        "tool_download_lock = {}",
        toml::Value::String(lock.display().to_string())
    ));

    assert!(cfg.sandbox.tool_downloads.is_empty());
}

#[test]
fn sandbox_rejects_tampered_content_addressed_lock() {
    let dir = tempdir().unwrap();
    let digest = format!("{:x}", Sha256::digest(b"[]\n"));
    let lock = dir
        .path()
        .join(format!("agent-providers.{digest}.lock.json"));
    std::fs::write(&lock, "[ ]\n").unwrap();

    let error = parse_err(&format!(
        "agent_provider_lock = {}",
        toml::Value::String(lock.display().to_string())
    ));

    assert!(error.contains("content digest does not match"));
}

#[test]
fn sandbox_resolves_relative_tool_download_lock_from_its_config_directory() {
    let dir = tempdir().unwrap();
    let config = dir.path().join("config.toml");
    let lock = dir.path().join("tool-downloads.content.lock.json");
    std::fs::write(
        &lock,
        format!(
            r#"[{{"id":"tool","download":{{"version":"1","archive":"zip","member":"tool","install_as":"tool","artifacts":{{"x86_64":{{"url":"https://example.com/tool.zip","sha256":"{}"}},"aarch64":{{"url":"https://example.com/tool.zip","sha256":"{}"}}}}}}}}]"#,
            "a".repeat(64),
            "b".repeat(64)
        ),
    )
    .unwrap();
    let toml = format!(
        "{}\ntool_download_lock = \"tool-downloads.content.lock.json\"",
        minimal_sandbox_toml()
    );

    let cfg = parse_toml_str(&toml, &config).unwrap();

    assert_eq!(
        cfg.sandbox.tool_download_lock.as_deref(),
        Some(lock.as_path())
    );
}

#[test]
fn overlay_resolves_relative_tool_download_lock_from_overlay_directory() {
    let dir = tempdir().unwrap();
    let base_dir = dir.path().join("user-config");
    let overlay_dir = dir.path().join("project/.ags");
    std::fs::create_dir_all(&base_dir).unwrap();
    std::fs::create_dir_all(&overlay_dir).unwrap();
    let base = base_dir.join("config.toml");
    let overlay = overlay_dir.join("config.toml");
    let lock = overlay_dir.join("tool-downloads.content.lock.json");
    std::fs::write(&base, minimal_sandbox_toml()).unwrap();
    std::fs::write(
        &lock,
        format!(
            r#"[{{"id":"tool","download":{{"version":"1","archive":"zip","member":"tool","install_as":"tool","artifacts":{{"x86_64":{{"url":"https://example.com/tool.zip","sha256":"{}"}},"aarch64":{{"url":"https://example.com/tool.zip","sha256":"{}"}}}}}}}}]"#,
            "a".repeat(64),
            "b".repeat(64)
        ),
    )
    .unwrap();
    std::fs::write(
        &overlay,
        "[sandbox]\ntool_download_lock = \"tool-downloads.content.lock.json\"\n",
    )
    .unwrap();

    let cfg = parse_and_validate_with_overlay(&base, Some(&overlay)).unwrap();

    assert_eq!(
        cfg.sandbox.tool_download_lock.as_deref(),
        Some(lock.as_path())
    );
}

#[test]
fn sandbox_rejects_incomplete_tool_download_lock() {
    let dir = tempdir().unwrap();
    let lock = dir.path().join("tool-downloads.lock.json");
    std::fs::write(
        &lock,
        format!(
            r#"[{{"id":"tool","download":{{"version":"1","archive":"zip","member":"tool","install_as":"tool","artifacts":{{"x86_64":{{"url":"https://example.com/tool.zip","sha256":"{}"}}}}}}}}]"#,
            "a".repeat(64)
        ),
    )
    .unwrap();

    let error = parse_err(&format!(
        "tool_download_lock = {}",
        toml::Value::String(lock.display().to_string())
    ));
    assert!(error.contains("must define exactly 'x86_64' and 'aarch64'"));
}

#[test]
fn sandbox_loads_relative_agent_provider_lock() {
    let dir = tempdir().unwrap();
    let config = dir.path().join("config.toml");
    let content = r#"[{
  "agent": "opencode",
  "provider": {
    "type": "github_release",
    "source": {
      "repository": "anomalyco/opencode",
      "release": {"mode": "latest"},
      "archive": "tar.gz",
      "member": "opencode",
      "install_as": "opencode",
      "assets": {
        "x86_64": {"archive": "^opencode-linux-x64-baseline\\.tar\\.gz$"},
        "aarch64": {"archive": "^opencode-linux-arm64\\.tar\\.gz$"}
      }
    }
  }
}]"#;
    let digest = format!("{:x}", Sha256::digest(content.as_bytes()));
    let lock = dir
        .path()
        .join(format!("agent-providers.{digest}.lock.json"));
    std::fs::write(&lock, content).unwrap();
    let toml = format!(
        "{}\nenabled_agents = [\"opencode\"]\nagent_provider_lock = \"{}\"",
        minimal_sandbox_toml(),
        lock.file_name().unwrap().to_string_lossy()
    );

    let cfg = parse_toml_str(&toml, &config).unwrap();

    assert_eq!(
        cfg.sandbox.agent_provider_lock.as_deref(),
        Some(lock.as_path())
    );
    assert_eq!(cfg.sandbox.agent_providers.len(), 1);
    assert_eq!(
        cfg.sandbox.agent_providers[0].agent,
        ags::cli::Agent::Opencode
    );
}

#[test]
fn sandbox_migrates_legacy_opencode_release_source_lock() {
    let dir = tempdir().unwrap();
    let config = dir.path().join("config.toml");
    let content = r#"[{
  "agent": "opencode",
  "github_release": {
    "repository": "example/opencode",
    "release": {"mode": "latest"},
    "archive": "tar.gz",
    "member": "opencode",
    "install_as": "opencode",
    "assets": {
      "x86_64": {"archive": "^opencode-linux-x64\\.tar\\.gz$"},
      "aarch64": {"archive": "^opencode-linux-arm64\\.tar\\.gz$"}
    }
  }
}]"#;
    let digest = format!("{:x}", Sha256::digest(content.as_bytes()));
    let lock = dir
        .path()
        .join(format!("agent-release-sources.{digest}.lock.json"));
    std::fs::write(&lock, content).unwrap();
    let toml = format!(
        "{}\nagent_release_source_lock = \"{}\"",
        minimal_sandbox_toml(),
        lock.file_name().unwrap().to_string_lossy()
    );

    let cfg = parse_toml_str(&toml, &config).unwrap();

    assert_eq!(
        cfg.sandbox.agent_provider_lock.as_deref(),
        Some(lock.as_path())
    );
    assert_eq!(cfg.sandbox.agent_providers.len(), Agent::INSTALLABLE.len());
    let opencode = cfg
        .sandbox
        .agent_providers
        .iter()
        .find(|entry| entry.agent == Agent::Opencode)
        .unwrap();
    let ags::config::AgentProviderPolicy::GithubRelease { source } = &opencode.provider else {
        panic!("expected GitHub release provider");
    };
    assert_eq!(source.repository, "example/opencode");
}

#[test]
fn sandbox_rejects_conflicting_agent_provider_lock_keys() {
    let error = parse_err(
        "enabled_agents = []\nagent_provider_lock = \"new.json\"\nagent_release_source_lock = \"old.json\"",
    );

    assert!(error.contains("must not define both"), "got: {error}");
}

#[test]
fn overlay_resolves_relative_agent_provider_lock_from_overlay_directory() {
    let dir = tempdir().unwrap();
    let base_dir = dir.path().join("user-config");
    let overlay_dir = dir.path().join("project/.ags");
    std::fs::create_dir_all(&base_dir).unwrap();
    std::fs::create_dir_all(&overlay_dir).unwrap();
    let base = base_dir.join("config.toml");
    let overlay = overlay_dir.join("config.toml");
    let content = "[]";
    let digest = format!("{:x}", Sha256::digest(content.as_bytes()));
    let lock = overlay_dir.join(format!("agent-providers.{digest}.lock.json"));
    std::fs::write(&base, minimal_sandbox_toml()).unwrap();
    std::fs::write(&lock, content).unwrap();
    std::fs::write(
        &overlay,
        format!(
            "[sandbox]\nenabled_agents = []\nagent_provider_lock = \"{}\"\n",
            lock.file_name().unwrap().to_string_lossy()
        ),
    )
    .unwrap();

    let cfg = parse_and_validate_with_overlay(&base, Some(&overlay)).unwrap();

    assert_eq!(
        cfg.sandbox.agent_provider_lock.as_deref(),
        Some(lock.as_path())
    );
}

#[test]
fn sandbox_requires_content_addressed_agent_provider_lock_name() {
    let dir = tempdir().unwrap();
    let lock = dir.path().join("agent-providers.lock.json");
    std::fs::write(&lock, "[]").unwrap();

    let error = parse_err(&format!(
        "enabled_agents = []\nagent_provider_lock = {}",
        toml::Value::String(lock.display().to_string())
    ));

    assert!(error.contains("must reference a content-addressed"));
}

#[test]
fn sandbox_rejects_enabled_agent_missing_from_provider_lock() {
    let dir = tempdir().unwrap();
    let content = "[]\n";
    let digest = format!("{:x}", Sha256::digest(content.as_bytes()));
    let lock = dir
        .path()
        .join(format!("agent-providers.{digest}.lock.json"));
    std::fs::write(&lock, content).unwrap();

    let error = parse_err(&format!(
        "enabled_agents = [\"pi\"]\nagent_provider_lock = {}",
        toml::Value::String(lock.display().to_string())
    ));

    assert!(error.contains("includes 'pi' but the agent provider lock does not"));
}

#[test]
fn sandbox_rejects_incompatible_agent_provider() {
    let dir = tempdir().unwrap();
    let content = r#"[{
  "agent": "pi",
  "provider": {"type": "builtin_installer", "installer": "claude"}
}]
"#;
    let digest = format!("{:x}", Sha256::digest(content.as_bytes()));
    let lock = dir
        .path()
        .join(format!("agent-providers.{digest}.lock.json"));
    std::fs::write(&lock, content).unwrap();

    let error = parse_err(&format!(
        "enabled_agents = [\"pi\"]\nagent_provider_lock = {}",
        toml::Value::String(lock.display().to_string())
    ));

    assert!(error.contains("provider is incompatible with agent 'pi'"));
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
fn clipboard_config_parses() {
    let cfg = parse_minimal(
        r#"
[clipboard]
enabled = true
mode = "read"
max_bytes = 1024
approval_required = true
approval_seconds = 60
approve_writes = true

[desktop_passthrough]
wayland = true
"#,
    );
    assert_eq!(cfg.clipboard.mode.to_string(), "read");
    assert_eq!(cfg.clipboard.max_bytes, 1024);
    assert!(cfg.clipboard.approval_required);
    assert_eq!(cfg.clipboard.approval_seconds, 60);
    assert!(cfg.clipboard.approve_writes);
    assert!(cfg.desktop_passthrough.wayland);
}

#[test]
fn invalid_clipboard_mode_rejected() {
    let err = parse_err(
        r#"
[clipboard]
mode = "write"
"#,
    );
    assert!(err.contains("[clipboard].mode must be"), "got: {err}");
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
fn command_secret_expands_only_the_executable() {
    let cfg = parse_minimal(
        r#"
[[secret]]
env = "TOKEN"
command = ["$HOME/.local/bin/credential-helper", "$HOME", "$(whoami)", ""]
"#,
    );
    assert_eq!(cfg.secrets.len(), 1);
    match &cfg.secrets[0].source {
        SecretSource::Command { argv } => {
            let home = std::env::var("HOME").unwrap();
            assert_eq!(argv[0], format!("{home}/.local/bin/credential-helper"));
            assert_eq!(&argv[1..], ["$HOME", "$(whoami)", ""]);
        }
        _ => panic!("expected Command source"),
    }
}

#[test]
fn command_secret_supports_tilde_and_braced_env_expansion() {
    let cfg = parse_minimal(
        r#"
[[secret]]
env = "TILDE_TOKEN"
command = ["~/.local/bin/tilde-helper"]

[[secret]]
env = "BRACED_TOKEN"
command = ["${HOME}/.local/bin/braced-helper"]
"#,
    );
    let home = std::env::var("HOME").unwrap();
    let executables: Vec<&str> = cfg
        .secrets
        .iter()
        .map(|secret| match &secret.source {
            SecretSource::Command { argv } => argv[0].as_str(),
            _ => panic!("expected Command source"),
        })
        .collect();
    assert_eq!(
        executables,
        [
            format!("{home}/.local/bin/tilde-helper"),
            format!("{home}/.local/bin/braced-helper"),
        ]
    );
}

#[test]
fn command_secret_rejects_empty_argv() {
    let err = parse_err(
        r#"
[[secret]]
env = "TOKEN"
command = []
"#,
    );
    assert!(
        err.contains("must include at least one argv element"),
        "got: {err}"
    );
}

#[test]
fn command_secret_rejects_empty_executable() {
    let err = parse_err(
        r#"
[[secret]]
env = "TOKEN"
command = ["", "lookup"]
"#,
    );
    assert!(
        err.contains("command[0] must be a non-empty string"),
        "got: {err}"
    );
}

#[test]
fn command_secret_rejects_bare_or_relative_executables() {
    for executable in ["credential-helper", "./credential-helper"] {
        let err = parse_err(&format!(
            r#"
[[secret]]
env = "TOKEN"
command = ["{executable}", "lookup"]
"#
        ));
        assert!(
            err.contains("must resolve to an absolute executable path"),
            "got: {err}"
        );
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
fn tool_command_secret_has_identical_validated_shape() {
    let cfg = parse_minimal(
        r#"
[[tool]]
name = "qwk"
path = "/usr/bin/qwk"
container_path = "/usr/local/bin/qwk"

[[tool.secret]]
env = "QWK_TOKEN"
command = ["/usr/local/bin/qwk-credential", "lookup", "--literal=$HOME"]
"#,
    );
    assert_eq!(cfg.secrets.len(), 1);
    assert_eq!(cfg.secrets[0].tool.as_deref(), Some("qwk"));
    match &cfg.secrets[0].source {
        SecretSource::Command { argv } => assert_eq!(
            argv,
            &["/usr/local/bin/qwk-credential", "lookup", "--literal=$HOME"]
        ),
        _ => panic!("expected Command source"),
    }
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
    assert_eq!(cfg.update.pi_spec, DEFAULT_PI_SPEC);
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
fn update_pi_spec_rejects_versioned_or_non_package_values() {
    for spec in ["@custom/agent@latest", "https://example.com/pi.tgz"] {
        let err = parse_err(&format!(
            "[update]\npi_spec = {}",
            toml::Value::String(spec.to_owned())
        ));
        assert!(err.contains("unversioned npm package name"), "got: {err}");
    }
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
extra_dnf_packages = ["base-package"]

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
extra_dnf_packages = ["repo-package"]

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
    assert_eq!(cfg.sandbox.extra_dnf_packages, vec!["repo-package"]);
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
fn overlay_rejects_top_level_command_secret() {
    let dir = tempdir().unwrap();
    let base_path = dir.path().join("base.toml");
    let overlay_path = dir.path().join("overlay.toml");
    std::fs::write(&base_path, minimal_sandbox_toml()).unwrap();
    std::fs::write(
        &overlay_path,
        r#"
[[secret]]
env = "TOKEN"
command = ["/tmp/untrusted-helper"]
"#,
    )
    .unwrap();

    let err = parse_and_validate_with_overlay(&base_path, Some(&overlay_path))
        .unwrap_err()
        .to_string();
    assert!(err.contains("repo-local config"), "got: {err}");
    assert!(err.contains("[[secret]] #0.command"), "got: {err}");
}

#[test]
fn overlay_rejects_nested_tool_command_secret() {
    let dir = tempdir().unwrap();
    let base_path = dir.path().join("base.toml");
    let overlay_path = dir.path().join("overlay.toml");
    std::fs::write(&base_path, minimal_sandbox_toml()).unwrap();
    std::fs::write(
        &overlay_path,
        r#"
[[tool]]
name = "helper"
path = "/usr/bin/helper"
container_path = "/usr/local/bin/helper"

[[tool.secret]]
env = "TOKEN"
command = ["/tmp/untrusted-helper"]
"#,
    )
    .unwrap();

    let err = parse_and_validate_with_overlay(&base_path, Some(&overlay_path))
        .unwrap_err()
        .to_string();
    assert!(err.contains("repo-local config"), "got: {err}");
    assert!(err.contains("[[tool]] #0.secret[0].command"), "got: {err}");
}

#[test]
fn overlay_still_accepts_non_command_secret_sources() {
    let dir = tempdir().unwrap();
    let base_path = dir.path().join("base.toml");
    let overlay_path = dir.path().join("overlay.toml");
    std::fs::write(&base_path, minimal_sandbox_toml()).unwrap();
    std::fs::write(
        &overlay_path,
        r#"
[[secret]]
env = "TOKEN"
from_env = "TOKEN"
"#,
    )
    .unwrap();

    let config = parse_and_validate_with_overlay(&base_path, Some(&overlay_path)).unwrap();
    assert!(matches!(config.secrets[0].source, SecretSource::Env { .. }));
}

#[test]
fn file_not_found_produces_io_error() {
    let result = ags::config::parse_and_validate(Path::new("/nonexistent/config.toml"));
    let err = result.unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("failed to read"), "got: {msg}");
}
