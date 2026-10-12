use ags::t3::compatibility;
use std::fs;
use std::io::{Read, Write};
use std::os::unix::{fs::PermissionsExt, net::UnixListener};
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

fn runner(version: &str) -> String {
    compatibility::RUNNER
        .replace("@@T3_NODE_ENV_SCRIPT@@", "prepend_path_if_dir() { :; }")
        .replace("@@T3_NODE_SCRIPT_PATH@@", "''")
        .replace("@@T3_ARCHIVE_VERSION@@", &format!("'{version}'"))
        .replace(
            "@@T3_RELEASE_BASE_URL@@",
            "'https://invalid.example/releases'",
        )
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
extra_dnf_packages = []
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
        let output = self.ags(&["t3", "status", "--repository", "."]);
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stderr),
            String::from_utf8_lossy(&output.stdout)
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
    fn private_dir(&self) -> std::path::PathBuf {
        fs::read_dir(self.root.path().join("data/ags/t3/identity"))
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path()
    }

    fn wait_for_owner_exit(&self, pid: u64) {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let alive = fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|stat| {
                stat.rsplit_once(')')
                    .is_some_and(|(_, fields)| fields.split_whitespace().next() != Some("Z"))
            });
            if !alive {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "owner {pid} did not exit before the deadline"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let status = self.ags(&["t3", "status", "--repository", "."]);
        let value = serde_json::from_slice::<serde_json::Value>(&status.stdout).ok();
        let mut pids: std::collections::HashSet<_> = value
            .as_ref()
            .and_then(|value| value["owner_pid"].as_u64())
            .into_iter()
            .collect();
        if let Ok(identities) = fs::read_dir(self.root.path().join("data/ags/t3/identity")) {
            for entry in identities.flatten() {
                if let Ok(bytes) = fs::read(entry.path().join("owner-process.json"))
                    && let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes)
                    && let Some(pid) = value["pid"].as_u64()
                {
                    pids.insert(pid);
                }
            }
        }
        for pid in pids {
            unsafe {
                libc::kill(pid as i32, libc::SIGTERM);
            }
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

#[test]
fn podman_image_id_formats_and_legacy_metadata_survive_status_reconnect_and_upgrade() {
    for image in ["a".repeat(64), format!("sha256:{}", "a".repeat(64))] {
        let fixture = Fixture::new();
        fs::write(fixture.root.path().join("image-id"), image).unwrap();
        let config = fixture.root.path().join("config.toml");
        let output = fixture.ags(&["--agent", "t3", "--config", config.to_str().unwrap()]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let status = fixture.status();
        let alias = status["ssh_alias"].as_str().unwrap();
        let metadata = fixture.private_dir().join("environment.json");
        let mut spec: serde_json::Value =
            serde_json::from_slice(&fs::read(&metadata).unwrap()).unwrap();
        assert_eq!(spec["image"], format!("sha256:{}", "a".repeat(64)));
        // Existing AGS installations saved Podman's unprefixed .Id verbatim.
        spec["image"] = "a".repeat(64).into();
        fs::write(&metadata, serde_json::to_vec(&spec).unwrap()).unwrap();
        assert_eq!(fixture.status()["runtime"], "0.0.45");
        assert!(
            fixture
                .ags(&["t3", "stop", "--repository", "."])
                .status
                .success()
        );
        unsafe {
            libc::kill(status["owner_pid"].as_u64().unwrap() as i32, libc::SIGTERM);
        }
        fixture.wait_for_owner_exit(status["owner_pid"].as_u64().unwrap());
        let offline = fixture.status();
        assert_eq!(offline["owner_running"], false);
        assert_eq!(offline["runtime"], "0.0.45");
        let result = fixture.ssh(alias, launch("0.0.45"), true);
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert_eq!(
            fixture
                .events()
                .iter()
                .filter(|event| event[0] == "create")
                .count(),
            1
        );
        // Exercise upgrade from legacy metadata as well as fresh inspection.
        fs::write(&metadata, serde_json::to_vec(&spec).unwrap()).unwrap();
        fixture.generation("0.0.46");
        let output = fixture.ags(&["t3", "upgrade", "--repository", "."]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(fixture.status()["runtime"], "0.0.46");
        assert_eq!(
            fixture
                .events()
                .iter()
                .filter(|event| event[0] == "create")
                .count(),
            2
        );
    }
}

#[test]
fn malformed_image_inspection_fails_before_creating_or_persisting_an_environment() {
    for image in ["localhost/test:latest", "sha256:bad", ""] {
        let fixture = Fixture::new();
        fs::write(fixture.root.path().join("image-id"), image).unwrap();
        let config = fixture.root.path().join("config.toml");
        let result = fixture.ags(&["--agent", "t3", "--config", config.to_str().unwrap()]);
        assert!(!result.status.success());
        assert!(String::from_utf8_lossy(&result.stderr).contains("invalid pinned image ID"));
        assert!(!fixture.private_dir().join("environment.json").exists());
        assert!(!fixture.events().iter().any(|event| event[0] == "create"));
    }
}

#[test]
fn folder_picker_defaults_to_checkout_and_preserves_custom_directory_on_upgrade() {
    let fixture = Fixture::new();
    let config = fixture.root.path().join("config.toml");
    let result = fixture.ags(&["--agent", "t3", "--config", config.to_str().unwrap()]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let home = fixture.container()["Mounts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|mount| {
            mount["Destination"]
                .as_str()
                .is_some_and(|path| path.ends_with("/home"))
        })
        .unwrap()["Source"]
        .as_str()
        .unwrap()
        .to_owned();
    let settings_path = Path::new(&home).join(".t3/userdata/settings.json");
    let read_settings = || -> serde_json::Value {
        serde_json::from_slice(&fs::read(&settings_path).unwrap()).unwrap()
    };
    let checkout = fixture.root.path().join("repo");
    assert_eq!(
        read_settings()["addProjectBaseDirectory"],
        checkout.to_str().unwrap()
    );
    assert!(fixture.events().iter().any(|event| {
        event.as_array().is_some_and(|args| {
            args.windows(2)
                .any(|pair| pair[0] == "--workdir" && pair[1] == checkout.to_str().unwrap())
                && args.iter().any(|arg| arg == "serve")
        })
    }));
    if let Ok(binary) = std::env::var("AGS_T3_NATIVE_BINARY") {
        let result = fixture
            .command("node")
            .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/t3_native_server.cjs"))
            .args([
                binary.as_str(),
                Path::new(&home).join(".t3").to_str().unwrap(),
                checkout.to_str().unwrap(),
            ])
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    // Older registrations can have no picker setting or an empty default.
    for value in [
        None,
        Some(serde_json::json!("")),
        Some(serde_json::json!("  ")),
    ] {
        let mut settings = read_settings();
        settings
            .as_object_mut()
            .unwrap()
            .remove("addProjectBaseDirectory");
        if let Some(value) = value {
            settings["addProjectBaseDirectory"] = value;
        }
        fs::write(&settings_path, serde_json::to_vec(&settings).unwrap()).unwrap();
        let result = fixture.ags(&["t3", "upgrade", "--repository", "."]);
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        assert_eq!(
            read_settings()["addProjectBaseDirectory"],
            checkout.to_str().unwrap()
        );
    }
    let mut settings = read_settings();
    settings["addProjectBaseDirectory"] = serde_json::json!("~/.t3/worktrees");
    settings["defaultRuntimeMode"] = serde_json::json!("full-access");
    fs::write(&settings_path, serde_json::to_vec(&settings).unwrap()).unwrap();
    let result = fixture.ags(&["t3", "upgrade", "--repository", "."]);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_eq!(
        read_settings()["addProjectBaseDirectory"],
        "~/.t3/worktrees"
    );
    assert_eq!(read_settings()["defaultRuntimeMode"], "full-access");
}

#[test]
fn desktop_request_variants_launch_and_pair_without_executing_incoming_scripts() {
    let fixture = Fixture::new();
    let config = fixture.root.path().join("config.toml");
    let output = fixture.ags(&["--agent", "t3", "--config", config.to_str().unwrap()]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let alias = fixture.status()["ssh_alias"].as_str().unwrap().to_owned();
    let notice = String::from_utf8_lossy(&output.stdout);
    assert!(notice.contains(&alias));
    assert!(notice.contains("no listening SSH port (leave port at default 22)"));
    assert!(notice.contains("SSH config:"));
    let settings = fixture
        .command("ssh")
        .args(["-G", "-F"])
        .arg(fixture.root.path().join("home/.ssh/config"))
        .arg(&alias)
        .output()
        .unwrap();
    assert!(settings.status.success());
    let settings = String::from_utf8_lossy(&settings.stdout);
    assert!(settings.contains("port 22\n"));
    assert!(settings.contains("user dev\n"));
    assert!(settings.contains("proxycommand ") && settings.contains(" t3 _proxy --registration "));
    let marker = fixture.root.path().join("incoming-script-executed");
    for (script, is_launch) in [
        (include_str!("fixtures/t3_desktop_launch.sh"), true),
        (include_str!("fixtures/t3_desktop_pair.sh"), false),
    ] {
        let script = script.replace("@@EXECUTION_MARKER@@", marker.to_str().unwrap());
        let script = script
            .lines()
            .map(|line| format!("  {line}\r\n"))
            .collect::<String>();
        let result = fixture.ssh(&alias, script.clone(), is_launch);
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let value: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
        if is_launch {
            assert_eq!(value["remotePort"], 3773);
            assert_eq!(value["serverKind"], "external");
        } else {
            assert_eq!(value["credential"], "fixture-pairing");
        }
        assert!(!marker.exists());
        let before = fixture.events().len();
        let mismatch = fixture.ssh(&alias, script.replace("0.0.45", "0.0.46"), is_launch);
        assert!(!mismatch.status.success());
        assert!(String::from_utf8_lossy(&mismatch.stderr).contains("desktop requests 0.0.46"));
        assert_eq!(fixture.events().len(), before);
    }
    assert_eq!(
        fixture
            .events()
            .iter()
            .filter(|event| event[0] == "create")
            .count(),
        1
    );
    assert_eq!(fixture.container()["State"]["Running"], true);
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
    let mut input = child.stdin.take().unwrap();
    let initial = b"streamed binary forwarding\0\xff";
    input.write_all(initial).unwrap();
    let mut echoed = vec![0; initial.len()];
    child
        .stdout
        .as_mut()
        .unwrap()
        .read_exact(&mut echoed)
        .unwrap();
    assert_eq!(echoed, initial, "forwarding must stream before stdin EOF");
    input.write_all(b"response after EOF").unwrap();
    drop(input);
    let forwarded = child.wait_with_output().unwrap();
    assert!(
        forwarded.status.success(),
        "{}",
        String::from_utf8_lossy(&forwarded.stderr)
    );
    assert_eq!(forwarded.stdout, b"response after EOF");
    assert!(fixture.events().iter().any(|event| {
        event
            .as_array()
            .is_some_and(|args| args.iter().any(|arg| arg == "/run/ags-t3/forward-tcp.js"))
    }));
    assert!(!fixture.events().iter().any(|event| {
        event
            .as_array()
            .is_some_and(|args| args.iter().any(|arg| arg == "socat"))
    }));
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
    assert!(
        fixture
            .ags(&["t3", "stop", "--repository", "."])
            .status
            .success()
    );
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
    let upgraded = fixture.ags(&["t3", "upgrade", "--repository", "."]);
    assert!(
        upgraded.status.success(),
        "{}",
        String::from_utf8_lossy(&upgraded.stderr)
    );
    assert_eq!(fixture.status()["runtime"], "0.0.46");
    assert_eq!(fs::read_to_string(&history).unwrap(), "persistent history");
    let upgraded_owner = fixture.status()["owner_pid"].as_u64().unwrap();
    assert_ne!(
        upgraded_owner, owner,
        "explicit upgrade must load the current AGS executable"
    );
    unsafe {
        libc::kill(upgraded_owner as i32, libc::SIGKILL);
    }
    fixture.wait_for_owner_exit(upgraded_owner);
    let result = fixture.ssh(&alias, launch("0.0.46"), true);
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    assert_ne!(
        fixture.status()["owner_pid"].as_u64().unwrap(),
        upgraded_owner
    );
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
    fixture.wait_for_owner_exit(status["owner_pid"].as_u64().unwrap());
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
                && event
                    .as_array()
                    .is_some_and(|args| args.iter().any(|arg| arg
                        .as_str()
                        .is_some_and(|arg| arg.starts_with("--preserve-fds=")))))
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
    let upgraded = fixture.ags(&["t3", "upgrade", "--repository", "."]);
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
    assert!(
        fixture
            .ags(&["t3", "stop", "--repository", "."])
            .status
            .success()
    );
    assert_eq!(fixture.container()["State"]["Running"], false);
}

#[path = "support/t3_management_tests.rs"]
mod management_tests;

#[path = "support/t3_review_tests.rs"]
mod review_tests;

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
