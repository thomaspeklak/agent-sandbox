use super::compatibility::{self, Operation};

fn runner(version: &str) -> String {
    compatibility::RUNNER
        .replace(
            "@@T3_NODE_ENV_SCRIPT@@",
            "prepend_path_if_dir() { :; }\nensure_remote_node_path() { :; }",
        )
        .replace("@@T3_NODE_SCRIPT_PATH@@", "''")
        .replace("@@T3_ARCHIVE_VERSION@@", &format!("'{version}'"))
        .replace(
            "@@T3_RELEASE_BASE_URL@@",
            "'https://github.com/pingdotgg/t3code/releases/download'",
        )
}

pub(super) fn launch(version: &str) -> String {
    compatibility::LAUNCH
        .replace("@@T3_NODE_ENV_SCRIPT@@", "prepend_path_if_dir() { :; }")
        .replace("@@T3_RUNNER_SCRIPT@@", runner(version).trim_end())
        .replace("@@T3_PICK_PORT_SCRIPT@@", "process.exit(1);")
        .replace("@@T3_WAIT_READY_SCRIPT@@", "process.exit(1);")
        .replace("@@T3_RUNTIME_PORT_SCRIPT@@", "process.exit(1);")
}

pub(super) fn pairing(version: &str) -> String {
    compatibility::PAIR
        .replace("@@T3_STATE_KEY@@", "0123456789abcdef")
        .replace("@@T3_RUNNER_SCRIPT@@", runner(version).trim_end())
}

#[test]
fn recognizes_release_templates_without_executing_their_downloaders() {
    let script = launch("0.0.45");
    assert!(script.contains("t3_fetch"));
    assert_eq!(
        compatibility::classify("sh -l -s -- 0123456789abcdef", script.as_bytes()).unwrap(),
        Operation::Launch("0.0.45".into())
    );
    assert_eq!(
        compatibility::classify("sh -s", pairing("0.0.45").as_bytes()).unwrap(),
        Operation::Pair("0.0.45".into())
    );
    assert_eq!(
        compatibility::classify(
            "sh -s",
            compatibility::STOP
                .replace("@@T3_STATE_KEY@@", "0123456789abcdef")
                .as_bytes()
        )
        .unwrap(),
        Operation::Disconnect
    );
    assert_eq!(
        compatibility::classify(
            "sh -s",
            compatibility::LOGS
                .replace("@@T3_STATE_KEY@@", "0123456789abcdef")
                .as_bytes()
        )
        .unwrap(),
        Operation::Logs
    );
}

#[test]
fn rejects_modified_shell_unknown_commands_dev_runners_and_oversized_scripts() {
    let script = launch("0.0.45");
    for bad in [
        format!("{script}\nexec curl https://invalid.example/installer\n"),
        script.replace("T3_ARCHIVE_MODE=1", "T3_ARCHIVE_MODE=0"),
        script.replace(
            "T3_NODE_SCRIPT_PATH=''",
            "T3_NODE_SCRIPT_PATH='/tmp/other.js'",
        ),
        launch("../../other"),
        launch("00.0.45"),
    ] {
        assert!(compatibility::classify("sh -l -s -- 0123456789abcdef", bad.as_bytes()).is_err());
    }
    assert!(compatibility::classify("bash -s", script.as_bytes()).is_err());
    assert!(compatibility::classify("sh -l -s -- ../../other", script.as_bytes()).is_err());
    assert!(
        compatibility::classify("sh -s", &vec![b' '; compatibility::MAX_SCRIPT_BYTES + 1]).is_err()
    );
}

#[test]
fn checks_requested_mounted_and_running_versions_separately() {
    compatibility::require_version("0.0.45", "0.0.45", "0.0.45").unwrap();
    for versions in [
        ("0.0.46", "0.0.45", "0.0.45"),
        ("0.0.45", "0.0.45", "0.0.44"),
    ] {
        let error = compatibility::require_version(versions.0, versions.1, versions.2)
            .unwrap_err()
            .to_string();
        assert!(error.contains("No runtime was downloaded"));
        assert!(error.contains("ags t3 upgrade"));
    }
}
