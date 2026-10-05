//! Process-level launch checks using an isolated fake Podman, never a host container.
use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};

struct Fixture {
    root: tempfile::TempDir,
    workspace: PathBuf,
    config: PathBuf,
    log: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let workspace = root.path().join("workspace");
        let bin = root.path().join("bin");
        for path in [&workspace, &bin, &root.path().join("tmp")] {
            fs::create_dir_all(path).unwrap();
        }
        let config = root.path().join("config.toml");
        fs::write(
            &config,
            format!(
                r#"
[sandbox]
image = "localhost/base:test"
containerfile = "{root}/Containerfile"
cache_dir = "{root}/cache"
gitconfig_path = "{root}/gitconfig"
auth_key = "{root}/auth"
sign_key = "{root}/sign"
enabled_agents = []
"#,
                root = root.path().display()
            ),
        )
        .unwrap();
        let log = root.path().join("podman.jsonl");
        let podman = bin.join("podman");
        fs::write(
            &podman,
            r#"#!/usr/bin/python3
import json, os, sys
args = sys.argv[1:]
with open(os.environ['PODMAN_LOG'], 'a') as log:
    log.write(json.dumps(args) + '\n')
if args[:2] == ['version', '--format']:
    print('5.6.0')
    sys.exit(0)
if args[:2] == ['image', 'exists']:
    sys.exit(1 if os.environ.get('IMAGE_MISSING') else 0)
if args[0] == 'build':
    sys.exit(0)
if args[0] == 'run':
    if os.environ.get('READ_STDIN'):
        sys.stdout.write(sys.stdin.read())
    sys.exit(int(os.environ.get('RUN_EXIT', '0')))
sys.exit(91)
"#,
        )
        .unwrap();
        fs::set_permissions(podman, fs::Permissions::from_mode(0o755)).unwrap();
        Self {
            root,
            workspace,
            config,
            log,
        }
    }

    fn command(&self, flags: &[&str]) -> Command {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_ags"));
        cmd.env_clear()
            .env(
                "PATH",
                format!("{}:/usr/bin:/bin", self.root.path().join("bin").display()),
            )
            .env("HOME", self.root.path().join("home"))
            .env("XDG_CONFIG_HOME", self.root.path().join("config-home"))
            .env("XDG_RUNTIME_DIR", self.root.path().join("runtime"))
            .env("TMPDIR", self.root.path().join("tmp"))
            .env("PODMAN_LOG", &self.log)
            .current_dir(&self.workspace)
            .args([
                "--config",
                self.config.to_str().unwrap(),
                "--agent",
                "shell",
                "--lockdown",
            ])
            .args(flags);
        cmd
    }

    fn events(&self) -> Vec<Vec<String>> {
        fs::read_to_string(&self.log)
            .unwrap_or_default()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    }

    fn add_overlay(&self, trusted: bool, contents: &str) -> PathBuf {
        let status = Command::new("git")
            .args(["init", "--quiet"])
            .arg(&self.workspace)
            .status()
            .unwrap();
        assert!(status.success());
        let overlay = self.workspace.join(".ags/config.toml");
        fs::create_dir_all(overlay.parent().unwrap()).unwrap();
        fs::write(&overlay, contents).unwrap();
        let store = self
            .root
            .path()
            .join("config-home/ags/trusted-repo-overlays.txt");
        if trusted {
            fs::create_dir_all(store.parent().unwrap()).unwrap();
            fs::write(
                &store,
                format!("{}\n", self.workspace.canonicalize().unwrap().display()),
            )
            .unwrap();
        }
        store
    }
}

fn assert_success(output: &Output) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn headless_launch_passes_stdin_name_and_timeout_and_removes_env_file() {
    let fixture = Fixture::new();
    let mut child = fixture
        .command(&[
            "--tty=false",
            "--container-name=job-42",
            "--timeout-seconds=30",
        ])
        .env("READ_STDIN", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"input without a terminal\n")
        .unwrap();
    let output = child.wait_with_output().unwrap();
    assert_success(&output);
    assert_eq!(output.stdout, b"input without a terminal\n");
    let events = fixture.events();
    let run = events.iter().find(|args| args[0] == "run").unwrap();
    assert!(run.windows(2).any(|args| args == ["--name", "job-42"]));
    for flag in ["--rm", "-i", "--log-driver=none", "--timeout=30"] {
        assert!(run.iter().any(|arg| arg == flag));
    }
    assert!(!run.iter().any(|arg| arg == "-it"));
    let env_file = &run[run.iter().position(|arg| arg == "--env-file").unwrap() + 1];
    assert!(!PathBuf::from(env_file).exists());
}

#[test]
fn headless_missing_image_fails_without_building() {
    let fixture = Fixture::new();
    let output = fixture
        .command(&["--tty=false"])
        .env("IMAGE_MISSING", "1")
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("require a preinstalled sandbox image")
    );
    assert!(
        fixture
            .events()
            .iter()
            .any(|args| args[..2] == ["image", "exists"])
    );
    assert!(
        !fixture
            .events()
            .iter()
            .any(|args| args[0] == "build" || args[0] == "run")
    );
}

#[test]
fn tty_default_still_builds_missing_images() {
    let fixture = Fixture::new();
    let output = fixture
        .command(&[])
        .env("IMAGE_MISSING", "1")
        .output()
        .unwrap();
    assert_success(&output);
    let events = fixture.events();
    assert!(events.iter().any(|args| args[0] == "build"));
    let run = events.iter().find(|args| args[0] == "run").unwrap();
    assert!(run.iter().any(|arg| arg == "-it"));
}

#[test]
fn headless_runs_without_payloads_allow_remote_podman() {
    for variable in ["CONTAINER_HOST", "CONTAINER_CONNECTION"] {
        let fixture = Fixture::new();
        let output = fixture
            .command(&["--tty=false"])
            .env(variable, "remote-fixture")
            .output()
            .unwrap();
        assert_success(&output);
        assert!(!fixture.events().iter().any(|args| args[0] == "system"));
    }
}

#[test]
fn headless_failure_returns_agent_exit_code_without_a_network_retry() {
    for code in [42, 125] {
        let fixture = Fixture::new();
        let output = fixture
            .command(&["--tty=false"])
            .env("RUN_EXIT", code.to_string())
            .output()
            .unwrap();
        assert_eq!(output.status.code(), Some(code));
        assert_eq!(
            fixture
                .events()
                .iter()
                .filter(|args| args[0] == "run")
                .count(),
            1
        );
    }
}

#[test]
fn disabling_repo_config_bypasses_trusted_overlay() {
    let fixture = Fixture::new();
    let store = fixture.add_overlay(true, "[sandbox]\nimage = 'localhost/overlay:test'\n");
    let trust_before = fs::read(&store).unwrap();
    let output = fixture.command(&["--tty=false"]).output().unwrap();
    assert_success(&output);
    assert!(
        fixture
            .events()
            .iter()
            .any(|args| args.iter().any(|arg| arg == "localhost/overlay:test"))
    );
    fs::remove_file(&fixture.log).unwrap();
    let output = fixture
        .command(&["--tty=false", "--no-repo-config"])
        .output()
        .unwrap();
    assert_success(&output);
    assert!(
        fixture
            .events()
            .iter()
            .any(|args| args.iter().any(|arg| arg == "localhost/base:test"))
    );
    assert!(
        !fixture
            .events()
            .iter()
            .any(|args| args.iter().any(|arg| arg == "localhost/overlay:test"))
    );
    assert_eq!(fs::read(store).unwrap(), trust_before);
}

#[test]
fn disabling_repo_config_skips_untrusted_prompt_and_invalid_trusted_config() {
    for trusted in [false, true] {
        let fixture = Fixture::new();
        let store = fixture.add_overlay(trusted, "invalid TOML [[[");
        let output = fixture
            .command(&["--tty=false", "--no-repo-config"])
            .output()
            .unwrap();
        assert_success(&output);
        assert!(!String::from_utf8_lossy(&output.stderr).contains("repo-local"));
        assert_eq!(store.exists(), trusted);
        if trusted {
            let output = fixture.command(&["--tty=false"]).output().unwrap();
            assert_eq!(output.status.code(), Some(2));
        }
    }
}
