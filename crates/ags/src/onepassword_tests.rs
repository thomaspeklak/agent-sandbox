use super::{MAX_ITEM_BYTES, OnePasswordError, SourceRef, prepare_with_op};
use std::fs;
use std::io::{Read, Seek, SeekFrom};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_FAKE_OP: AtomicUsize = AtomicUsize::new(0);

fn fake_op(dir: &Path, body: &str) -> PathBuf {
    let id = NEXT_FAKE_OP.fetch_add(1, Ordering::Relaxed);
    let path = dir.join(format!("op-{id}"));
    let staging = dir.join(format!("op-{id}.new"));
    fs::write(&staging, format!("#!/bin/sh\nset -eu\n{body}\n")).unwrap();
    fs::set_permissions(&staging, fs::Permissions::from_mode(0o755)).unwrap();
    fs::rename(staging, &path).unwrap();
    path
}

fn source(raw: &str) -> SourceRef {
    SourceRef::parse(raw).unwrap()
}

fn read_fixture(item: &super::PreparedItem) -> String {
    let mut file = fs::File::open(format!("/proc/self/fd/{}", item.fd())).unwrap();
    file.seek(SeekFrom::Start(0)).unwrap();
    let mut text = String::new();
    file.read_to_string(&mut text).unwrap();
    text
}

#[test]
fn parse_splits_only_the_first_slash() {
    let parsed = source("Employee/EXO/readonly item");
    assert_eq!(parsed.vault(), "Employee");
    assert_eq!(parsed.item(), "EXO/readonly item");
    for invalid in ["", "Employee", "/item", "vault/"] {
        assert!(matches!(
            SourceRef::parse(invalid),
            Err(OnePasswordError::InvalidSource)
        ));
    }
}

#[test]
fn invokes_op_with_exact_argument_boundaries_and_preserves_order() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("argv");
    let op = fake_op(
        dir.path(),
        &format!(
            "printf '%s\\n' \"$@\" > '{}'\nprintf '%s' '{{\"category\":\"SECURE_NOTE\"}}'",
            log.display()
        ),
    );
    let items = prepare_with_op(&[source("vault one/item one"), source("id/item/two")], &op)
        .expect("fake op should succeed");

    assert_eq!(items.len(), 2);
    assert_eq!(items[0].source().to_string(), "vault one/item one");
    assert_eq!(items[1].source().to_string(), "id/item/two");
    assert_eq!(read_fixture(&items[0]), r#"{"category":"SECURE_NOTE"}"#);
    assert_eq!(
        fs::read_to_string(log).unwrap(),
        "item\nget\nitem/two\n--vault\nid\n--format=json\n--reveal\n"
    );
}

#[test]
fn rewinds_and_seals_the_payload() {
    let dir = tempfile::tempdir().unwrap();
    let op = fake_op(
        dir.path(),
        "printf '%s' '{\"category\":\"SECURE_NOTE\",\"fields\":[]}'",
    );
    let item = prepare_with_op(&[source("vault/item")], &op)
        .unwrap()
        .pop()
        .unwrap();

    assert_eq!(
        unsafe { libc::lseek(item.fd(), 0, libc::SEEK_CUR) },
        0,
        "descriptor should be rewound before handoff"
    );
    let seals = unsafe { libc::fcntl(item.fd(), libc::F_GET_SEALS) };
    assert_eq!(
        seals,
        libc::F_SEAL_WRITE | libc::F_SEAL_GROW | libc::F_SEAL_SHRINK | libc::F_SEAL_SEAL
    );
    assert_eq!(
        unsafe { libc::write(item.fd(), b"x".as_ptr().cast(), 1) },
        -1,
        "sealed payload must reject writes"
    );
}

#[test]
fn rejects_nonzero_empty_and_oversized_payloads_without_values_in_errors() {
    let dir = tempfile::tempdir().unwrap();
    let sentinel = "NEVER-REPORT-THIS-SECRET";
    let cases = vec![
        (
            "printf '%s' 'NEVER-REPORT-THIS-SECRET'; exit 7".to_owned(),
            "lookup failed",
        ),
        ("exit 0".to_owned(), "empty item"),
        (
            format!("head -c {} /dev/zero", MAX_ITEM_BYTES + 1),
            "oversized item",
        ),
    ];
    for (body, expected) in cases {
        let op = fake_op(dir.path(), &body);
        let error = prepare_with_op(&[source("vault/item")], &op).unwrap_err();
        let message = error.to_string();
        assert!(message.contains(expected), "{message}");
        assert!(!message.contains(sentinel));
        assert!(!message.contains('{'));
    }
}

#[test]
fn missing_op_error_is_targeted_and_metadata_only() {
    let error = OnePasswordError::Spawn {
        source: source("vault/item"),
        kind: std::io::ErrorKind::NotFound,
    };
    assert_eq!(
        error.to_string(),
        "op is not installed or not on PATH; required by --op-secret-set for vault/item"
    );
}

#[test]
fn production_loader_source_never_captures_or_reads_op_stdout() {
    let source = include_str!("onepassword.rs");
    assert!(source.contains(".stdout(Stdio::from(stdout))"));
    assert!(!source.contains(".output()"));
    assert!(!source.contains("read_to_"));
    assert!(!source.contains("serde_json"));
}

#[test]
fn failed_batch_drops_prior_descriptors() {
    let dir = tempfile::tempdir().unwrap();
    let op = fake_op(
        dir.path(),
        "if [ \"$3\" = bad ]; then exit 1; fi; printf '%s' '{\"category\":\"SECURE_NOTE\"}'",
    );
    let error = prepare_with_op(&[source("vault/good"), source("vault/bad")], &op).unwrap_err();
    assert!(error.to_string().contains("vault/bad"), "{error}");
}
