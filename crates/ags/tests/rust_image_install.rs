//! Exercise the embedded installer without downloads or host toolchain changes.
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::process::Command;

fn install(existing: bool) -> (tempfile::TempDir, std::process::Output) {
    let root = tempfile::tempdir().unwrap();
    let bin = root.path().join("cargo/bin");
    fs::create_dir_all(&bin).unwrap();
    fs::create_dir(root.path().join("rustup")).unwrap();
    let proxy = r#"#!/bin/sh
set -eu
printf '%s %s\n' "${0##*/}" "$*" >> "$TEST_ROOT/calls"
case "${0##*/}:$*" in
  'rustup:--version') echo 'rustup 1.29.1 (fixture)' ;;
  'rustup:toolchain list')
    if [ -f "$TEST_ROOT/compiler" ]; then echo 'stable-x86_64-unknown-linux-gnu (default)'; fi ;;
  'rustup:toolchain install '* | 'rustup:component add '*)
    touch "$TEST_ROOT/compiler" "$TEST_ROOT/components" ;;
  'rustup:default stable') test -f "$TEST_ROOT/compiler" ;;
  'rustc:+stable -V')
    # rustup proxies may auto-install the minimal profile on a version probe.
    touch "$TEST_ROOT/compiler"
    echo 'rustc 1.99.0 (fixture)' ;;
  'cargo:+stable -V') echo 'cargo 1.99.0' ;;
  'rustfmt:+stable --version' | 'cargo:+stable clippy --version')
    test -f "$TEST_ROOT/components" ;;
  *) echo "unexpected command: $0 $*" >&2; exit 1 ;;
esac
"#;
    for name in ["rustup", "rustc", "rustfmt", "cargo"] {
        let path = bin.join(name);
        fs::write(&path, proxy).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }
    if existing {
        fs::write(root.path().join("compiler"), "").unwrap();
        fs::write(root.path().join("components"), "").unwrap();
    }
    let script = ags::assets::image_recipe("rust-install.sh")
        .replace("/usr/local", root.path().to_str().unwrap());
    let path = root.path().join("install.sh");
    fs::write(&path, script).unwrap();
    let output = Command::new("sh")
        .arg(path)
        .env("TEST_ROOT", root.path())
        .env("RUST_TRIPLE", "x86_64-unknown-linux-gnu")
        .env("RUSTUP_VERSION", "1.29.1")
        .env("RUSTC_VERSION", "1.99.0 (fixture)")
        .output()
        .unwrap();
    (root, output)
}

#[test]
fn fresh_install_includes_rustfmt_and_clippy_before_version_probes() {
    let (root, output) = install(false);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let calls = fs::read_to_string(root.path().join("calls")).unwrap();
    let install = calls.find("rustup toolchain install stable").unwrap();
    let probe = calls.find("rustc +stable -V").unwrap();
    assert!(
        install < probe,
        "version probes must not auto-install a minimal toolchain"
    );
    assert!(calls.contains("--component rustfmt --component clippy"));
}

#[test]
fn unchanged_toolchain_is_not_reinstalled() {
    let (root, output) = install(true);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let calls = fs::read_to_string(root.path().join("calls")).unwrap();
    assert!(!calls.contains("rustup toolchain install"));
    assert!(!calls.contains("rustup component add"));
}
