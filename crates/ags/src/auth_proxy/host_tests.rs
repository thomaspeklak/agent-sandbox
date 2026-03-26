use super::{
    OpenDecision, is_proxyable_localhost_url, parse_zenity_decision,
    rewrite_localhost_url_via_relay,
};
use crate::webview_relay;
use std::os::unix::process::ExitStatusExt;

#[test]
fn proxyable_localhost_detection_requires_http_and_explicit_port() {
    assert!(is_proxyable_localhost_url("http://localhost:4173/app"));
    assert!(is_proxyable_localhost_url("http://127.0.0.1:4173/"));
    assert!(!is_proxyable_localhost_url("https://localhost:4173/app"));
    assert!(!is_proxyable_localhost_url("http://localhost/app"));
    assert!(!is_proxyable_localhost_url("http://example.com:4173/app"));
}

#[test]
fn parse_zenity_proxy_button_even_when_exit_status_is_nonzero() {
    let output = std::process::Output {
        status: std::process::ExitStatus::from_raw(256),
        stdout: b"Proxy\n".to_vec(),
        stderr: Vec::new(),
    };
    assert_eq!(parse_zenity_decision(&output, true), OpenDecision::Proxy);
}

#[test]
fn relay_rewrite_preserves_path_query_and_hash() {
    let dir = tempfile::tempdir().unwrap();
    let runtime_dir = dir.path().join("relay-runtime");
    let guard = webview_relay::start(&runtime_dir).unwrap();
    let socket_path = guard.runtime_dir.join(webview_relay::SOCKET_NAME);

    let rewritten = rewrite_localhost_url_via_relay(
        "http://localhost:4173/app/index.html?x=1&y=2#frag",
        &socket_path,
    )
    .unwrap();

    assert!(rewritten.starts_with("http://127.0.0.1:"));
    assert!(rewritten.ends_with("/app/index.html?x=1&y=2#frag"));
}
