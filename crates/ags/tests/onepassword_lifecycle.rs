use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

const SENTINEL: &str = "fake-op-lifecycle-sentinel";

fn write_executable(path: &Path, body: &str) {
    fs::write(path, body).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

fn config_toml(root: &Path) -> String {
    let containerfile = root.join("Containerfile");
    fs::write(&containerfile, "FROM scratch\n").unwrap();
    for path in ["pi", "claude", "codex", "gemini", "opencode"] {
        fs::create_dir_all(root.join(path)).unwrap();
    }
    fs::write(root.join(".claude.json"), "{}\n").unwrap();
    format!(
        r#"
[sandbox]
image = "localhost/agent-sandbox:latest"
containerfile = "{containerfile}"
cache_dir = "{root}/cache"
gitconfig_path = "{root}/gitconfig"
auth_key = "{root}/auth"
sign_key = "{root}/sign"

[browser]
enabled = false
command = "google-chrome"
profile_dir = "/tmp/chrome"
debug_port = 9222

[[agent_mount]]
host = "{root}/.claude.json"
container = "/home/dev/.claude.json"
kind = "file"

[[agent_mount]]
host = "{root}/claude"
container = "/home/dev/.claude"

[[agent_mount]]
host = "{root}/codex"
container = "/home/dev/.codex"

[[agent_mount]]
host = "{root}/pi"
container = "/home/dev/.pi"

[[agent_mount]]
host = "{root}/opencode"
container = "/home/dev/.config/opencode"

[[agent_mount]]
host = "{root}/gemini"
container = "/home/dev/.gemini"
"#,
        root = root.display(),
        containerfile = containerfile.display(),
    )
}

struct Fixture {
    root: tempfile::TempDir,
    config: PathBuf,
    bin: PathBuf,
    log: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let config = root.path().join("ags.toml");
        fs::write(&config, config_toml(root.path())).unwrap();
        let bin = root.path().join("bin");
        fs::create_dir(&bin).unwrap();
        let log = root.path().join("events.log");
        write_executable(
            &bin.join("op"),
            r#"#!/usr/bin/env python3
import json, os, sys
open(os.environ['FIXTURE_LOG'], 'a').write('op ' + repr(sys.argv[1:]) + '\n')
if os.environ.get('FIXTURE_OP_FAIL'):
    raise SystemExit(23)
if os.environ.get('FIXTURE_REMOVE_PODMAN'):
    os.remove(os.path.join(os.path.dirname(sys.argv[0]), 'podman'))
values = {
    'readonly item': 'fake-op-lifecycle-sentinel',
    'first': 'first-item-value',
    'second': 'second-item-value',
}
sys.stdout.write(json.dumps({'category': 'SECURE_NOTE', 'fields': [
    {'label': 'FIXTURE_SECRET', 'value': values[sys.argv[3]]},
    {'label': 'EMPTY', 'value': ''},
]}))
"#,
        );
        write_executable(
            &bin.join("podman"),
            "#!/usr/bin/env python3\nimport json, os, sys\nargs = sys.argv[1:]\nopen(os.environ['FIXTURE_LOG'], 'a').write('podman ' + json.dumps(args) + '\\n')\nif args[:2] == ['version', '--format']:\n    raise SystemExit(1)\nif '--pull=never' in args and '--name' not in args:\n    print('slirp4netns has been removed; use pasta', file=sys.stderr)\n    raise SystemExit(125)\nif args[:2] == ['image', 'exists'] or '--entrypoint' in args:\n    raise SystemExit(0)\npreserve = next((arg for arg in args if arg.startswith('--preserve-fds=')), None)\nif preserve is None:\n    open(os.environ['FIXTURE_LOG'], 'a').write('final-no-payload\\n')\n    raise SystemExit(0)\nfd_count = int(preserve.removeprefix('--preserve-fds='))\nenv_file = args[args.index('--env-file') + 1]\nif any('fake-op-lifecycle-sentinel' in value for value in os.environ.values()) or 'fake-op-lifecycle-sentinel' in open(env_file).read():\n    raise SystemExit(83)\nvalues = [json.loads(os.read(3 + offset, 1024 * 1024))['fields'][0]['value'] for offset in range(fd_count)]\nif values not in [['fake-op-lifecycle-sentinel'], ['first-item-value', 'second-item-value']]:\n    raise SystemExit(82)\nopen(os.environ['FIXTURE_LOG'], 'a').write('final-fd-ok\\n')\nif os.environ.get('FIXTURE_PODMAN_FAIL'):\n    raise SystemExit(42)\nif os.environ.get('FIXTURE_NETWORK_RETRY'): \n    state = os.environ['FIXTURE_STATE']\n    count = int(open(state).read()) if os.path.exists(state) else 0\n    open(state, 'w').write(str(count + 1))\n    if count == 0:\n        raise SystemExit(125)\nif os.environ.get('FIXTURE_BLOCK'):\n    import time\n    while os.getppid() != 1:\n        time.sleep(0.05)\n    raise SystemExit(130)\n",
        );
        // Avoid starting a real ssh-agent during this process-level test.
        write_executable(
            &bin.join("ssh-agent"),
            "#!/bin/sh\necho \"SSH_AGENT_PID=$$; export SSH_AGENT_PID;\"\n",
        );
        write_executable(&bin.join("fuser"), "#!/bin/sh\nexit 0\n");
        Self {
            root,
            config,
            bin,
            log,
        }
    }

    fn command(&self, args: &[&str]) -> Command {
        let path = std::env::join_paths(std::iter::once(self.bin.clone()).chain(
            std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
        ))
        .unwrap();
        let mut command = Command::new(env!("CARGO_BIN_EXE_ags"));
        command
            .args(args)
            .env("PATH", path)
            .env("FIXTURE_LOG", &self.log)
            .env("FIXTURE_STATE", self.root.path().join("podman-state"))
            .env("XDG_RUNTIME_DIR", self.root.path().join("runtime"))
            .env("XDG_CACHE_HOME", self.root.path().join("cache-home"));
        command
    }

    fn run(&self, args: &[&str]) -> std::process::Output {
        self.command(args).output().unwrap()
    }

    fn run_with(&self, args: &[&str], network_retry: bool) -> std::process::Output {
        let mut command = self.command(args);
        if network_retry {
            command.env("FIXTURE_NETWORK_RETRY", "1");
        }
        command.output().unwrap()
    }

    fn events(&self) -> String {
        fs::read_to_string(&self.log).unwrap_or_default()
    }
}

fn bootstrap_dirs(runtime: &Path) -> Vec<PathBuf> {
    fs::read_dir(runtime)
        .unwrap_or_else(|_| panic!("missing runtime directory: {}", runtime.display()))
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with("ags-onepassword-"))
        })
        .collect()
}

fn regular_file_contents(root: &Path) -> String {
    let mut contents = String::new();
    for entry in walk(root) {
        if entry.is_file() {
            contents.push_str(&fs::read_to_string(entry).unwrap_or_default());
        }
    }
    contents
}

fn walk(root: &Path) -> Vec<PathBuf> {
    fs::read_dir(root)
        .unwrap()
        .flat_map(|entry| {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(&path)
            } else {
                vec![path]
            }
        })
        .collect()
}

#[test]
fn resolves_after_preflight_and_hands_only_an_anonymous_fd_to_podman() {
    let fixture = Fixture::new();
    let output = fixture.run(&[
        "--config",
        fixture.config.to_str().unwrap(),
        "--agent",
        "pi",
        "-1",
        "Employee/readonly item",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let events = fixture.events();
    let image_check = events.find("podman [\"image\", \"exists\"").unwrap();
    let binary_check = events.find("--entrypoint").unwrap();
    let lookup = events.find("op ['item', 'get', 'readonly item'").unwrap();
    let handoff = events.find("final-fd-ok").unwrap();
    assert!(image_check < lookup && binary_check < lookup && lookup < handoff);
    assert!(events.contains("'--vault', 'Employee', '--format=json', '--reveal']"));
    assert!(events.contains("--preserve-fds=1"));
    assert!(!events.contains(SENTINEL));
    assert!(!String::from_utf8_lossy(&output.stderr).contains(SENTINEL));

    let runtime = fixture.root.path().join("runtime/ags");
    assert!(
        bootstrap_dirs(&runtime).is_empty(),
        "bootstrap directories must be removed after the payload run"
    );
    assert!(!regular_file_contents(&runtime).contains(SENTINEL));
}

#[test]
fn ordered_sources_reach_contiguous_descriptors() {
    let fixture = Fixture::new();
    let output = fixture.run(&[
        "--config",
        fixture.config.to_str().unwrap(),
        "--agent",
        "pi",
        "-1",
        "Employee/first",
        "-1",
        "Employee/second",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let events = fixture.events();
    let first = events.find("op ['item', 'get', 'first'").unwrap();
    let second = events.find("op ['item', 'get', 'second'").unwrap();
    assert!(first < second);
    assert!(events.contains("--preserve-fds=2"));
    assert!(events.contains("final-fd-ok"));
}

#[test]
fn lookup_and_podman_failures_clean_up_without_value_diagnostics() {
    for (name, op_failure, podman_failure, spawn_failure) in [
        ("lookup", true, false, false),
        ("podman", false, true, false),
        ("spawn", false, false, true),
    ] {
        let fixture = Fixture::new();
        let mut command = fixture.command(&[
            "--config",
            fixture.config.to_str().unwrap(),
            "--agent",
            "pi",
            "-1",
            "Employee/readonly item",
        ]);
        if op_failure {
            command.env("FIXTURE_OP_FAIL", "1");
        }
        if podman_failure {
            command.env("FIXTURE_PODMAN_FAIL", "1");
        }
        if spawn_failure {
            command.env("FIXTURE_REMOVE_PODMAN", "1");
        }
        let output = command.output().unwrap();
        assert!(
            !output.status.success(),
            "{name} fixture unexpectedly succeeded"
        );
        let events = fixture.events();
        assert!(events.contains("op ['item', 'get', 'readonly item'"));
        assert!(!String::from_utf8_lossy(&output.stderr).contains(SENTINEL));
        assert!(!events.contains(SENTINEL));
        assert!(bootstrap_dirs(&fixture.root.path().join("runtime/ags")).is_empty());
    }
}

#[test]
fn remote_podman_is_rejected_before_op_lookup() {
    let fixture = Fixture::new();
    let output = fixture
        .command(&[
            "--config",
            fixture.config.to_str().unwrap(),
            "--agent",
            "pi",
            "-1",
            "Employee/readonly item",
        ])
        .env(
            "CONTAINER_HOST",
            "ssh://remote.example.invalid/run/podman.sock",
        )
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("requires local Podman"));
    assert!(!fixture.events().contains("op ["));
}

#[test]
fn no_source_keeps_the_payload_transport_disabled() {
    let fixture = Fixture::new();
    let output = fixture.run(&[
        "--config",
        fixture.config.to_str().unwrap(),
        "--agent",
        "pi",
    ]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let events = fixture.events();
    assert!(events.contains("final-no-payload"));
    assert!(!events.contains("op ["));
    assert!(!events.contains("preserve-fds"));
    assert!(bootstrap_dirs(&fixture.root.path().join("runtime/ags")).is_empty());
}

#[test]
fn retries_network_with_fresh_payloads() {
    let fixture = Fixture::new();
    let output = fixture.run_with(
        &[
            "--config",
            fixture.config.to_str().unwrap(),
            "--agent",
            "pi",
            "-1",
            "Employee/readonly item",
        ],
        true,
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );

    let events = fixture.events();
    assert_eq!(
        events.matches("op ['item', 'get', 'readonly item'").count(),
        2
    );
    assert_eq!(events.matches("final-fd-ok").count(), 2);
    assert!(events.contains("pasta"));
    assert!(!events.contains(SENTINEL));
}

#[test]
fn signal_termination_reaps_the_nonsecret_bootstrap_asset() {
    let fixture = Fixture::new();
    let mut command = fixture.command(&[
        "--config",
        fixture.config.to_str().unwrap(),
        "--agent",
        "pi",
        "-1",
        "Employee/readonly item",
    ]);
    command
        .process_group(0)
        .env("FIXTURE_BLOCK", "1")
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut child = command.spawn().unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    while !fixture.events().contains("final-fd-ok") && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(25));
    }
    assert!(fixture.events().contains("final-fd-ok"));
    assert_eq!(unsafe { libc::kill(-(child.id() as i32), libc::SIGINT) }, 0);
    assert!(!child.wait().unwrap().success());

    let runtime = fixture.root.path().join("runtime/ags");
    let deadline = Instant::now() + Duration::from_secs(5);
    while !bootstrap_dirs(&runtime).is_empty() && Instant::now() < deadline {
        thread::sleep(Duration::from_millis(25));
    }
    assert!(bootstrap_dirs(&runtime).is_empty());
}

#[test]
fn lockdown_rejects_the_source_before_invoking_op() {
    let fixture = Fixture::new();
    let output = fixture.run(&[
        "--config",
        fixture.config.to_str().unwrap(),
        "--agent",
        "pi",
        "--lockdown",
        "-1",
        "Employee/readonly item",
    ]);
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("--lockdown cannot be combined"),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!fixture.events().contains("op "));
}
