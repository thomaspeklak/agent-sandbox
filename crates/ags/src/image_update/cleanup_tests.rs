use super::*;

#[test]
fn nothing_to_clean_without_a_superseded_image() {
    assert_eq!(
        remove_previous_image(None, "new", false),
        PreviousImageCleanup::NotNeeded
    );
    assert_eq!(
        remove_previous_image(Some("sha256:same"), "same", false),
        PreviousImageCleanup::NotNeeded
    );
}

#[test]
fn keep_existing_retains_the_previous_image() {
    let cleanup = remove_previous_image(Some("old"), "new", true);
    assert_eq!(cleanup, PreviousImageCleanup::Kept("old".to_owned()));
    assert!(cleanup.describe().unwrap().contains("--keep-existing"));
}

#[test]
fn failures_are_warnings_after_a_successful_update() {
    let failed = PreviousImageCleanup::Failed {
        image_id: "sha256:1234567890abcdef".to_owned(),
        message: "busy".to_owned(),
    };
    let text = failed.describe().unwrap();
    assert!(text.starts_with("warning:"));
    assert!(text.contains("the update succeeded"));
    assert!(text.contains("podman image rm sha256:1234567890abcdef"));

    let retained = PreviousImageCleanup::Retained {
        image_id: "old".to_owned(),
        container_ids: vec!["c1".to_owned(), "c2".to_owned()],
    };
    assert!(retained.describe().unwrap().contains("c1, c2"));
}

#[test]
fn cleanup_matches_only_exact_image_references() {
    let output = "direct\tsha256:abc123\nchild\tsha256:def456\nexternal\tabc123\nimageless\t\n";
    assert_eq!(
        parse_container_image_refs(output, "sha256:abc123").unwrap(),
        vec!["direct", "external"]
    );
    assert!(parse_container_image_refs("no-tab-here\n", "abc").is_err());
    assert!(parse_container_image_refs("\tabc\n", "abc").is_err());
}

#[test]
fn only_podman_reference_conflicts_are_retention() {
    assert!(is_image_reference_conflict(Some(2)));
    assert!(!is_image_reference_conflict(Some(125)));
    assert!(!is_image_reference_conflict(None));
}

#[test]
fn container_lookup_includes_external_containers() {
    assert_eq!(
        container_image_refs_args(),
        [
            "ps",
            "--all",
            "--external",
            "--no-trunc",
            "--format",
            "{{.ID}}\t{{.ImageID}}"
        ]
    );
}

#[test]
fn short_image_id_strips_prefix_and_truncates() {
    assert_eq!(short_image_id("sha256:1234567890abcdef"), "1234567890ab");
    assert_eq!(short_image_id("abc"), "abc");
}
