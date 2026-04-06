use super::{
    OpenDecision, is_auto_allowed, is_proxyable_localhost_url, parse_zenity_decision, prompt_text,
    rewrite_localhost_url_via_relay,
};
use crate::webview_relay;
use std::os::unix::process::ExitStatusExt;

#[test]
fn auto_allow_matches_exact_hosts_and_subdomains() {
    let domains = vec!["example.com".to_owned(), "trusted.test".to_owned()];

    assert!(is_auto_allowed("https://example.com/login", &domains));
    assert!(is_auto_allowed("https://api.example.com/login", &domains));
    assert!(is_auto_allowed("HTTP://TRUSTED.TEST:8443/path", &domains));
}

#[test]
fn auto_allow_rejects_suffix_tricks_and_invalid_urls() {
    let domains = vec!["example.com".to_owned(), "trusted.test".to_owned()];

    assert!(!is_auto_allowed("https://evil-example.com/login", &domains));
    assert!(!is_auto_allowed(
        "https://example.com.evil.test/login",
        &domains
    ));
    assert!(!is_auto_allowed("file:///tmp/example.com", &domains));
    assert!(!is_auto_allowed("not a url", &domains));
    assert!(!is_auto_allowed(
        "https://example.com/login",
        &[" https://example.com ".to_owned()]
    ));
}

#[test]
fn proxyable_localhost_detection_requires_http_and_explicit_port() {
    assert!(is_proxyable_localhost_url("http://localhost:4173/app"));
    assert!(is_proxyable_localhost_url("http://127.0.0.1:4173/"));
    assert!(!is_proxyable_localhost_url("https://localhost:4173/app"));
    assert!(!is_proxyable_localhost_url("http://localhost/app"));
    assert!(!is_proxyable_localhost_url("http://example.com:4173/app"));
}

#[test]
fn prompt_text_surfaces_requested_host_and_callback_context() {
    let text = prompt_text(
        "https://provider.example/auth?redirect_uri=http://localhost:4317/callback",
        true,
        false,
    );

    assert!(text.contains("Requested host: provider.example"));
    assert!(text.contains("relay a localhost callback back into the sandbox"));
    assert!(text.contains("AGS will capture and relay to the sandbox"));
}

#[test]
fn prompt_text_explains_proxy_choice_for_localhost_apps() {
    let text = prompt_text("http://localhost:4173/app?token=secret", false, true);

    assert!(text.contains("Requested host: localhost"));
    assert!(text.contains("http://localhost:4173/app?..."));
    assert!(text.contains("Proxy routes it through AGS"));
    assert!(text.contains("Choose Open to open the original URL, Proxy to route sandbox localhost through AGS, or Cancel to deny."));
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
