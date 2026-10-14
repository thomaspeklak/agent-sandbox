use super::Fixture;
use serde_json::Value;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Output;

fn success(output: &Output) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

struct Environments {
    fixture: Fixture,
}

impl Environments {
    fn new() -> Self {
        let fixture = Fixture::new();
        let root = fixture.root.path();
        fs::create_dir(root.join("containers")).unwrap();
        fs::create_dir(root.join("second repository")).unwrap();
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
            success(
                &fixture
                    .command("git")
                    .arg("-C")
                    .arg(root.join("second repository"))
                    .args(args)
                    .output()
                    .unwrap(),
            );
        }
        fs::copy(root.join("config.toml"), root.join("config-second.toml")).unwrap();
        for (repository, config) in [
            ("repo", "config.toml"),
            ("second repository", "config-second.toml"),
        ] {
            success(
                &fixture
                    .command(env!("CARGO_BIN_EXE_ags"))
                    .current_dir(root.join(repository))
                    .args(["--agent", "t3", "--config"])
                    .arg(root.join(config))
                    .output()
                    .unwrap(),
            );
        }
        Self { fixture }
    }

    fn global(&self, arguments: &[&str]) -> Output {
        self.fixture
            .command(env!("CARGO_BIN_EXE_ags"))
            .current_dir(self.fixture.root.path())
            .args(arguments)
            .output()
            .unwrap()
    }

    fn rows(&self) -> Vec<Value> {
        let output = self.global(&["t3", "status"]);
        success(&output);
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn status(&self, identifier: &str) -> Value {
        let output = self.global(&["t3", "status", identifier]);
        success(&output);
        serde_json::from_slice(&output.stdout).unwrap()
    }

    fn container(&self, identifier: &str) -> PathBuf {
        self.fixture
            .root
            .path()
            .join("containers")
            .join(format!("{identifier}.json"))
    }

    fn pty(&self, action: &str, keys: &str) -> Value {
        self.pty_with_stop(action, keys, None)
    }

    fn pty_with_stop(&self, action: &str, keys: &str, stop_before: Option<&str>) -> Value {
        self.pty_with_hooks(action, keys, stop_before, None)
    }

    fn pty_with_hooks(
        &self,
        action: &str,
        keys: &str,
        stop_before: Option<&str>,
        upgrade_before: Option<&str>,
    ) -> Value {
        let mut command = self.fixture.command("python3");
        if let Some(target) = stop_before {
            command.env("AGS_T3_TEST_STOP_BEFORE_SELECT", target);
        }
        if let Some(target) = upgrade_before {
            command.env("AGS_T3_TEST_UPGRADE_BEFORE_SELECT", target);
        }
        let output = command
            .current_dir(self.fixture.root.path())
            .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/t3_manage_pty.py"))
            .args([keys, env!("CARGO_BIN_EXE_ags"), "t3", action])
            .output()
            .unwrap();
        success(&output);
        let result: Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(
            result["exit"],
            if stop_before.is_some() || upgrade_before.is_some() {
                1
            } else {
                0
            },
            "{result}"
        );
        assert_eq!(result["sent"], true, "{result}");
        assert_eq!(result["restored"], true, "{result}");
        assert!(result["output"].as_str().unwrap().contains("[ ]"));
        result
    }
}

#[test]
fn global_status_is_read_only_cwd_independent_and_empty_inventory_creates_nothing() {
    let fixture = Fixture::new();
    let output = fixture
        .command(env!("CARGO_BIN_EXE_ags"))
        .current_dir(fixture.root.path())
        .args(["t3", "status"])
        .output()
        .unwrap();
    success(&output);
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap(),
        serde_json::json!([])
    );
    assert!(!fixture.root.path().join("data/ags/t3").exists());

    let environments = Environments::new();
    let before = environments.fixture.events().len();
    let rows = environments.rows();
    assert_eq!(rows.len(), 2);
    for row in &rows {
        assert_eq!(row["environment"], row["ssh_alias"]);
        assert_eq!(row["state"], "running");
        assert_eq!(row["owner_running"], true);
        assert_eq!(row["server_running"], true);
        assert_eq!(row["container_running"], true);
        assert_eq!(row["runtime"], "0.0.45");
        assert_eq!(
            environments.status(row["repository_id"].as_str().unwrap())["environment"],
            row["environment"]
        );
    }
    assert!(
        environments.fixture.events()[before..]
            .iter()
            .all(|event| event[0] == "container")
    );
}

#[test]
fn targeted_actions_change_only_one_environment_and_allow_stopped_upgrade() {
    let environments = Environments::new();
    let rows = environments.rows();
    let first = rows[0]["environment"].as_str().unwrap();
    let second = rows[1]["environment"].as_str().unwrap();
    let second_pid = rows[1]["owner_pid"].clone();
    success(&environments.global(&["t3", "stop", first]));
    assert_eq!(environments.status(first)["state"], "stopped");
    assert_eq!(environments.status(second)["running"], true);
    environments.fixture.generation("0.0.46");
    success(&environments.global(&["t3", "upgrade", rows[0]["repository_id"].as_str().unwrap()]));
    assert_eq!(environments.status(first)["runtime"], "0.0.46");
    assert_eq!(environments.status(second)["runtime"], "0.0.45");
    assert_eq!(environments.status(second)["owner_pid"], second_pid);
    let before = environments.fixture.events().len();
    let output = environments.global(&["t3", "stop", "unknown-id"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unknown T3 environment"));
    assert_eq!(before, environments.fixture.events().len());
}

#[test]
fn noninteractive_actions_require_opt_in_and_bulk_upgrade_includes_stopped_registrations() {
    let environments = Environments::new();
    let rows = environments.rows();
    for action in ["stop", "upgrade"] {
        let before = environments.fixture.events().len();
        let output = environments.global(&["t3", action]);
        assert!(!output.status.success());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("requires an interactive terminal")
        );
        assert!(
            environments.fixture.events()[before..]
                .iter()
                .all(|event| event[0] == "container")
        );
    }
    let first = rows[0]["environment"].as_str().unwrap();
    let second = rows[1]["environment"].as_str().unwrap();
    success(&environments.global(&["t3", "stop", first]));
    environments.fixture.generation("0.0.46");
    let output = environments.global(&["t3", "upgrade", "--all"]);
    success(&output);
    assert!(String::from_utf8_lossy(&output.stdout).contains("2 succeeded, 0 failed"));
    assert_eq!(environments.status(first)["state"], "running");
    assert_eq!(environments.status(first)["runtime"], "0.0.46");
    assert_eq!(environments.status(second)["runtime"], "0.0.46");
    success(&environments.global(&["t3", "stop", "--all"]));
    assert!(
        environments
            .rows()
            .iter()
            .all(|row| row["state"] == "stopped")
    );
}

#[test]
fn invalid_registrations_and_dead_servers_are_reported_and_excluded_from_global_actions() {
    let environments = Environments::new();
    let rows = environments.rows();
    let first = rows[0]["environment"].as_str().unwrap();
    let container: Value =
        serde_json::from_slice(&fs::read(environments.container(first)).unwrap()).unwrap();
    unsafe {
        libc::kill(
            container["server_pid"].as_u64().unwrap() as i32,
            libc::SIGTERM,
        );
    }
    std::thread::sleep(std::time::Duration::from_millis(100));
    let registry = environments
        .fixture
        .root
        .path()
        .join("data/ags/t3/registry");
    let invalid = registry.join(format!("{}.json", "f".repeat(64)));
    fs::write(&invalid, "broken json").unwrap();
    fs::set_permissions(&invalid, fs::Permissions::from_mode(0o600)).unwrap();
    let output = environments.global(&["t3", "status"]);
    assert!(!output.status.success());
    let status: Vec<Value> = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(status.len(), 3);
    assert_eq!(
        status
            .iter()
            .filter(|row| row["state"] == "unavailable")
            .count(),
        2
    );
    let output = environments.global(&["t3", "stop", "--all"]);
    success(&output);
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 succeeded, 0 failed"));
    assert!(String::from_utf8_lossy(&output.stderr).contains("Skipping"));
    let container: Value =
        serde_json::from_slice(&fs::read(environments.container(first)).unwrap()).unwrap();
    assert_eq!(
        container["State"]["Running"], true,
        "unhealthy instance must be untouched"
    );
    success(&environments.global(&["t3", "stop", first]));
}

#[test]
fn bulk_upgrade_reports_failures_continues_and_preserves_mount_data_and_identity() {
    let environments = Environments::new();
    let rows = environments.rows();
    let failed = rows[0]["environment"].as_str().unwrap();
    let successful = rows[1]["environment"].as_str().unwrap();
    let registry = environments
        .fixture
        .root
        .path()
        .join("data/ags/t3/registry");
    let registration: Value = serde_json::from_slice(
        &fs::read(registry.join(format!(
            "{}.json",
            rows[0]["repository_id"].as_str().unwrap()
        )))
        .unwrap(),
    )
    .unwrap();
    let config = Path::new(registration["config"].as_str().unwrap());
    fs::write(
        config,
        fs::read_to_string(config)
            .unwrap()
            .replace("enabled_agents = [\"t3\"]", "enabled_agents = []"),
    )
    .unwrap();
    let history = environments
        .fixture
        .root
        .path()
        .join("data/ags/t3/environments")
        .join(rows[1]["repository_id"].as_str().unwrap())
        .join("home/.t3/userdata/history-test");
    fs::write(&history, "persistent history").unwrap();
    let identity = environments
        .fixture
        .root
        .path()
        .join("data/ags/t3/identity")
        .join(rows[1]["repository_id"].as_str().unwrap());
    let before_keys = fs::read_dir(&identity)
        .unwrap()
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().contains("key"))
        .map(|entry| (entry.path(), fs::read(entry.path()).unwrap()))
        .collect::<Vec<_>>();
    assert!(!before_keys.is_empty());
    environments.fixture.generation("0.0.46");
    let output = environments.global(&["t3", "upgrade", "--all"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains(&format!("FAILED {failed}")));
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 succeeded, 1 failed"));
    assert_eq!(environments.status(failed)["runtime"], "0.0.45");
    assert_eq!(environments.status(failed)["running"], true);
    assert_eq!(environments.status(successful)["runtime"], "0.0.46");
    assert_eq!(fs::read_to_string(history).unwrap(), "persistent history");
    for (path, bytes) in before_keys {
        assert_eq!(fs::read(path).unwrap(), bytes);
    }
}

#[test]
fn checkbox_cancellation_empty_selection_and_ctrl_c_leave_every_environment_untouched() {
    let environments = Environments::new();
    let before = environments.rows();
    for (action, keys) in [("upgrade", "0d"), ("stop", "201b"), ("upgrade", "2003")] {
        environments.pty(action, keys);
        assert_eq!(environments.rows(), before);
    }
}

#[test]
fn real_checklist_upgrades_only_checked_environment_and_stops_only_checked_environment() {
    let environments = Environments::new();
    let rows = environments.rows();
    let first = rows[0]["environment"].as_str().unwrap();
    let second = rows[1]["environment"].as_str().unwrap();
    environments.fixture.generation("0.0.46");
    let result = environments.pty("upgrade", "200d"); // Space, Enter: select first only.
    assert!(
        result["output"]
            .as_str()
            .unwrap()
            .contains("1 succeeded, 0 failed")
    );
    assert_eq!(environments.status(first)["runtime"], "0.0.46");
    assert_eq!(environments.status(second)["runtime"], "0.0.45");
    assert_eq!(
        environments.status(second)["owner_pid"],
        rows[1]["owner_pid"]
    );
    environments.pty("stop", "1b5b42200d"); // Down, Space, Enter: select second only.
    assert_eq!(environments.status(first)["running"], true);
    assert_eq!(environments.status(second)["state"], "stopped");
}

#[test]
fn registry_errors_do_not_follow_symlinks_or_allow_ambiguous_environment_selection() {
    let environments = Environments::new();
    let rows = environments.rows();
    let id = rows[0]["repository_id"].as_str().unwrap();
    let registry = environments
        .fixture
        .root
        .path()
        .join("data/ags/t3/registry");
    let original = registry.join(format!("{id}.json"));
    std::os::unix::fs::symlink(&original, registry.join("foreign.json")).unwrap();
    use std::os::unix::ffi::OsStringExt;
    fs::write(
        registry.join(std::ffi::OsString::from_vec(b"invalid-\xff.json".to_vec())),
        "broken json",
    )
    .unwrap();
    let collision = format!("{}{}", &id[..16], "0".repeat(48));
    fs::copy(&original, registry.join(format!("{collision}.json"))).unwrap();
    let output = environments.global(&["t3", "status"]);
    assert!(!output.status.success());
    let status: Vec<Value> = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        status
            .iter()
            .filter(|entry| entry["running"] == true)
            .count(),
        2
    );
    assert!(status.iter().any(|entry| {
        entry["error"]
            .as_str()
            .is_some_and(|error| error.contains("symbolic link"))
    }));
    let before = environments.fixture.events().len();
    let output = environments.global(&["t3", "stop", rows[0]["environment"].as_str().unwrap()]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("ambiguous T3 environment"));
    assert_eq!(environments.fixture.events().len(), before);
    success(&environments.global(&["t3", "stop", id]));
}

#[test]
fn a_stalled_owner_status_is_bounded_and_does_not_hide_other_environments() {
    use std::io::Read;
    use std::os::unix::net::UnixListener;
    use std::time::{Duration, Instant};
    let environments = Environments::new();
    let rows = environments.rows();
    let id = rows[0]["repository_id"].as_str().unwrap();
    let registry = environments
        .fixture
        .root
        .path()
        .join("data/ags/t3/registry");
    let registration: Value =
        serde_json::from_slice(&fs::read(registry.join(format!("{id}.json"))).unwrap()).unwrap();
    let socket = Path::new(registration["control_dir"].as_str().unwrap()).join("control.sock");
    let saved = socket.with_extension("saved");
    fs::rename(&socket, &saved).unwrap();
    let listener = UnixListener::bind(&socket).unwrap();
    let stalled = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = String::new();
        stream.read_to_string(&mut request).unwrap();
        assert_eq!(request, "status\n");
        std::thread::sleep(Duration::from_millis(2500));
    });
    let started = Instant::now();
    let output = environments.global(&["t3", "status"]);
    assert!(started.elapsed() < Duration::from_secs(5));
    assert!(!output.status.success());
    let status: Vec<Value> = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        status
            .iter()
            .filter(|entry| entry["running"] == true)
            .count(),
        1
    );
    assert!(status.iter().any(|entry| {
        entry["error"]
            .as_str()
            .is_some_and(|error| error.contains("owner status unavailable"))
    }));
    stalled.join().unwrap();
    fs::remove_file(&socket).unwrap();
    fs::rename(saved, socket).unwrap();
}

#[test]
fn an_instance_stopped_while_the_checklist_is_open_is_not_restarted() {
    let environments = Environments::new();
    let rows = environments.rows();
    let first = rows[0]["environment"].as_str().unwrap();
    let second = rows[1]["environment"].as_str().unwrap();
    environments.fixture.generation("0.0.46");
    let result = environments.pty_with_stop("upgrade", "200d", Some(first));
    assert!(
        result["output"]
            .as_str()
            .unwrap()
            .contains("environment is no longer working")
    );
    assert_eq!(environments.status(first)["state"], "stopped");
    assert_eq!(environments.status(first)["runtime"], "0.0.45");
    assert_eq!(environments.status(second)["running"], true);
    assert_eq!(
        environments.status(second)["owner_pid"],
        rows[1]["owner_pid"]
    );
}

#[test]
fn global_upgrade_offers_a_stopped_environment_after_configuration_change_blocks_startup() {
    let fixture = Fixture::new();
    let config = fixture.root.path().join("config.toml");
    success(&fixture.ags(&["--agent", "t3", "--config", config.to_str().unwrap()]));
    let before = fixture.status();
    let home = fixture.container()["Mounts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|mount| mount["Destination"].as_str().unwrap().ends_with("/home"))
        .unwrap()["Source"]
        .as_str()
        .unwrap()
        .to_owned();
    let history = Path::new(&home).join(".t3/userdata/recovery-history");
    fs::write(&history, "retained history").unwrap();
    success(&fixture.ags(&["t3", "stop", "--repository", "."]));
    fs::write(
        &config,
        fs::read_to_string(&config).unwrap().replace(
            "localhost/t3-fixture:latest",
            "localhost/t3-fixture:changed",
        ),
    )
    .unwrap();
    let failed = fixture.ags(&["--agent", "t3", "--config", config.to_str().unwrap()]);
    assert!(!failed.status.success());
    assert!(String::from_utf8_lossy(&failed.stderr).contains("configuration changed"));
    assert_eq!(fixture.status()["state"], "stopped");
    let output = fixture
        .command("python3")
        .current_dir(fixture.root.path())
        .arg(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/t3_manage_pty.py"))
        .args(["200d", env!("CARGO_BIN_EXE_ags"), "t3", "upgrade"])
        .output()
        .unwrap();
    success(&output);
    let result: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(result["sent"], true, "{result}");
    assert_eq!(result["exit"], 0, "{result}");
    assert_eq!(result["restored"], true, "{result}");
    assert!(result["output"].as_str().unwrap().contains("[stopped]"));
    assert_eq!(fixture.status()["running"], true);
    assert_eq!(fixture.status()["environment"], before["environment"]);
    assert_eq!(fs::read_to_string(history).unwrap(), "retained history");
}

#[test]
fn global_upgrade_recovers_an_ownerless_container_and_skips_invalid_registrations() {
    let fixture = Fixture::new();
    let config = fixture.root.path().join("config.toml");
    success(&fixture.ags(&["--agent", "t3", "--config", config.to_str().unwrap()]));
    let before = fixture.status();
    let pid = before["owner_pid"].as_u64().unwrap();
    unsafe {
        libc::kill(pid as i32, libc::SIGKILL);
    }
    fixture.wait_for_owner_exit(pid);
    assert_eq!(fixture.container()["State"]["Running"], true);
    let unavailable = fixture.ags(&["t3", "status", "--repository", "."]);
    assert!(!unavailable.status.success());
    let status: Value = serde_json::from_slice(&unavailable.stdout).unwrap();
    assert_eq!(status["state"], "unavailable");
    let invalid = fixture
        .root
        .path()
        .join("data/ags/t3/registry")
        .join(format!("{}.json", "f".repeat(64)));
    fs::write(&invalid, "invalid json").unwrap();
    fs::set_permissions(&invalid, fs::Permissions::from_mode(0o600)).unwrap();
    fixture.generation("0.0.46");
    let output = fixture
        .command(env!("CARGO_BIN_EXE_ags"))
        .current_dir(fixture.root.path())
        .args(["t3", "upgrade", "--all"])
        .output()
        .unwrap();
    success(&output);
    assert!(String::from_utf8_lossy(&output.stdout).contains("1 succeeded, 0 failed"));
    assert!(String::from_utf8_lossy(&output.stderr).contains("Skipping ags-t3-ffffffffffffffff"));
    assert_eq!(fixture.status()["running"], true);
    assert_eq!(fixture.status()["runtime"], "0.0.46");
    assert_eq!(fixture.status()["environment"], before["environment"]);
}

#[test]
fn cancelling_global_upgrade_with_only_stopped_instances_does_not_start_them() {
    let environments = Environments::new();
    success(&environments.global(&["t3", "stop", "--all"]));
    let before = environments.rows();
    assert!(before.iter().all(|entry| entry["state"] == "stopped"));
    for keys in ["0d", "201b", "2003"] {
        let result = environments.pty("upgrade", keys);
        assert!(result["output"].as_str().unwrap().contains("[stopped]"));
        assert_eq!(environments.rows(), before);
    }
}

#[test]
fn stopped_selection_that_starts_while_the_picker_is_open_is_not_interrupted() {
    let environments = Environments::new();
    let rows = environments.rows();
    let first = rows[0]["environment"].as_str().unwrap();
    let second = rows[1]["environment"].as_str().unwrap();
    success(&environments.global(&["t3", "stop", first]));
    environments.fixture.generation("0.0.46");
    let result = environments.pty_with_hooks("upgrade", "200d", None, Some(first));
    assert!(
        result["output"]
            .as_str()
            .unwrap()
            .contains("environment state changed since selection")
    );
    assert_eq!(environments.status(first)["state"], "running");
    assert_eq!(environments.status(first)["runtime"], "0.0.46");
    assert_eq!(
        environments.status(second)["owner_pid"],
        rows[1]["owner_pid"]
    );
    assert_eq!(environments.status(second)["runtime"], "0.0.45");
    assert_eq!(
        environments
            .fixture
            .events()
            .iter()
            .filter(|event| event[0] == "create")
            .count(),
        3
    );
}
