use super::*;
use std::cell::Cell;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;

#[test]
fn github_token_precedence_uses_existing_login_only_when_environment_is_empty() {
    for (gh, github, expected, lookups) in [
        (Some("primary"), Some("secondary"), "primary", 0),
        (None, Some("secondary"), "secondary", 0),
        (Some(" "), Some("secondary"), "secondary", 0),
        (None, None, "stored", 1),
    ] {
        let calls = Cell::new(0);
        let result = resolve_token(
            |name| match name {
                "GH_TOKEN" => gh.map(str::to_owned),
                "GITHUB_TOKEN" => github.map(str::to_owned),
                _ => unreachable!(),
            },
            || {
                calls.set(calls.get() + 1);
                Ok(Some("stored\n".into()))
            },
        )
        .unwrap();
        assert_eq!(result.as_deref(), Some(expected));
        assert_eq!(calls.get(), lookups);
    }
    assert_eq!(resolve_token(|_| None, || Ok(None)).unwrap(), None);
}

#[test]
fn invalid_environment_token_fails_without_leaking_it_or_downgrading_to_anonymous() {
    let value = "fixture-secret\nheader = injected";
    let error = resolve_token(|_| Some(value.into()), || panic!("must not fall back")).unwrap_err();
    assert!(error.contains("invalid characters"));
    assert!(!error.contains("fixture-secret"));
}

fn helper(root: &Path, contents: &str) -> std::path::PathBuf {
    let path = root.join("fake-gh");
    std::fs::write(&path, format!("#!/bin/sh\n{contents}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    path
}

#[test]
fn stored_login_lookup_is_noninteractive_and_scoped_to_github_com() {
    let root = tempfile::tempdir().unwrap();
    let gh = helper(
        root.path(),
        r#"
[ "$*" = 'auth token --hostname github.com' ] || exit 2
[ "$GH_PROMPT_DISABLED" = 1 ] || exit 2
[ "$GH_NO_UPDATE_NOTIFIER" = 1 ] || exit 2
[ -z "${GH_TOKEN+x}" ] && [ -z "${GITHUB_TOKEN+x}" ] || exit 2
printf 'stored-fixture-token\n'
"#,
    );
    assert_eq!(
        stored_token_with(&gh, Duration::from_secs(1))
            .unwrap()
            .as_deref(),
        Some("stored-fixture-token")
    );
}

#[test]
fn absent_or_logged_out_github_cli_keeps_public_fetches_available() {
    let root = tempfile::tempdir().unwrap();
    assert!(
        stored_token_with(&root.path().join("absent"), Duration::from_secs(1))
            .unwrap()
            .is_none()
    );
    let gh = helper(root.path(), "exit 1");
    assert!(
        stored_token_with(&gh, Duration::from_secs(1))
            .unwrap()
            .is_none()
    );
}

#[test]
fn credential_lookup_timeout_is_bounded_and_actionable() {
    let root = tempfile::tempdir().unwrap();
    let gh = helper(root.path(), "while :; do :; done");
    let started = std::time::Instant::now();
    let error = stored_token_with(&gh, Duration::from_millis(30)).unwrap_err();
    assert!(error.contains("timed out"), "{error}");
    assert!(error.contains("GH_TOKEN"));
    assert!(started.elapsed() < Duration::from_secs(2));
}

#[test]
fn stored_credentials_are_bounded_and_validated() {
    let mut file = tempfile::tempfile().unwrap();
    file.write_all(&vec![b'a'; 16 * 1024 + 1]).unwrap();
    file.rewind().unwrap();
    assert!(read_token(file).unwrap_err().contains("size limit"));
}
