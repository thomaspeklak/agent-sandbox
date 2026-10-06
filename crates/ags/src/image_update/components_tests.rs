use super::*;

#[test]
fn pnpm_summary_reports_version_changes_even_when_the_artifact_is_cached() {
    let release = |version: &str| PnpmRelease {
        version: version.to_owned(),
        integrity: "sha512-fixture".to_owned(),
        sha512: "ab".repeat(64),
    };
    let previous = PnpmRecord {
        release: release("12.9.1"),
        key: "fixture".to_owned(),
        image_id: "fixture".to_owned(),
    };
    for (selected, status) in [
        ("12.9.1", "current"),
        ("12.9.0", "updated to 12.9.0"),
        ("12.9.2", "updated to 12.9.2"),
    ] {
        for (rebuilt, artifact) in [(false, "reused"), (true, "rebuilt")] {
            assert_eq!(
                pnpm_summary(Some(&previous), &release(selected), rebuilt),
                format!("{status}; artifact {artifact}")
            );
            assert_eq!(
                pnpm_summary(None, &release(selected), rebuilt),
                format!("installed {selected}; artifact {artifact}")
            );
        }
    }
}
