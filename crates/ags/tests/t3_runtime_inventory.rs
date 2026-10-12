#[test]
fn published_t3_runtime_view_and_inventory_contract() {
    let test = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/t3_runtime.test.cjs");
    let output = std::process::Command::new("node")
        .arg("--test")
        .arg(test)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn t3_forwarding_streams_without_optional_packages() {
    let test =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/t3_forwarding.test.cjs");
    let output = std::process::Command::new("node")
        .arg("--test")
        .arg(test)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
