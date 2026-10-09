use super::{
    registration::{Registration, Settings},
    repository::Repository,
    sidecars::Sidecars,
};
use std::fs;
use std::path::Path;
use std::process::Command;

fn git(root: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn repository_identity_worktree_paths_and_persistent_mount_parent_are_consistent() {
    let temp = tempfile::tempdir().unwrap();
    let main = temp.path().join("main");
    fs::create_dir(&main).unwrap();
    git(&main, &["init", "-b", "main"]);
    git(
        &main,
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--allow-empty",
            "-m",
            "initial",
        ],
    );
    let external = temp.path().join("external");
    git(
        &main,
        &[
            "worktree",
            "add",
            "-b",
            "external",
            external.to_str().unwrap(),
        ],
    );
    let repository = Repository::resolve(&main).unwrap();
    let linked = Repository::resolve(&external).unwrap();
    assert_eq!(repository, linked);
    assert_eq!(repository.main, main);
    let home = temp.path().join("t3-home");
    fs::create_dir(&home).unwrap();
    let registration = Registration {
        schema: 1,
        data_root: temp.path().join("data"),
        control_dir: temp.path().join("control"),
        repository,
        config: temp.path().join("config.toml"),
        overlay: None,
        home: home.clone(),
        settings: Settings {
            browser: false,
            psp: false,
            psp_keep: false,
            yolo: false,
            root: false,
            wayland: false,
            add_dirs: Vec::new(),
            env_names: Vec::new(),
            op_sources: Vec::new(),
        },
    };
    let cache = temp.path().join("cache");
    let generation = crate::agent_runtime::Update::begin(&cache).unwrap();
    generation.publish().unwrap();
    let config = crate::config::parse_toml_str(
        &format!(
            "[sandbox]\nimage = \"localhost/test:latest\"\ncontainerfile = \"/tmp/Containerfile\"\nauth_key = \"/tmp/auth\"\nsign_key = \"/tmp/sign\"\nenabled_agents = [\"t3\"]\ncache_dir = {:?}\ngitconfig_path = {:?}\n",
            cache,
            temp.path().join("gitconfig")
        ),
        &registration.config,
    )
    .unwrap();
    let sidecars = Sidecars {
        _browser: None,
        ui: None,
        clipboard: None,
        relay: None,
        auth: None,
        psp: None,
    };
    let assets = temp.path().join("assets");
    fs::create_dir(&assets).unwrap();
    let first = super::environment_plan::build(
        &registration,
        &config,
        &sidecars,
        &generation.path,
        "0.0.45",
        &assets,
        None,
    )
    .unwrap();
    for path in [&main, &external, &home] {
        assert!(
            first
                .mounts
                .iter()
                .any(|mount| mount.host == *path && mount.container == path.display().to_string())
        );
    }
    let runtime = first
        .mounts
        .iter()
        .find(|mount| mount.container == home.join(".t3/runtime").display().to_string())
        .unwrap();
    assert_eq!(runtime.mode, crate::config::MountMode::Ro);
    assert!(
        first
            .env
            .inline
            .contains(&("HOME".into(), home.display().to_string()))
    );
    assert!(first.env.env_file_entries.is_empty());
    let future = home.join(".t3/worktrees/main/future");
    fs::create_dir_all(future.parent().unwrap()).unwrap();
    git(
        &main,
        &["worktree", "add", "-b", "future", future.to_str().unwrap()],
    );
    git(&future, &["status", "--porcelain"]);
    assert_eq!(
        Repository::resolve(&future).unwrap().id,
        registration.repository.id
    );
    let second = super::environment_plan::build(
        &registration,
        &config,
        &sidecars,
        &generation.path,
        "0.0.45",
        &assets,
        None,
    )
    .unwrap();
    assert_eq!(
        format!("{:?}", first.mounts),
        format!("{:?}", second.mounts),
        "future T3 worktrees must be covered by the pre-mounted home parent"
    );
    assert!(
        !Command::new("git")
            .arg("-C")
            .arg(&main)
            .args(["worktree", "list", "--porcelain"])
            .output()
            .unwrap()
            .stdout
            .windows(8)
            .any(|bytes| bytes == b"prunable")
    );
}

#[test]
fn retained_generation_pinning_rejects_candidates_and_escape_paths() {
    let temp = tempfile::tempdir().unwrap();
    let update = crate::agent_runtime::Update::begin(temp.path()).unwrap();
    assert!(crate::agent_runtime::pin_path(temp.path(), &update.path).is_err());
    update.publish().unwrap();
    let lease = crate::agent_runtime::pin_path(temp.path(), &update.path).unwrap();
    assert_eq!(lease.path, update.path);
    assert!(crate::agent_runtime::pin_path(temp.path(), Path::new("/tmp")).is_err());
}

#[test]
fn managed_providers_keep_shared_homes_and_t3_permission_arguments() {
    let temp = tempfile::tempdir().unwrap();
    let home = temp.path().join("home");
    fs::create_dir_all(home.join(".t3/userdata")).unwrap();
    fs::write(home.join(".t3/userdata/settings.json"), r#"{"providers":{"claudeAgent":{"launchArgs":"--permission-mode acceptEdits"},"codex":{"launchArgs":"--existing-choice"}},"defaultRuntimeMode":"full-access"}"#).unwrap();
    let registration = Registration {
        schema: 1,
        data_root: temp.path().join("data"),
        control_dir: temp.path().join("control"),
        repository: Repository {
            id: "b".repeat(64),
            main: temp.path().join("repo"),
            common: temp.path().join("repo/.git"),
            worktrees: Vec::new(),
            device: 0,
            inode: 0,
        },
        config: temp.path().join("config.toml"),
        overlay: None,
        home: home.clone(),
        settings: Settings {
            browser: false,
            psp: false,
            psp_keep: false,
            yolo: false,
            root: false,
            wayland: false,
            add_dirs: Vec::new(),
            env_names: Vec::new(),
            op_sources: Vec::new(),
        },
    };
    let config = crate::config::parse_toml_str(
        &format!(
            r#"[sandbox]
image = "localhost/test:latest"
containerfile = "/tmp/Containerfile"
cache_dir = {:?}
gitconfig_path = "/tmp/gitconfig"
auth_key = "/tmp/auth"
sign_key = "/tmp/sign"
enabled_agents = ["t3", "claude", "codex", "opencode"]
"#,
            temp.path().join("cache")
        ),
        &registration.config,
    )
    .unwrap();
    let assets = super::providers::prepare(&registration, &config).unwrap();
    let settings: serde_json::Value =
        serde_json::from_slice(&fs::read(home.join(".t3/userdata/settings.json")).unwrap())
            .unwrap();
    assert_eq!(
        settings["providers"]["claudeAgent"]["homePath"],
        "/home/dev/.claude"
    );
    assert_eq!(
        settings["providers"]["codex"]["homePath"],
        "/home/dev/.codex"
    );
    assert_eq!(settings["providers"]["codex"]["setupMode"], "existing");
    assert_eq!(
        settings["providers"]["claudeAgent"]["launchArgs"],
        "--permission-mode acceptEdits"
    );
    assert_eq!(
        settings["providers"]["codex"]["launchArgs"],
        "--existing-choice"
    );
    assert_eq!(settings["defaultRuntimeMode"], "full-access");
    let wrapper = fs::read_to_string(assets.join("claude")).unwrap();
    assert!(wrapper.contains("export HOME=/home/dev"));
    assert!(wrapper.contains("unset CLAUDE_CONFIG_DIR"));
    assert!(wrapper.contains("--settings") && wrapper.contains("guard.sh"));
    assert!(!wrapper.contains("--dangerously-skip-permissions"));
    assert!(wrapper.contains("\"$@\""));
    assert_eq!(settings["providers"]["antigravity"]["enabled"], false);
}
