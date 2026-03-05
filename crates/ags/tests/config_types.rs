use ags::config::{MountKind, MountMode, MountWhen, SecretSource, ValidatedMount, ValidatedSecret};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[test]
fn raw_config_deserializes_minimal_toml() {
    let toml_str = r#"
[sandbox]
image = "localhost/agent-sandbox:latest"
containerfile = "/tmp/Containerfile"
sandbox_pi_dir = "/tmp/sandbox"
host_pi_dir = "/tmp/host"
host_claude_dir = "/tmp/claude"
cache_dir = "/tmp/cache"
gitconfig_path = "/tmp/gitconfig"
auth_key = "/tmp/auth"
sign_key = "/tmp/sign"
"#;
    let raw: ags::config::RawConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(raw.sandbox.image, "localhost/agent-sandbox:latest");
    assert!(raw.mount.is_empty());
    assert!(raw.tool.is_empty());
    assert!(raw.secret.is_empty());
    assert!(!raw.browser.enabled);
    assert_eq!(raw.update.minimum_release_age, 1440);
}

#[test]
fn raw_config_deserializes_mounts_and_tools() {
    let toml_str = r#"
[sandbox]
image = "test:latest"
containerfile = "/tmp/Containerfile"
sandbox_pi_dir = "/tmp/s"
host_pi_dir = "/tmp/h"
host_claude_dir = "/tmp/c"
cache_dir = "/tmp/cache"
gitconfig_path = "/tmp/gc"
auth_key = "/tmp/a"
sign_key = "/tmp/s2"
passthrough_env = ["API_KEY"]

[[mount]]
host = "/home/user/data"
container = "/data"
mode = "rw"
kind = "dir"
optional = true

[[tool]]
name = "kno"
path = "/usr/bin/kno"
container_path = "/usr/local/bin/kno"
optional = true

[[tool.directory]]
host = "/home/user/.kno"
container = "/home/dev/.kno"
mode = "rw"
kind = "dir"
create = true

[[tool.secret]]
env = "KNO_TOKEN"
from_env = "KNO_TOKEN"

[[secret]]
env = "GH_TOKEN"
from_env = "GH_TOKEN"

[[secret]]
env = "GH_TOKEN"
secret_store = { service = "github", username = "user" }
"#;
    let raw: ags::config::RawConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(raw.mount.len(), 1);
    assert_eq!(raw.mount[0].host, "/home/user/data");
    assert!(raw.mount[0].optional);

    assert_eq!(raw.tool.len(), 1);
    assert_eq!(raw.tool[0].name, "kno");
    assert_eq!(raw.tool[0].directory.len(), 1);
    assert!(raw.tool[0].directory[0].create);
    assert_eq!(raw.tool[0].secret.len(), 1);

    assert_eq!(raw.secret.len(), 2);
    assert_eq!(raw.secret[0].from_env.as_deref(), Some("GH_TOKEN"));
    assert!(raw.secret[1].secret_store.is_some());
}

#[test]
fn raw_config_deserializes_browser_section() {
    let toml_str = r#"
[sandbox]
image = "test:latest"
containerfile = "/tmp/cf"
sandbox_pi_dir = "/tmp/s"
host_pi_dir = "/tmp/h"
host_claude_dir = "/tmp/c"
cache_dir = "/tmp/cache"
gitconfig_path = "/tmp/gc"
auth_key = "/tmp/a"
sign_key = "/tmp/s2"

[browser]
enabled = true
command = "google-chrome"
profile_dir = "/tmp/chrome"
debug_port = 9222
pi_skill_path = "/home/dev/browser-tools"
command_args = ["--no-sandbox"]
"#;
    let raw: ags::config::RawConfig = toml::from_str(toml_str).unwrap();
    assert!(raw.browser.enabled);
    assert_eq!(raw.browser.command, "google-chrome");
    assert_eq!(raw.browser.debug_port, 9222);
    assert_eq!(raw.browser.command_args, vec!["--no-sandbox"]);
}

#[test]
fn validated_types_construct_correctly() {
    let mount = ValidatedMount {
        host: PathBuf::from("/home/user/data"),
        container: "/data".to_owned(),
        mode: MountMode::Rw,
        kind: MountKind::Dir,
        when: MountWhen::Always,
        create: false,
        optional: true,
        source: "config".to_owned(),
    };
    assert_eq!(mount.mode.to_string(), "rw");
    assert_eq!(mount.kind.to_string(), "dir");
    assert_eq!(mount.when.to_string(), "always");

    let secret = ValidatedSecret {
        env: "TOKEN".to_owned(),
        source: SecretSource::SecretTool {
            attributes: BTreeMap::from([
                ("service".to_owned(), "github".to_owned()),
                ("username".to_owned(), "user".to_owned()),
            ]),
        },
        origin: "[[secret]] #0".to_owned(),
        tool: None,
    };
    match &secret.source {
        SecretSource::SecretTool { attributes } => {
            assert_eq!(attributes.get("service"), Some(&"github".to_owned()));
        }
        _ => panic!("expected SecretTool"),
    }
}
