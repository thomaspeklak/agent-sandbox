use std::path::Path;

use super::{build_helper_run_args, install_script, list_script};
use crate::node_runtime::NODE_STORE_CONTAINER;

#[test]
fn helper_install_mount_is_writable_and_networked() {
    let args = build_helper_run_args(
        "localhost/agent-sandbox:latest",
        Path::new("/tmp/ags/mise"),
        "true",
        None,
        "rw",
    );
    // No explicit backend is selected: Podman chooses its compatible default
    // network for the download helper.
    assert!(!args.contains(&"--network".to_owned()));
    assert!(args.iter().any(|arg| arg.ends_with(":/opt/ags/mise:rw")));
}

#[test]
fn helper_list_mount_is_read_only_and_offline() {
    let args = build_helper_run_args(
        "image",
        Path::new("/tmp/store"),
        list_script(),
        Some("none"),
        "ro",
    );
    assert!(args.contains(&"--network".to_owned()));
    assert!(args.contains(&"none".to_owned()));
    assert!(args.iter().any(|arg| arg.ends_with(":/opt/ags/mise:ro")));
    assert!(list_script().contains("--no-config ls --installed node"));
}

#[test]
fn install_script_disables_project_config_and_targets_mise_store() {
    let script = install_script("22.14.0");
    assert!(script.contains("mise --no-config install node@22.14.0"));
    assert!(script.contains(NODE_STORE_CONTAINER));
}
