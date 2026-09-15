use std::fs;

use ags::node_runtime::{nearest_nvmrc, validate_node_version};

#[test]
fn validates_numeric_node_selectors_and_rejects_shell_content() {
    assert_eq!(validate_node_version("v22.14.0").unwrap(), "22.14.0");
    assert_eq!(validate_node_version("node@20").unwrap(), "20");
    for value in ["lts/*", "$(touch /tmp/pwned)", "22 && echo bad", "22\n20"] {
        assert!(validate_node_version(value).is_err(), "accepted {value:?}");
    }
}

#[test]
fn nearest_nvmrc_stays_inside_boundary_and_prefers_nested_file() {
    let root = tempfile::tempdir().unwrap();
    let nested = root.path().join("packages/app/src");
    fs::create_dir_all(&nested).unwrap();
    fs::write(root.path().join(".nvmrc"), "20\n").unwrap();
    fs::write(root.path().join("packages/app/.nvmrc"), "v22.14.0\n").unwrap();

    let found = nearest_nvmrc(&nested, root.path()).unwrap().unwrap();
    assert_eq!(found.1, "22.14.0");
    assert_eq!(found.0, root.path().join("packages/app/.nvmrc"));

    let outside = tempfile::tempdir().unwrap();
    assert!(
        nearest_nvmrc(outside.path(), root.path())
            .unwrap()
            .is_none()
    );
}

#[test]
fn symlink_nvmrc_is_rejected() {
    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("version");
    fs::write(&target, "22").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(&target, root.path().join(".nvmrc")).unwrap();
    #[cfg(unix)]
    assert!(nearest_nvmrc(root.path(), root.path()).is_err());
}
