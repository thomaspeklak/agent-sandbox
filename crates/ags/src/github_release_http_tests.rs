use super::*;
use std::os::unix::fs::PermissionsExt;

struct CurlFixture {
    root: tempfile::TempDir,
    curl: std::path::PathBuf,
}

impl CurlFixture {
    fn new(status: u16, headers: &str, body: &[u8]) -> Self {
        let root = tempfile::tempdir().unwrap();
        let curl = root.path().join("curl");
        std::fs::write(root.path().join("body"), body).unwrap();
        std::fs::write(root.path().join("headers"), headers).unwrap();
        let script = format!(
            r#"#!/usr/bin/env python3
import json, os, sys
from pathlib import Path
root = Path(__file__).parent
args = sys.argv[1:]
config = sys.stdin.read() if '--config' in args else ''
(root / 'request.json').write_text(json.dumps({{
    'args': args, 'config': config,
    'token_env': 'GH_TOKEN' in os.environ or 'GITHUB_TOKEN' in os.environ
}}))
Path(args[args.index('--output') + 1]).write_bytes((root / 'body').read_bytes())
Path(args[args.index('--dump-header') + 1]).write_bytes((root / 'headers').read_bytes())
sys.stdout.write('{status}')
sys.exit({exit})
"#,
            exit = if (200..300).contains(&status) { 0 } else { 22 }
        );
        std::fs::write(&curl, script).unwrap();
        std::fs::set_permissions(&curl, std::fs::Permissions::from_mode(0o700)).unwrap();
        Self { root, curl }
    }

    fn request(&self) -> serde_json::Value {
        serde_json::from_slice(&std::fs::read(self.root.path().join("request.json")).unwrap())
            .unwrap()
    }
}

#[test]
fn authenticated_metadata_uses_private_stdin_not_argv_or_child_environment() {
    let fixture = CurlFixture::new(200, "HTTP/2 200\r\n\r\n", b"[]");
    let client = Client {
        token: Some("fixture-secret".into()),
    };
    let body = client
        .fetch_with_curl(
            "example/repo",
            "https://api.github.com/repos/example/repo/releases",
            false,
            &fixture.curl,
        )
        .unwrap();
    assert_eq!(body, b"[]");
    let request = fixture.request();
    assert!(
        request["config"]
            .as_str()
            .unwrap()
            .contains("Authorization: Bearer fixture-secret")
    );
    assert!(!request["args"].to_string().contains("fixture-secret"));
    assert_eq!(request["token_env"], false);
    assert!(!request["args"].to_string().contains("location-trusted"));
}

#[test]
fn bearer_config_quotes_and_backslashes_cannot_inject_options() {
    let fixture = CurlFixture::new(200, "HTTP/2 200\r\n\r\n", b"[]");
    let client = Client {
        token: Some("fixture\\\"secret".into()),
    };
    client
        .fetch_with_curl(
            "example/repo",
            "https://api.github.com/repos/example/repo/releases",
            false,
            &fixture.curl,
        )
        .unwrap();
    assert_eq!(
        fixture.request()["config"],
        "header = \"Authorization: Bearer fixture\\\\\\\"secret\"\n"
    );
}

#[test]
fn checksum_and_other_origins_never_receive_github_credentials() {
    let client = Client {
        token: Some("fixture-secret".into()),
    };
    for (url, checksum) in [
        (
            "https://github.com/example/repo/releases/download/v1/checksums",
            true,
        ),
        ("https://api.github.com/repos/example/repo/asset", true),
        ("https://api.github.com.evil.invalid/releases", false),
        ("https://example.invalid/releases", false),
    ] {
        let fixture = CurlFixture::new(200, "HTTP/2 200\r\n\r\n", b"binary\0\xff");
        let result = client
            .fetch_with_curl("example/repo", url, checksum, &fixture.curl)
            .unwrap();
        assert_eq!(result, b"binary\0\xff");
        let request = fixture.request();
        assert_eq!(request["config"], "");
        assert!(!request["args"].to_string().contains("--config"));
        if checksum {
            assert!(request["args"].to_string().contains("1048576"));
        }
    }
}

#[test]
fn anonymous_rate_limit_reports_github_reason_reset_and_authentication_action() {
    let fixture = CurlFixture::new(
        403,
        "HTTP/2 403\r\nx-ratelimit-remaining: 0\r\nx-ratelimit-reset: 1791547200\r\n\r\n",
        br#"{"message":"API rate limit exceeded"}"#,
    );
    let client = Client { token: None };
    let error = client
        .fetch_with_curl(
            "example/repo",
            "https://api.github.com/repos/example/repo/releases",
            false,
            &fixture.curl,
        )
        .unwrap_err()
        .to_string();
    assert!(error.contains("HTTP 403: API rate limit exceeded"));
    assert!(error.contains("rate limit resets at"));
    assert!(error.contains("request was anonymous"));
    assert!(error.contains("gh auth login --hostname github.com"));
    assert!(error.contains("GH_TOKEN"));
}

#[test]
fn authenticated_limits_invalid_credentials_and_unrelated_403s_are_distinguished() {
    let error = http_error(
        403,
        "HTTP/2 403\r\nX-RateLimit-Remaining: 0\r\nRetry-After: 60\r\n\r\n",
        r#"{"message":"API rate limit exceeded"}"#,
        "",
        Some("fixture-secret"),
    );
    assert!(error.contains("authenticated GitHub API quota"));
    assert!(error.contains("retry after 60 seconds"));
    assert!(!error.contains("request was anonymous"));
    let error = http_error(
        401,
        "",
        r#"{"message":"Bad credentials fixture-secret"}"#,
        "",
        Some("fixture-secret"),
    );
    assert!(error.contains("GitHub rejected the credentials"));
    assert!(!error.contains("fixture-secret"));
    let error = http_error(
        403,
        "HTTP/1.1 301\r\nX-RateLimit-Remaining: 0\r\n\r\nHTTP/2 403\r\n\r\n",
        r#"{"message":"Resource not accessible by integration"}"#,
        "",
        None,
    );
    assert!(error.contains("Resource not accessible by integration"));
    assert!(!error.contains("rate limit"));
}

#[test]
fn secondary_limits_and_transport_errors_keep_useful_diagnostics() {
    let error = http_error(
        403,
        "Retry-After: 10\r\n",
        r#"{"message":"You have exceeded a secondary rate limit"}"#,
        "",
        None,
    );
    assert!(error.contains("secondary rate limit"));
    assert!(error.contains("retry after 10 seconds"));
    let error = http_error(0, "", "", "curl: failed to connect", None);
    assert_eq!(error, "curl request failed: curl: failed to connect");
}
