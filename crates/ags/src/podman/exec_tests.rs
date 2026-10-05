use std::path::Path;

use base64::Engine;

use super::build_podman_build_args;
use crate::config::LockedToolDownload;

#[test]
fn image_build_args_include_configured_dnf_packages() {
    let args = build_podman_build_args(
        "localhost/agent-sandbox:latest",
        Path::new("/tmp/Containerfile"),
        Path::new("/tmp"),
        &["ansible-lint".to_owned(), "shellcheck".to_owned()],
        &[],
    );

    assert!(args.contains(&"EXTRA_DNF_PACKAGES=ansible-lint shellcheck".to_owned()));
}

#[test]
fn image_build_args_override_containerfile_default_for_empty_package_list() {
    let args = build_podman_build_args(
        "localhost/agent-sandbox:latest",
        Path::new("/tmp/Containerfile"),
        Path::new("/tmp"),
        &[],
        &[],
    );

    assert!(args.contains(&"EXTRA_DNF_PACKAGES=".to_owned()));
    assert!(args.contains(&"EXTRA_TOOL_DOWNLOADS_B64=W10=".to_owned()));
}

#[test]
fn image_build_args_encode_verified_tool_downloads() {
    let download = serde_json::from_value::<LockedToolDownload>(serde_json::json!({
        "id": "terraform",
        "download": {
            "version": "1.0.0",
            "archive": "zip",
            "member": "terraform",
            "install_as": "terraform",
            "artifacts": {
                "x86_64": {"url": "https://example.com/x.zip", "sha256": "a".repeat(64)},
                "aarch64": {"url": "https://example.com/a.zip", "sha256": "b".repeat(64)}
            }
        }
    }))
    .unwrap();
    let args = build_podman_build_args(
        "localhost/agent-sandbox:latest",
        Path::new("/tmp/Containerfile"),
        Path::new("/tmp"),
        &[],
        &[download],
    );
    let encoded = args
        .iter()
        .find_map(|arg| arg.strip_prefix("EXTRA_TOOL_DOWNLOADS_B64="))
        .unwrap();
    let decoded = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .unwrap();
    let lock: Vec<LockedToolDownload> = serde_json::from_slice(&decoded).unwrap();
    assert_eq!(lock[0].id, "terraform");
}
