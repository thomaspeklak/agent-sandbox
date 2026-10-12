use super::Fixture;
use sha2::{Digest, Sha256};
use std::fs;
use std::process::Output;

fn success(output: &Output) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn start_with_registered_values(fixture: &Fixture) {
    let config = fixture.root.path().join("config.toml");
    success(&fixture.ags(&[
        "--agent",
        "t3",
        "--config",
        config.to_str().unwrap(),
        "--env",
        "T3_REVIEW_CAPTURE=launch-only-value",
        "--env",
        "T3_REVIEW_OTHER=other-launch-value",
    ]));
}

#[test]
fn missing_upgrade_environment_values_leave_the_existing_owner_and_jobs_running() {
    let fixture = Fixture::new();
    start_with_registered_values(&fixture);
    let before = fixture.status();
    let events = fixture.events().len();
    let output = fixture
        .command(env!("CARGO_BIN_EXE_ags"))
        .env_remove("T3_REVIEW_CAPTURE")
        .env_remove("T3_REVIEW_OTHER")
        .args(["t3", "upgrade", "--repository", "."])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let error = String::from_utf8_lossy(&output.stderr);
    assert!(error.contains("T3_REVIEW_CAPTURE") && error.contains("T3_REVIEW_OTHER"));
    assert!(!error.contains("launch-only-value") && !error.contains("other-launch-value"));
    assert_eq!(fixture.status()["owner_pid"], before["owner_pid"]);
    assert_eq!(fixture.status()["running"], true);
    assert!(
        fixture.events()[events..]
            .iter()
            .all(|event| event[0] == "container")
    );
}

#[test]
fn upgrade_reacquires_exported_registered_values_and_keeps_them_out_of_durable_metadata() {
    let fixture = Fixture::new();
    start_with_registered_values(&fixture);
    let before = fixture.status();
    let output = fixture
        .command(env!("CARGO_BIN_EXE_ags"))
        .env("T3_REVIEW_CAPTURE", "replacement-capture-value")
        .env("T3_REVIEW_OTHER", "replacement-other-value")
        .args(["t3", "upgrade", "--repository", "."])
        .output()
        .unwrap();
    success(&output);
    assert_ne!(fixture.status()["owner_pid"], before["owner_pid"]);
    assert_eq!(
        fixture.container()["boot_environment_hashes"]["T3_REVIEW_CAPTURE"],
        format!("{:x}", Sha256::digest(b"replacement-capture-value"))
    );
    assert_eq!(
        fixture.container()["boot_environment_hashes"]["T3_REVIEW_OTHER"],
        format!("{:x}", Sha256::digest(b"replacement-other-value"))
    );
    for path in [
        fixture.private_dir().join("owner.log"),
        fixture.private_dir().join("owner-process.json"),
        fixture.root.path().join("events.jsonl"),
    ] {
        let contents = fs::read_to_string(path).unwrap();
        assert!(
            !contents.contains("replacement-capture-value")
                && !contents.contains("replacement-other-value")
        );
    }
    let registry = fixture.root.path().join("data/ags/t3/registry");
    for entry in fs::read_dir(registry).unwrap().flatten() {
        let contents = fs::read_to_string(entry.path()).unwrap();
        assert!(
            !contents.contains("launch-only-value")
                && !contents.contains("replacement-capture-value")
        );
    }
}

#[test]
fn upgrade_hands_off_the_reacquired_secret_snapshot_to_the_replacement_owner() {
    let fixture = Fixture::new();
    let config = fixture.root.path().join("config.toml");
    let counter = fixture.root.path().join("lookup-count");
    let lookup = fixture.root.path().join("changing-lookup.py");
    fs::write(&lookup, "import pathlib, sys\np = pathlib.Path(sys.argv[1])\nn = int(p.read_text()) + 1 if p.exists() else 1\np.write_text(str(n))\nprint('source-value-' + str(n))\n").unwrap();
    let source = format!(
        "\n[[secret]]\nenv = \"T3_REVIEW_CAPTURE\"\ncommand = [{:?}, {:?}, {:?}]\n",
        ags::util::which("python3").unwrap(),
        lookup,
        counter
    );
    fs::write(
        &config,
        format!("{}{source}", fs::read_to_string(&config).unwrap()),
    )
    .unwrap();
    success(&fixture.ags(&[
        "--agent",
        "t3",
        "--config",
        config.to_str().unwrap(),
        "--env",
        "T3_REVIEW_CAPTURE=launch-only-value",
    ]));
    assert_eq!(fs::read_to_string(&counter).unwrap(), "1");
    let output = fixture
        .command(env!("CARGO_BIN_EXE_ags"))
        .env_remove("T3_REVIEW_CAPTURE")
        .args(["t3", "upgrade", "--repository", "."])
        .output()
        .unwrap();
    success(&output);
    assert_eq!(fs::read_to_string(counter).unwrap(), "3");
    assert_eq!(
        fixture.container()["boot_environment_hashes"]["T3_REVIEW_CAPTURE"],
        format!("{:x}", Sha256::digest(b"source-value-2"))
    );
}

#[test]
fn owner_reported_stop_errors_are_not_hidden_by_stop_or_upgrade_fallback() {
    use std::io::{Read, Write};
    use std::os::unix::net::UnixListener;
    use std::path::Path;
    for action in ["stop", "upgrade"] {
        let fixture = Fixture::new();
        let config = fixture.root.path().join("config.toml");
        success(&fixture.ags(&["--agent", "t3", "--config", config.to_str().unwrap()]));
        let before = fixture.status();
        let id = before["repository_id"].as_str().unwrap();
        let registration: serde_json::Value = serde_json::from_slice(
            &fs::read(
                fixture
                    .root
                    .path()
                    .join("data/ags/t3/registry")
                    .join(format!("{id}.json")),
            )
            .unwrap(),
        )
        .unwrap();
        let socket = Path::new(registration["control_dir"].as_str().unwrap()).join("control.sock");
        let saved = socket.with_extension("saved");
        fs::rename(&socket, &saved).unwrap();
        let listener = UnixListener::bind(&socket).unwrap();
        let response = format!("{before}\n");
        let server = std::thread::spawn(move || {
            for (expected, response) in [
                ("status\n", response.as_bytes()),
                ("stop\n", b"ERR fixture owner refused to stop\n".as_slice()),
            ] {
                let (mut stream, _) = listener.accept().unwrap();
                let mut request = String::new();
                stream.read_to_string(&mut request).unwrap();
                assert_eq!(request, expected);
                stream.write_all(response).unwrap();
            }
        });
        let events = fixture.events().len();
        let output = fixture.ags(&["t3", action, "--repository", "."]);
        server.join().unwrap();
        fs::remove_file(&socket).unwrap();
        fs::rename(saved, socket).unwrap();
        assert!(
            !output.status.success(),
            "{action} must propagate an owner stop error"
        );
        assert!(String::from_utf8_lossy(&output.stderr).contains("fixture owner refused to stop"));
        assert_eq!(fixture.status()["owner_pid"], before["owner_pid"]);
        assert_eq!(fixture.status()["running"], true);
        assert!(
            fixture.events()[events..]
                .iter()
                .all(|event| event[0] == "container")
        );
    }
}

#[test]
fn stop_with_an_absent_owner_endpoint_still_stops_the_owned_container() {
    let fixture = Fixture::new();
    let config = fixture.root.path().join("config.toml");
    success(&fixture.ags(&["--agent", "t3", "--config", config.to_str().unwrap()]));
    let status = fixture.status();
    unsafe {
        libc::kill(status["owner_pid"].as_u64().unwrap() as i32, libc::SIGKILL);
    }
    fixture.wait_for_owner_exit(status["owner_pid"].as_u64().unwrap());
    success(&fixture.ags(&["t3", "stop", "--repository", "."]));
    assert_eq!(fixture.container()["State"]["Running"], false);
}

#[test]
fn browser_reconnect_and_owner_recovery_refresh_the_proxy_without_adopting_new_runtime_or_image() {
    use std::path::Path;
    let fixture = Fixture::new();
    let config = fixture.root.path().join("config.toml");
    let browser = format!(
        "\n[browser]\nenabled = true\ncommand = {:?}\ncommand_args = [{:?}]\nprofile_dir = {:?}\nstartup_timeout_ms = 5000\n",
        ags::util::which("python3").unwrap(),
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/t3_browser.py"),
        fixture.root.path().join("browser-profiles")
    );
    fs::write(
        &config,
        format!("{}{browser}", fs::read_to_string(&config).unwrap()),
    )
    .unwrap();
    success(&fixture.ags(&[
        "--agent",
        "t3",
        "--browser",
        "--config",
        config.to_str().unwrap(),
    ]));
    let original = fixture.status();
    let alias = original["ssh_alias"].as_str().unwrap();
    let first: Vec<u16> =
        serde_json::from_slice(&fs::read(fixture.root.path().join("browser-ports.json")).unwrap())
            .unwrap();
    let ports = || -> Vec<u16> {
        serde_json::from_slice(&fs::read(fixture.root.path().join("browser-ports.json")).unwrap())
            .unwrap()
    };
    let assert_proxy = |port: u16| {
        assert!(
            fixture.container()["Config"]["Cmd"][2]
                .as_str()
                .unwrap()
                .contains(&format!("TCP:10.0.2.2:{port} "))
        );
        assert!(std::net::TcpStream::connect(("127.0.0.1", port)).is_ok());
        assert_eq!(
            fixture.container()["Image"],
            format!("sha256:{}", "a".repeat(64))
        );
        assert_eq!(fixture.status()["runtime"], "0.0.45");
        assert_eq!(fixture.status()["generation"], original["generation"]);
    };
    assert_proxy(first[0]);
    let home = fixture.container()["Mounts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|mount| mount["Destination"].as_str().unwrap().ends_with("/home"))
        .unwrap()["Source"]
        .as_str()
        .unwrap()
        .to_owned();
    let history = Path::new(&home).join(".t3/userdata/browser-history-test");
    fs::write(&history, "persistent browser session history").unwrap();
    success(&fixture.ags(&["t3", "stop", alias]));
    fixture.generation("0.0.46");
    fs::write(
        fixture.root.path().join("image-id"),
        format!("sha256:{}", "b".repeat(64)),
    )
    .unwrap();
    success(&fixture.ssh(alias, super::launch("0.0.45"), true));
    let second = ports();
    assert_eq!(second.len(), 2);
    assert_ne!(second[0], second[1]);
    assert_proxy(second[1]);
    assert_eq!(
        fs::read_to_string(&history).unwrap(),
        "persistent browser session history"
    );
    let owner = fixture.status()["owner_pid"].as_u64().unwrap() as i32;
    unsafe {
        libc::kill(owner, libc::SIGKILL);
    }
    fixture.wait_for_owner_exit(owner as u64);
    success(&fixture.ssh(alias, super::launch("0.0.45"), true));
    let third = ports();
    assert_eq!(third.len(), 3);
    assert_proxy(third[2]);
    assert_eq!(
        fs::read_to_string(history).unwrap(),
        "persistent browser session history"
    );
    assert_eq!(
        fixture
            .events()
            .iter()
            .filter(|event| event[0] == "create")
            .count(),
        3
    );
    assert_eq!(fixture.status()["ssh_alias"], original["ssh_alias"]);
}
