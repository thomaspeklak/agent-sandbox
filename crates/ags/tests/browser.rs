use std::net::TcpListener;
use std::path::PathBuf;

use ags::browser;
use ags::config::BrowserConfig;

fn unused_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port()
}

fn make_config(enabled: bool, port: u16) -> BrowserConfig {
    BrowserConfig {
        enabled,
        window_class: "ags-browser".to_owned(),
        command: String::new(),
        profile_dir: PathBuf::from("/tmp/ags-browser-test-profile"),
        debug_port: port,
        pi_skill_path: String::new(),
        command_args: Vec::new(),
    }
}

#[test]
fn start_returns_none_when_browser_mode_off() {
    let config = make_config(true, 9222);
    assert!(browser::start_if_needed(false, &config).unwrap().is_none());
}

#[test]
fn start_fails_when_not_enabled() {
    let config = make_config(false, 9222);
    let result = browser::start_if_needed(true, &config);
    assert!(result.is_err());
    let err = result.unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("enabled is false"), "unexpected error: {msg}");
}

#[test]
fn start_fails_when_command_empty() {
    let config = make_config(true, 9222);
    let result = browser::start_if_needed(true, &config);
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(msg.contains("command is empty"), "unexpected error: {msg}");
}

#[test]
fn start_fails_when_command_not_found() {
    let mut config = make_config(true, unused_port());
    config.command = "ags-nonexistent-browser-command-xyz".to_owned();
    let result = browser::start_if_needed(true, &config);
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(msg.contains("not found in PATH"), "unexpected error: {msg}");
}

#[test]
fn start_fails_when_absolute_command_not_executable() {
    let mut config = make_config(true, unused_port());
    config.command = "/nonexistent/path/to/browser".to_owned();
    let result = browser::start_if_needed(true, &config);
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(msg.contains("not executable"), "unexpected error: {msg}");
}

fn fake_browser() -> (tempfile::TempDir, BrowserConfig) {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("browser");
    std::fs::write(
        &script,
        r#"#!/usr/bin/python3
import sys, socket, pathlib, time
args = dict(arg.split('=', 1) for arg in sys.argv[1:] if '=' in arg)
assert args['--class'] == 'ags-browser'
assert args['--remote-debugging-port'] == '0'
profile = pathlib.Path(args['--user-data-dir'])
sock = socket.socket()
sock.bind(('127.0.0.1', 0))
sock.listen()
(profile / 'DevToolsActivePort').write_text(str(sock.getsockname()[1]) + '\n')
while True:
    conn, _ = sock.accept()
    conn.close()
"#,
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut config = make_config(true, 9222);
    config.command = script.display().to_string();
    config.profile_dir = dir.path().join("profiles");
    (dir, config)
}

#[test]
fn sessions_are_isolated_and_cleanup_only_their_own_browser() {
    let (_dir, mut config) = fake_browser();
    let occupied = TcpListener::bind("127.0.0.1:0").unwrap();
    config.debug_port = occupied.local_addr().unwrap().port();
    let first = browser::start_if_needed(true, &config).unwrap().unwrap();
    let second = browser::start_if_needed(true, &config).unwrap().unwrap();
    assert_ne!(first.port, second.port);
    assert_ne!(first.port, config.debug_port);
    let sessions = config.profile_dir.join("sessions");
    assert_eq!(std::fs::read_dir(&sessions).unwrap().count(), 2);
    let first_port = first.port;
    drop(first);
    assert!(std::net::TcpStream::connect(("127.0.0.1", first_port)).is_err());
    assert!(std::net::TcpStream::connect(("127.0.0.1", second.port)).is_ok());
    assert_eq!(std::fs::read_dir(&sessions).unwrap().count(), 1);
    let socat = second.socat_command();
    assert!(socat.contains(&format!("TCP-LISTEN:{}", second.port)));
    assert!(socat.contains(&format!("TCP:10.0.2.2:{}", second.port)));
    drop(second);
    assert_eq!(std::fs::read_dir(&sessions).unwrap().count(), 0);
    assert!(config.profile_dir.exists());
}

#[test]
fn readiness_failure_cleans_up_profile() {
    let (_dir, mut config) = fake_browser();
    config.command = "/bin/true".to_owned();
    assert!(browser::start_if_needed(true, &config).is_err());
    assert_eq!(
        std::fs::read_dir(config.profile_dir.join("sessions"))
            .unwrap()
            .count(),
        0
    );
}

#[test]
fn error_display_formats() {
    use ags::browser::BrowserError;
    use std::time::Duration;

    let cases: Vec<(BrowserError, &str)> = vec![
        (BrowserError::NotEnabled, "enabled is false"),
        (BrowserError::EmptyCommand, "command is empty"),
        (
            BrowserError::CommandNotFound("chrome".to_owned()),
            "not found in PATH: chrome",
        ),
        (
            BrowserError::CommandNotExecutable("/usr/bin/x".to_owned()),
            "not executable: /usr/bin/x",
        ),
        (
            BrowserError::ReadyTimeout {
                port: 9222,
                timeout: Duration::from_secs(5),
            },
            "port 9222 within 5.0s",
        ),
    ];

    for (err, expected_substr) in cases {
        let msg = err.to_string();
        assert!(
            msg.contains(expected_substr),
            "Expected '{expected_substr}' in '{msg}'"
        );
    }
}
