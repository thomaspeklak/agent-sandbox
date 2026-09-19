use ags::config::{MountKind, MountMode, MountWhen, SecretSource, ValidatedMount, ValidatedSecret};
use std::collections::BTreeMap;
use std::path::PathBuf;

#[test]
fn raw_config_deserializes_minimal_toml() {
    let toml_str = r#"
[sandbox]
image = "localhost/agent-sandbox:latest"
containerfile = "/tmp/Containerfile"
cache_dir = "/tmp/cache"
gitconfig_path = "/tmp/gitconfig"
auth_key = "/tmp/auth"
sign_key = "/tmp/sign"
"#;
    let raw: ags::config::RawConfig = toml::from_str(toml_str).unwrap();
    assert_eq!(raw.sandbox.image, "localhost/agent-sandbox:latest");
    assert!(raw.sandbox.tool_download_lock.is_empty());
    assert!(raw.sandbox.agent_provider_lock.is_empty());
    assert_eq!(
        raw.sandbox.extra_dnf_packages,
        ags::config::DEFAULT_EXTRA_DNF_PACKAGES
    );
    assert!(raw.mount.is_empty());
    assert!(raw.tool.is_empty());
    assert!(raw.secret.is_empty());
    assert!(!raw.browser.enabled);
    assert!(raw.clipboard.enabled);
    assert_eq!(raw.clipboard.mode, "readwrite");
    assert!(raw.clipboard.approval_required);
    assert_eq!(raw.clipboard.approval_seconds, 300);
    assert!(!raw.clipboard.approve_writes);
    assert!(!raw.desktop_passthrough.wayland);
    assert_eq!(raw.update.minimum_release_age, 1440);
}

const FINAL_RECIPE: &str = include_str!("../../../config/Containerfile");
const OS_BASELINE_RECIPE: &str = include_str!("../../../config/image/os-baseline.Containerfile");
const VENDOR_RECIPE: &str = include_str!("../../../config/image/vendor-tool.Containerfile");

#[test]
fn generated_config_and_os_baseline_use_canonical_package_defaults() {
    let raw: ags::config::RawConfig = toml::from_str(ags::config::DEFAULT_CONFIG).unwrap();
    assert_eq!(
        raw.sandbox.extra_dnf_packages,
        ags::config::DEFAULT_EXTRA_DNF_PACKAGES
    );

    let recipe = OS_BASELINE_RECIPE;
    let argument = recipe
        .lines()
        .find_map(|line| line.strip_prefix("ARG EXTRA_DNF_PACKAGES=\""))
        .and_then(|value| value.strip_suffix('"'))
        .unwrap();
    assert_eq!(
        argument.split_whitespace().collect::<Vec<_>>(),
        ags::config::DEFAULT_EXTRA_DNF_PACKAGES
    );

    let baseline = recipe
        .lines()
        .find_map(|line| line.strip_prefix("RUN BASE_DNF_PACKAGES=\""))
        .and_then(|value| value.split('"').next())
        .unwrap();
    assert_eq!(
        baseline.split_whitespace().collect::<Vec<_>>(),
        ags::config::BASE_DNF_PACKAGES
    );
    let copr = recipe
        .find("dnf -y copr enable jdxcode/mise")
        .expect("mise requires its upstream COPR repository");
    let plugins = recipe
        .find("dnf -y install dnf5-plugins")
        .expect("minimal Fedora needs the DNF5 COPR plugin");
    let strict_repos = recipe
        .find("skip_if_unavailable=False")
        .expect("update checks must not skip unavailable repositories");
    let baseline_install = recipe.find("RUN BASE_DNF_PACKAGES=").unwrap();
    let extras_install = recipe.find("ARG EXTRA_DNF_PACKAGES=").unwrap();
    assert!(plugins < copr && copr < strict_repos && strict_repos < baseline_install);
    assert!(baseline_install < extras_install);
    assert!(ags::config::BASE_DNF_PACKAGES.contains(&"mise"));
    assert!(baseline.split_whitespace().any(|package| package == "mise"));

    let example: ags::config::RawConfig =
        toml::from_str(include_str!("../../../config/config.example.toml")).unwrap();
    assert_eq!(
        example.sandbox.extra_dnf_packages,
        ags::config::DEFAULT_EXTRA_DNF_PACKAGES
    );
}

#[test]
fn final_recipe_only_assembles_prebuilt_components() {
    for forbidden in [
        "dnf ",
        "curl ",
        "cargo build",
        "rustup-init",
        "rustup toolchain",
        "npm install",
        "ARG BR_VERSION",
    ] {
        assert!(
            !FINAL_RECIPE.contains(forbidden),
            "final assembly must not run `{forbidden}`"
        );
    }
    for marker in ["# @ags-vendor-stages@", "# @ags-vendor-copies@"] {
        assert_eq!(
            FINAL_RECIPE.lines().filter(|line| *line == marker).count(),
            1
        );
    }
    let glimpse = FINAL_RECIPE
        .find("COPY --from=glimpse /out/glimpse-shim /opt/ags/glimpse-shim")
        .unwrap();
    let vendor = FINAL_RECIPE.find("\n# @ags-vendor-copies@").unwrap();
    let uv = FINAL_RECIPE.find("COPY uv.toml /etc/uv/uv.toml").unwrap();
    let tmux = FINAL_RECIPE.find("COPY --chown=dev:dev tmux.conf").unwrap();
    assert!(vendor < uv && glimpse < uv && uv < tmux);
}

#[test]
fn vendor_recipe_verifies_and_extracts_only_the_declared_member() {
    let block = VENDOR_RECIPE
        .split_once("RUN set -eu;")
        .map(|(_, block)| block.split_whitespace().collect::<Vec<_>>().join(" "))
        .expect("vendor extraction RUN block");

    let verify = block.find("sha256sum -c -").unwrap();
    let extract = block
        .find("unzip -p \"$archive\" \"$archive_member\"")
        .unwrap();
    assert!(verify < extract);
    assert!(block.contains("tar -xOzf \"$archive\" -- \"$archive_member\""));
    assert!(block.contains("tar -xOJf \"$archive\" -- \"$archive_member\""));
    assert!(block.contains("test \"$(wc -l < \"$found\")\" -eq 1;"));
    assert!(block.contains("install -D -m 0755 \"$binary\" \"/out/$TOOL_INSTALL_AS\""));
    assert!(VENDOR_RECIPE.contains("FROM scratch\nCOPY --from=work /out/ /out/"));
}

#[test]
fn final_image_recreates_and_executes_the_pnpm_launcher() {
    assert!(FINAL_RECIPE.contains(
        "COPY --from=pnpm /usr/local/lib/node_modules/pnpm/ /usr/local/lib/node_modules/pnpm/"
    ));
    assert!(!FINAL_RECIPE.contains("COPY --from=pnpm /usr/local/bin/pnpm"));
    assert!(FINAL_RECIPE.contains("require('/usr/local/lib/node_modules/pnpm/package.json')"));
    assert!(
        FINAL_RECIPE.contains("ln -s \"../lib/node_modules/pnpm/$pnpm_bin\" /usr/local/bin/pnpm")
    );
    assert!(FINAL_RECIPE.contains("test -L /usr/local/bin/pnpm"));
    assert!(FINAL_RECIPE.contains("/usr/local/bin/pnpm --version"));
}

#[test]
fn os_baseline_uses_pnpm_yaml_for_non_auth_settings() {
    let containerfile = OS_BASELINE_RECIPE;
    assert!(containerfile.contains("ignoreScripts: true\\nstoreDir: /usr/local/pnpm/.store\\nglobalBinDir: /usr/local/pnpm/bin\\n' > /home/dev/.config/pnpm/config.yaml"));
    assert!(!containerfile.contains("/home/dev/.config/pnpm/rc"));
}

#[test]
fn os_baseline_precreates_xdg_data_home_before_chown() {
    let (_, user_setup) = OS_BASELINE_RECIPE
        .split_once("RUN useradd")
        .expect("image user setup block");
    let (before_chown, _) = user_setup
        .split_once("chown -R dev:dev /workspace /home/dev")
        .expect("dev home ownership setup");

    assert!(before_chown.contains("/home/dev/.local/share"));
}

#[test]
fn image_uses_conservative_system_wide_uv_policy() {
    let policy = include_str!("../../../config/uv.toml");

    assert!(FINAL_RECIPE.contains("COPY uv.toml /etc/uv/uv.toml"));
    assert!(policy.contains("exclude-newer = \"1 week\""));
    assert!(policy.contains("index-strategy = \"first-index\""));
    assert!(policy.contains("verify-hashes = true"));
    assert!(!policy.contains("require-hashes"));
    assert!(!policy.contains("no-build"));
    assert!(!policy.contains("malware-check"));
}

#[test]
fn raw_config_deserializes_mounts_and_tools() {
    let toml_str = r#"
[sandbox]
image = "test:latest"
containerfile = "/tmp/Containerfile"
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
command = ["/usr/bin/kno-credential", "lookup"]

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
    assert_eq!(
        raw.tool[0].secret[0].command.as_deref(),
        Some(["/usr/bin/kno-credential".to_owned(), "lookup".to_owned()].as_slice())
    );

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
