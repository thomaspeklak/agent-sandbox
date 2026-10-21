use super::*;
use std::fs;
use std::os::unix::fs::PermissionsExt;

fn op_fixture(root: &Path, program: &str) -> std::path::PathBuf {
    let path = root.join("op-fixture");
    fs::write(&path, format!("#!/usr/bin/env python3\n{program}\n")).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    path
}
#[test]
fn deduplicated_single_inject_batch_returns_shared_values_without_credentials() {
    let root = tempfile::tempdir().unwrap();
    let calls = root.path().join("calls");
    let input = root.path().join("input");
    let program = format!(
        r#"
import sys
if '--help' in sys.argv:
    print('--in-file --out-file'); sys.exit(0)
with open({calls:?},'a') as f: f.write('inject\n')
b = sys.stdin.buffer.read()
with open({input:?},'wb') as f: f.write(b)
sys.stdout.buffer.write(b.replace(b'{{{{ op://vault/item/a }}}}',b'fixture-a').replace(b'{{{{ op://vault/item/b }}}}',b'fixture-b'))
"#,
        calls = calls.to_str().unwrap(),
        input = input.to_str().unwrap()
    );
    let op = op_fixture(root.path(), &program);
    let refs = [
        ("A".into(), "op://vault/item/a".into()),
        ("B".into(), "op://vault/item/a".into()),
        ("C".into(), "op://vault/item/b".into()),
    ]
    .into();
    let values = resolve_with_op(&refs, &op).unwrap();
    assert_eq!(
        values,
        [
            ("A".into(), "fixture-a".into()),
            ("B".into(), "fixture-a".into()),
            ("C".into(), "fixture-b".into())
        ]
    );
    assert_eq!(fs::read_to_string(calls).unwrap(), "inject\n");
    let input = fs::read(input).unwrap();
    assert_eq!(
        input.split(|b| *b == 0).filter(|s| !s.is_empty()).count(),
        2
    );
}
#[test]
fn empty_surviving_reference_set_does_not_launch_op() {
    assert!(
        resolve_with_op(&BTreeMap::new(), Path::new("/does-not-exist/op"))
            .unwrap()
            .is_empty()
    );
}
#[test]
fn invalid_refs_unsupported_capabilities_failed_batches_and_invalid_values_fail_redacted() {
    for reference in ["not-op", "op://v/i", "op://v/i/{{code}}", "op://v/i/f\n"] {
        assert!(validate_reference(reference).is_err());
    }
    let root = tempfile::tempdir().unwrap();
    let refs = [("X".into(), "op://v/i/f".into())].into();
    for code in [
        "import sys; print('unsupported')",
        "import sys\nif '--help' in sys.argv: print('--in-file --out-file'); sys.exit(0)\nsys.stderr.write('TOP_SECRET'); sys.exit(9)",
        "import sys\nif '--help' in sys.argv: print('--in-file --out-file'); sys.exit(0)\nsys.stdin.buffer.read(); sys.stdout.buffer.write(b'TOP_SECRET\\n\\0')",
        "import sys\nif '--help' in sys.argv: print('--in-file --out-file'); sys.exit(0)\nsys.stdin.buffer.read(); sys.stdout.buffer.write(b'TOP_SECRET')",
    ] {
        let op = op_fixture(root.path(), code);
        let error = resolve_with_op(&refs, &op).unwrap_err();
        assert!(!error.contains("TOP_SECRET"));
    }
}
