use ags::t3::compatibility;
use std::fs;
use std::io::Write;
use std::os::unix::{fs::PermissionsExt, net::UnixListener};
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

fn runner(version: &str) -> String {
    let runner = compatibility::RUNNER
        .replace("@@T3_NODE_ENV_SCRIPT@@", "prepend_path_if_dir() { :; }")
        .replace("@@T3_NODE_SCRIPT_PATH@@", "''")
        .replace("@@T3_ARCHIVE_VERSION@@", &format!("'{version}'"))
        .replace(
            "@@T3_RELEASE_BASE_URL@@",
            "'https://invalid.example/releases'",
        );
    runner
}

fn launch(version: &str) -> String {
    compatibility::LAUNCH
        .replace("@@T3_NODE_ENV_SCRIPT@@", "prepend_path_if_dir() { :; }")
        .replace("@@T3_RUNNER_SCRIPT@@", runner(version).trim_end())
        .replace("@@T3_PICK_PORT_SCRIPT@@", "process.exit(1);")
        .replace("@@T3_WAIT_READY_SCRIPT@@", "process.exit(1);")
        .replace("@@T3_RUNTIME_PORT_SCRIPT@@", "process.exit(1);")
}

struct Fixture {
    root: tempfile::TempDir,
    _agent: UnixListener,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::Builder::new()
            .prefix("ags-t3-")
            .tempdir()
            .unwrap();
        for folder in ["bin", "home", "repo", "runtime", "cache", "data"] {
            fs::create_dir(root.path().join(folder)).unwrap();
        }
        let podman = root.path().join("bin/podman");
        fs::write(&podman, include_str!("fixtures/t3_podman.py")).unwrap();
        fs::set_permissions(&podman, fs::Permissions::from_mode(0o755)).unwrap();
        let agent = UnixListener::bind(root.path().join("cache/ssh-agent.sock")).unwrap();
        fs::write(
            root.path().join("cache/ssh-agent.env"),
            format!(
                "SSH_AUTH_SOCK={}\nSSH_AGENT_PID={}\n",
                root.path().join("cache/ssh-agent.sock").display(),
                std::process::id()
            ),
        )
        .unwrap();
        for args in [
            vec!["init", "-b", "main"],
            vec![
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
        ] {
            assert!(
                Command::new("git")
                    .arg("-C")
                    .arg(root.path().join("repo"))
                    .args(args)
                    .output()
                    .unwrap()
                    .status
                    .success()
            );
        }
        fs::write(root.path().join("secret"), "first-fixture-value").unwrap();
        let secret_command = root.path().join("lookup.py");
        fs::write(
            &secret_command,
            "import pathlib, sys\nprint(pathlib.Path(sys.argv[1]).read_text())\n",
        )
        .unwrap();
        fs::write(
            root.path().join("config.toml"),
            format!(
                r#"[sandbox]
image = "localhost/t3-fixture:latest"
containerfile = {containerfile:?}
cache_dir = {cache:?}
gitconfig_path = {gitconfig:?}
auth_key = {auth:?}
sign_key = {sign:?}
enabled_agents = ["t3"]
[host_ui]
enabled = false
[clipboard]
enabled = false
[[secret]]
env = "T3_TEST_SECRET"
command = [{python:?}, {lookup:?}, {secret:?}]
"#,
                containerfile = root.path().join("Containerfile"),
                cache = root.path().join("cache"),
                gitconfig = root.path().join("gitconfig"),
                auth = root.path().join("auth"),
                sign = root.path().join("sign"),
                python = ags::util::which("python3").unwrap(),
                lookup = secret_command,
                secret = root.path().join("secret")
            ),
        )
        .unwrap();
        let fixture = Self {
            root,
            _agent: agent,
        };
        fixture.generation("0.0.45");
        fixture
    }
    fn command(&self, program: &str) -> Command {
        let mut command = Command::new(program);
        command
            .current_dir(self.root.path().join("repo"))
            .env("HOME", self.root.path().join("home"))
            .env("XDG_DATA_HOME", self.root.path().join("data"))
            .env("XDG_CONFIG_HOME", self.root.path().join("config"))
            .env("XDG_RUNTIME_DIR", self.root.path().join("runtime"))
            .env("AGS_T3_TEST_ROOT", self.root.path())
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    self.root.path().join("bin").display(),
                    std::env::var("PATH").unwrap()
                ),
            );
        command
    }
    fn ags(&self, args: &[&str]) -> Output {
        self.command(env!("CARGO_BIN_EXE_ags"))
            .args(args)
            .output()
            .unwrap()
    }
    fn generation(&self, version: &str) {
        let update = ags::agent_runtime::Update::begin(&self.root.path().join("cache")).unwrap();
        let root = update.path.join("pnpm-home/ags-t3-runtime");
        let bundle = root.join("versions").join(version);
        fs::create_dir_all(&bundle).unwrap();
        for entry in ["client", "resource-monitor", "node_modules"] {
            fs::create_dir(bundle.join(entry)).unwrap();
        }
        fs::write(bundle.join("t3"), "#!/bin/sh\nexit 0\n").unwrap();
        fs::set_permissions(bundle.join("t3"), fs::Permissions::from_mode(0o755)).unwrap();
        fs::write(bundle.join(".install-complete"), version).unwrap();
        fs::write(root.join("version"), version).unwrap();
        update.publish().unwrap();
    }
    fn status(&self) -> serde_json::Value {
        let output = self.ags(&["t3", "status"]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }
    fn ssh(&self, alias: &str, script: String, launch: bool) -> Output {
        let mut command = self.command("timeout");
        command.args(["20", "ssh"]);
        command
            .arg("-F")
            .arg(self.root.path().join("home/.ssh/config"))
            .args(["-o", "ConnectTimeout=5", alias, "sh"]);
        if launch {
            command.args(["-l", "-s", "--", "0123456789abcdef"]);
        } else {
            command.arg("-s");
        }
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(script.as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    }
    fn container(&self) -> serde_json::Value {
        serde_json::from_slice(&fs::read(self.root.path().join("container.json")).unwrap()).unwrap()
    }
    fn events(&self) -> Vec<serde_json::Value> {
        fs::read_to_string(self.root.path().join("events.jsonl"))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let status = self.ags(&["t3", "status"]);
        if let Ok(value) = serde_json::from_slice::<serde_json::Value>(&status.stdout)
            && let Some(pid) = value["owner_pid"].as_u64()
        {
            unsafe {
                libc::kill(pid as i32, libc::SIGTERM);
            }
            let deadline = Instant::now() + Duration::from_secs(3);
            while Instant::now() < deadline
                && self
                    .root
                    .path()
                    .join("runtime/ags")
                    .read_dir()
                    .is_ok_and(|entries| {
                        entries
                            .flatten()
                            .any(|entry| entry.path().join("control.sock").exists())
                    })
            {
                std::thread::sleep(Duration::from_millis(25));
            }
        }
    }
}

#[test]
fn automatic_ssh_start_reuse_disconnect_stop_upgrade_and_owner_recovery() {
    let fixture = Fixture::new();
    let config = fixture.root.path().join("config.toml");
    let output = fixture.ags(&["--agent", "t3", "--config", config.to_str().unwrap()]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let status = fixture.status();
    let alias = status["ssh_alias"].as_str().unwrap().to_owned();
    let owner = status["owner_pid"].as_u64().unwrap();
    let home_mount = fixture.container()["Mounts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|mount| {
            mount["Destination"]
                .as_str()
                .is_some_and(|path| path.ends_with("/home"))
        })
        .unwrap()
        .clone();
    assert_eq!(home_mount["Source"], home_mount["Destination"]);
    let history =
        Path::new(home_mount["Source"].as_str().unwrap()).join(".t3/userdata/history-fixture");
    fs::write(&history, "persistent history").unwrap();
    let first_hash = fixture.container()["secret_hash"].clone();
    for _ in 0..2 {
        let result = fixture.ssh(&alias, launch("0.0.45"), true);
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert!(String::from_utf8_lossy(&result.stdout).contains("external"));
    }
    let pairing = compatibility::PAIR
        .replace("@@T3_STATE_KEY@@", "0123456789abcdef")
        .replace("@@T3_RUNNER_SCRIPT@@", runner("0.0.45").trim_end());
    let result = fixture.ssh(&alias, pairing, false);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&result.stdout).unwrap()["credential"],
        "fixture-pairing"
    );
    let mut forward = fixture.command("timeout");
    forward
        .args(["20", "ssh", "-F"])
        .arg(fixture.root.path().join("home/.ssh/config"))
        .args(["-W", "127.0.0.1:3773", &alias]);
    let mut child = forward
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"forwarding with EOF")
        .unwrap();
    let forwarded = child.wait_with_output().unwrap();
    assert!(
        forwarded.status.success(),
        "{}",
        String::from_utf8_lossy(&forwarded.stderr)
    );
    assert_eq!(forwarded.stdout, b"forwarding with EOF");
    assert_eq!(
        fixture
            .events()
            .iter()
            .filter(|event| event[0] == "create")
            .count(),
        1
    );
    let stop = compatibility::STOP.replace("@@T3_STATE_KEY@@", "0123456789abcdef");
    assert!(fixture.ssh(&alias, stop.clone(), false).status.success());
    assert_eq!(fixture.container()["State"]["Running"], true);
    let before = fixture.events().len();
    let mismatch = fixture.ssh(&alias, launch("0.0.46"), true);
    assert!(!mismatch.status.success());
    assert!(String::from_utf8_lossy(&mismatch.stderr).contains("No runtime was downloaded"));
    assert_eq!(fixture.events().len(), before);
    fixture.generation("0.0.46");
    assert!(fixture.ags(&["t3", "stop"]).status.success());
    assert!(fixture.ssh(&alias, stop, false).status.success());
    assert_eq!(fixture.container()["State"]["Running"], false);
    fs::write(fixture.root.path().join("secret"), "second-fixture-value").unwrap();
    let result = fixture.ssh(&alias, launch("0.0.45"), true);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_ne!(fixture.container()["secret_hash"], first_hash);
    assert_eq!(fixture.status()["runtime"], "0.0.45");
    let upgraded = fixture.ags(&["t3", "upgrade"]);
    assert!(
        upgraded.status.success(),
        "{}",
        String::from_utf8_lossy(&upgraded.stderr)
    );
    assert_eq!(fixture.status()["runtime"], "0.0.46");
    assert_eq!(fs::read_to_string(&history).unwrap(), "persistent history");
    unsafe {
        libc::kill(owner as i32, libc::SIGKILL);
    }
    std::thread::sleep(Duration::from_millis(100));
    let result = fixture.ssh(&alias, launch("0.0.46"), true);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_ne!(fixture.status()["owner_pid"].as_u64().unwrap(), owner);
    assert_eq!(fs::read_to_string(&history).unwrap(), "persistent history");
    let events = fs::read_to_string(fixture.root.path().join("events.jsonl")).unwrap();
    assert!(!events.contains("first-fixture-value") && !events.contains("second-fixture-value"));
}

#[test]
fn simultaneous_cold_connections_start_one_owner_and_container() {
    let fixture = Fixture::new();
    let cache = fixture.root.path().join("cache");
    let selected = ags::agent_runtime::selected(&cache).unwrap();
    let binary = selected.join("pnpm-home/ags-t3-runtime/versions/0.0.45/t3");
    fs::remove_file(&binary).unwrap();
    let config = fixture.root.path().join("config.toml");
    assert!(
        !fixture
            .ags(&["--agent", "t3", "--config", config.to_str().unwrap()])
            .status
            .success()
    );
    let status = fixture.status();
    unsafe {
        libc::kill(status["owner_pid"].as_u64().unwrap() as i32, libc::SIGTERM);
    }
    std::thread::sleep(Duration::from_millis(150));
    fs::write(&binary, "#!/bin/sh\nexit 0\n").unwrap();
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o755)).unwrap();
    let alias = status["ssh_alias"].as_str().unwrap();
    std::thread::scope(|scope| {
        let first = scope.spawn(|| fixture.ssh(alias, launch("0.0.45"), true));
        let second = scope.spawn(|| fixture.ssh(alias, launch("0.0.45"), true));
        for result in [first.join().unwrap(), second.join().unwrap()] {
            assert!(
                result.status.success(),
                "{}",
                String::from_utf8_lossy(&result.stderr)
            );
        }
    });
    assert_eq!(
        fixture
            .events()
            .iter()
            .filter(|event| event[0] == "create")
            .count(),
        1
    );
    assert_eq!(
        fixture
            .events()
            .iter()
            .filter(|event| event[0] == "exec"
                && event[1]
                    .as_str()
                    .is_some_and(|arg| arg.starts_with("--preserve-fds=")))
            .count(),
        1
    );
}

#[test]
fn new_external_worktrees_require_explicit_recreation_and_disabled_t3_rejects_new_work() {
    let fixture = Fixture::new();
    let config = fixture.root.path().join("config.toml");
    assert!(
        fixture
            .ags(&["--agent", "t3", "--config", config.to_str().unwrap()])
            .status
            .success()
    );
    let alias = fixture.status()["ssh_alias"].as_str().unwrap().to_owned();
    let worktree = fixture.root.path().join("new-worktree");
    assert!(
        fixture
            .command("git")
            .args([
                "worktree",
                "add",
                "-b",
                "external-new",
                worktree.to_str().unwrap()
            ])
            .output()
            .unwrap()
            .status
            .success()
    );
    let before = fixture.events().len();
    let response = fixture.ssh(&alias, launch("0.0.45"), true);
    assert!(!response.status.success());
    assert!(String::from_utf8_lossy(&response.stderr).contains("outside the live T3 mount layout"));
    assert_eq!(fixture.events().len(), before);
    assert_eq!(fixture.container()["State"]["Running"], true);
    let upgraded = fixture.ags(&["t3", "upgrade"]);
    assert!(
        upgraded.status.success(),
        "{}",
        String::from_utf8_lossy(&upgraded.stderr)
    );
    assert!(
        fixture.container()["Mounts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|mount| mount["Source"] == worktree.display().to_string()
                && mount["Destination"] == mount["Source"])
    );
    fs::write(
        &config,
        fs::read_to_string(&config)
            .unwrap()
            .replace("enabled_agents = [\"t3\"]", "enabled_agents = []"),
    )
    .unwrap();
    let before = fixture.events().len();
    let response = fixture.ssh(&alias, launch("0.0.45"), true);
    assert!(!response.status.success());
    assert!(String::from_utf8_lossy(&response.stderr).contains("T3 is disabled"));
    assert_eq!(fixture.events().len(), before);
    assert_eq!(fixture.container()["State"]["Running"], true);
    assert!(fixture.ags(&["t3", "stop"]).status.success());
    assert_eq!(fixture.container()["State"]["Running"], false);
}

#[test]
fn revoked_overlay_trust_does_not_silently_boot_with_global_config() {
    let fixture = Fixture::new();
    let main = fixture.root.path().join("repo");
    fs::create_dir(main.join(".ags")).unwrap();
    fs::write(
        main.join(".ags/config.toml"),
        "[sandbox]\nenabled_agents = [\"t3\"]\n",
    )
    .unwrap();
    let trust = fixture
        .root
        .path()
        .join("config/ags/trusted-repo-overlays.txt");
    fs::create_dir_all(trust.parent().unwrap()).unwrap();
    fs::write(&trust, format!("{}\n", main.display())).unwrap();
    let config = fixture.root.path().join("config.toml");
    let boot = fixture.ags(&["--agent", "t3", "--config", config.to_str().unwrap()]);
    assert!(
        boot.status.success(),
        "{}",
        String::from_utf8_lossy(&boot.stderr)
    );
    let alias = fixture.status()["ssh_alias"].as_str().unwrap().to_owned();
    fs::write(trust, "").unwrap();
    let before = fixture.events().len();
    let response = fixture.ssh(&alias, launch("0.0.45"), true);
    assert!(!response.status.success());
    assert!(String::from_utf8_lossy(&response.stderr).contains("overlay is unavailable/untrusted"));
    assert_eq!(fixture.events().len(), before);
    assert_eq!(fixture.container()["State"]["Running"], true);
}
