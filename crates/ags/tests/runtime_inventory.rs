use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::Path;
use std::process::Command;

fn fixture(root: &Path, nonce: &str) -> serde_json::Value {
    let pnpm = root.join("pnpm-home");
    let install = pnpm.join(format!("global/v11/{nonce}/node_modules"));
    let package = install.join(".pnpm/pi@1/node_modules/pi");
    fs::create_dir_all(&package).unwrap();
    fs::write(
        package.join("package.json"),
        r#"{"name":"pi","version":"1"}"#,
    )
    .unwrap();
    fs::write(package.join("dependency.js"), "dependency v1").unwrap();
    fs::write(install.join(".modules.yaml"), format!("prunedAt: {nonce}")).unwrap();
    symlink(".pnpm/pi@1/node_modules/pi", install.join("pi")).unwrap();
    fs::create_dir_all(pnpm.join("bin")).unwrap();
    fs::write(pnpm.join("bin/pi"), format!("#!/bin/sh\n# cmd-shim-target=/usr/local/pnpm/global/v11/{nonce}/node_modules/pi/cli.js\n")).unwrap();
    serde_json::json!({"pi": {"path": install.join("pi"), "version": "1"}})
}

fn snapshot_output(root: &Path, dependencies: &serde_json::Value) -> std::process::Output {
    let module = Path::new(env!("CARGO_MANIFEST_DIR")).join("src/cmd/update_agents_manifest.js");
    let script = format!(
        "const {{ snapshot }} = require({}); console.log(JSON.stringify(snapshot(['pi'], {}, {{'pnpm-home': {}}})));",
        serde_json::to_string(&module).unwrap(),
        dependencies,
        serde_json::to_string(&root.join("pnpm-home")).unwrap()
    );
    Command::new("node").args(["-e", &script]).output().unwrap()
}

fn snapshot(root: &Path, dependencies: &serde_json::Value) -> serde_json::Value {
    let output = snapshot_output(root, dependencies);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn identity(entries: &serde_json::Value) -> Vec<serde_json::Value> {
    entries.as_array().unwrap().iter().map(|entry| serde_json::json!({
        "key": entry["key"], "kind": entry["kind"], "mode": entry["mode"], "digest": entry["digest"]
    })).collect()
}

#[test]
fn fingerprints_ignore_pnpm_nonce_and_install_time_but_detect_dependency_and_mode_changes() {
    if Command::new("node").arg("--version").output().is_err() {
        eprintln!("skipping inventory test: node unavailable");
        return;
    }
    let a = tempfile::tempdir().unwrap();
    let b = tempfile::tempdir().unwrap();
    let deps_a = fixture(a.path(), "nonce-one");
    let deps_b = fixture(b.path(), "nonce-two-is-longer");
    let first = snapshot(a.path(), &deps_a);
    let second = snapshot(b.path(), &deps_b);
    assert_ne!(first, second); // physical paths and raw shim bytes differ
    assert_eq!(identity(&first), identity(&second));
    let dependency = b.path().join("pnpm-home/global/v11/nonce-two-is-longer/node_modules/.pnpm/pi@1/node_modules/pi/dependency.js");
    fs::write(&dependency, "dependency v2").unwrap();
    assert_ne!(identity(&first), identity(&snapshot(b.path(), &deps_b)));
    fs::write(&dependency, "dependency v1").unwrap();
    fs::set_permissions(&dependency, fs::Permissions::from_mode(0o755)).unwrap();
    assert_ne!(identity(&first), identity(&snapshot(b.path(), &deps_b)));
    let external = b.path().join("pnpm-home/.store");
    fs::create_dir(&external).unwrap();
    symlink(
        &external,
        dependency.parent().unwrap().join("external-store"),
    )
    .unwrap();
    let output = snapshot_output(b.path(), &deps_b);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("escapes isolated installation"));
}
