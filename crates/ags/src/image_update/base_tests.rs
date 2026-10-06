use super::*;

fn record(reference: &str) -> BaseRecord {
    BaseRecord {
        reference: reference.to_owned(),
        digest: format!("sha256:{}", "a".repeat(64)),
        image_id: "b".repeat(64),
    }
}

#[test]
fn normal_updates_keep_the_recorded_base() {
    let recorded = record(&fedora_reference());
    assert_eq!(
        plan(Some(&recorded), false),
        BasePlan::Recorded(recorded.clone())
    );
}

#[test]
fn first_use_reuses_a_local_release_image() {
    assert_eq!(plan(None, false), BasePlan::InitializeMissingOnly);
    // A record for another release (an older AGS) starts over.
    assert_eq!(
        plan(Some(&record("registry.fedoraproject.org/fedora:43")), false),
        BasePlan::InitializeMissingOnly
    );
}

#[test]
fn rebase_always_pulls_the_configured_release() {
    assert_eq!(plan(None, true), BasePlan::PullRelease);
    assert_eq!(
        plan(Some(&record(&fedora_reference())), true),
        BasePlan::PullRelease
    );
    assert_eq!(
        fedora_reference(),
        format!("registry.fedoraproject.org/fedora:{FEDORA_RELEASE}")
    );
}

#[test]
fn digests_must_be_full_sha256_references() {
    assert!(valid_digest(&format!("sha256:{}", "a".repeat(64))));
    assert!(!valid_digest(""));
    assert!(!valid_digest(&"a".repeat(64)));
    assert!(!valid_digest(&format!("sha256:{}", "a".repeat(63))));
    assert!(!valid_digest(&format!("sha256:{}", "g".repeat(64))));
}

#[test]
fn rebase_outcome_distinguishes_a_same_digest_rebase() {
    assert!(
        BaseOutcome::Rebased { changed: false }
            .describe()
            .contains("new OS lineage")
    );
    assert!(BaseOutcome::Retained.describe().contains("retained"));
}
