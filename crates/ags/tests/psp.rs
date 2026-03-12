//! Integration tests for the AGS ↔ PSP (podman-socket-proxy) contract.
//!
//! # Test categories
//!
//! - **Unit-level**: Run without external dependencies (no `psp` binary, no podman).
//!   These validate error handling and guard behavior.
//!
//! - **Integration** (`#[ignore]`): Require `psp` on PATH and a running podman socket.
//!   Run with: `cargo test --test psp -- --ignored`
//!
//! # Preconditions for integration tests
//!
//! 1. `psp` binary installed and on PATH
//! 2. Podman socket available at `$XDG_RUNTIME_DIR/podman/podman.sock`
//! 3. A valid PSP policy file (default or project-local `.psp.json`)

use std::io::{Read, Write};
use std::os::unix::net::UnixStream;
use std::time::Duration;

// --- Unit-level tests (no psp binary required) ---

#[test]
fn start_fails_with_missing_binary() {
    // Point to a nonexistent binary via config override
    let result = ags::psp::start("/nonexistent/psp-binary", false);
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("failed to start"),
        "should fail to spawn: {err}"
    );
}

#[test]
fn start_fails_when_not_on_path() {
    // Use empty config_binary so it falls back to PATH lookup.
    // Set PATH to empty so `psp` is not found.
    let original_path = std::env::var("PATH").unwrap_or_default();
    unsafe { std::env::set_var("PATH", "") };
    let result = ags::psp::start("", false);
    unsafe { std::env::set_var("PATH", &original_path) };

    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("not found"),
        "should report binary not found: {err}"
    );
}

#[test]
fn start_fails_when_binary_exits_immediately() {
    // Use `false` as the "psp" binary — it exits immediately with code 1
    let result = ags::psp::start("/usr/bin/false", false);
    assert!(result.is_err());
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("exited immediately") || err.contains("failed to start"),
        "should detect early child exit: {err}"
    );
}

#[test]
fn container_paths_are_stable() {
    assert_eq!(
        ags::psp::PspGuard::container_socket_path(),
        "/run/psp/psp.sock"
    );
    assert_eq!(ags::psp::PspGuard::container_socket_dir(), "/run/psp");
}

// --- Integration tests (require psp + podman) ---

/// Start PSP and verify the socket accepts connections.
#[test]
#[ignore]
fn psp_starts_and_socket_is_ready() {
    let guard = ags::psp::start("", false).expect("psp should start");
    assert!(
        guard.socket_path.exists(),
        "socket file should exist: {}",
        guard.socket_path.display()
    );

    // Verify we can connect
    let stream = UnixStream::connect(&guard.socket_path);
    assert!(stream.is_ok(), "should connect to PSP socket");
}

/// Start PSP and send a Docker API /_ping through the socket.
#[test]
#[ignore]
fn psp_responds_to_ping() {
    let guard = ags::psp::start("", false).expect("psp should start");

    let mut stream = UnixStream::connect(&guard.socket_path).expect("connect to socket");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .ok();

    // Send HTTP GET /_ping
    let request = "GET /_ping HTTP/1.1\r\nHost: localhost\r\n\r\n";
    stream.write_all(request.as_bytes()).expect("send request");

    let mut response = vec![0u8; 4096];
    let n = stream.read(&mut response).expect("read response");
    let response_str = String::from_utf8_lossy(&response[..n]);

    assert!(
        response_str.contains("200 OK"),
        "/_ping should return 200: {response_str}"
    );
}

/// Verify PSP returns the x-psp-request-id header for correlation.
#[test]
#[ignore]
fn psp_returns_request_id_header() {
    let guard = ags::psp::start("", false).expect("psp should start");

    let mut stream = UnixStream::connect(&guard.socket_path).expect("connect to socket");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .ok();

    let request = "GET /_ping HTTP/1.1\r\nHost: localhost\r\n\r\n";
    stream.write_all(request.as_bytes()).expect("send request");

    let mut response = vec![0u8; 4096];
    let n = stream.read(&mut response).expect("read response");
    let response_str = String::from_utf8_lossy(&response[..n]).to_lowercase();

    assert!(
        response_str.contains("x-psp-request-id"),
        "response should include x-psp-request-id: {response_str}"
    );
}

/// Verify PSP returns the effective session ID header.
#[test]
#[ignore]
fn psp_returns_effective_session_id() {
    let guard = ags::psp::start("", false).expect("psp should start");

    let mut stream = UnixStream::connect(&guard.socket_path).expect("connect to socket");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .ok();

    let request =
        "GET /_ping HTTP/1.1\r\nHost: localhost\r\nx-psp-session-id: test-session\r\n\r\n";
    stream.write_all(request.as_bytes()).expect("send request");

    let mut response = vec![0u8; 4096];
    let n = stream.read(&mut response).expect("read response");
    let response_str = String::from_utf8_lossy(&response[..n]).to_lowercase();

    assert!(
        response_str.contains("x-psp-effective-session-id"),
        "response should include x-psp-effective-session-id: {response_str}"
    );
}

/// Verify that unsupported endpoints return 501.
#[test]
#[ignore]
fn psp_rejects_unsupported_endpoint() {
    let guard = ags::psp::start("", false).expect("psp should start");

    let mut stream = UnixStream::connect(&guard.socket_path).expect("connect to socket");
    stream
        .set_read_timeout(Some(Duration::from_secs(5)))
        .ok();

    // /networks is not in the testcontainers profile
    let request = "GET /networks HTTP/1.1\r\nHost: localhost\r\n\r\n";
    stream.write_all(request.as_bytes()).expect("send request");

    let mut response = vec![0u8; 4096];
    let n = stream.read(&mut response).expect("read response");
    let response_str = String::from_utf8_lossy(&response[..n]);

    assert!(
        response_str.contains("501") || response_str.contains("Not Implemented"),
        "unsupported endpoint should return 501: {response_str}"
    );
}

/// Verify guard cleanup removes the socket directory on drop.
#[test]
#[ignore]
fn psp_guard_cleans_up_on_drop() {
    let socket_dir;
    {
        let guard = ags::psp::start("", false).expect("psp should start");
        socket_dir = guard.socket_path.parent().unwrap().to_owned();
        assert!(socket_dir.exists(), "socket dir should exist while guard is alive");
    }
    // Guard dropped — socket dir should be gone
    // Give a moment for cleanup
    std::thread::sleep(Duration::from_millis(200));
    assert!(
        !socket_dir.exists(),
        "socket dir should be cleaned up after drop: {}",
        socket_dir.display()
    );
}

/// Verify that keep_on_failure flag is accepted without error.
#[test]
#[ignore]
fn psp_starts_with_keep_on_failure() {
    let guard = ags::psp::start("", true).expect("psp should start with keep_on_failure");
    assert!(guard.socket_path.exists());
}
