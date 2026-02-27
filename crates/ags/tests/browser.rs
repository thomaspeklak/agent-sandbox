use std::net::TcpListener;
use std::path::PathBuf;

use ags::browser;
use ags::config::BrowserConfig;

fn make_config(enabled: bool, port: u16) -> BrowserConfig {
    BrowserConfig {
        enabled,
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
    let result = browser::start_if_needed(false, &config);
    assert!(result.is_ok());
    assert!(result.unwrap().is_none());
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
    let mut config = make_config(true, 9222);
    config.command = "ags-nonexistent-browser-command-xyz".to_owned();
    let result = browser::start_if_needed(true, &config);
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(msg.contains("not found in PATH"), "unexpected error: {msg}");
}

#[test]
fn start_fails_when_absolute_command_not_executable() {
    let mut config = make_config(true, 9222);
    config.command = "/nonexistent/path/to/browser".to_owned();
    let result = browser::start_if_needed(true, &config);
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(msg.contains("not executable"), "unexpected error: {msg}");
}

#[test]
fn start_detects_already_running_browser() {
    // Bind a TCP listener to simulate an already-running browser debug port
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();

    let mut config = make_config(true, port);
    config.command = "unused-because-already-running".to_owned();

    let result = browser::start_if_needed(true, &config);
    assert!(result.is_ok());
    let sidecar = result.unwrap();
    assert!(
        sidecar.is_some(),
        "should return sidecar for running browser"
    );

    let sidecar = sidecar.unwrap();
    assert_eq!(sidecar.port, port);

    // Keep listener alive for the duration of the test
    drop(listener);
}

#[test]
fn socat_command_format() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();

    let mut config = make_config(true, port);
    config.command = "unused".to_owned();

    let result = browser::start_if_needed(true, &config).unwrap().unwrap();
    let socat = result.socat_command();

    assert!(
        socat.contains(&format!("TCP-LISTEN:{port}")),
        "socat should listen on port: {socat}"
    );
    assert!(
        socat.contains(&format!("TCP:10.0.2.2:{port}")),
        "socat should forward to host via slirp4netns: {socat}"
    );
    assert!(socat.contains("fork"), "socat should fork: {socat}");

    drop(listener);
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
