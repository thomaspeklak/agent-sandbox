use super::*;
use crate::image_update::state::tests::sample_state;

fn pending(previous: Option<&str>) -> PendingPublication {
    PendingPublication {
        schema: STATE_SCHEMA,
        previous_id: previous.map(str::to_owned),
        candidate_id: "new".to_owned(),
        next: sample_state(),
    }
}

#[test]
fn recovery_commits_a_published_candidate() {
    assert_eq!(
        decide_recovery(Some("new"), &pending(Some("old"))),
        Recovery::CommitCandidate
    );
    assert_eq!(
        decide_recovery(Some("new"), &pending(None)),
        Recovery::CommitCandidate
    );
}

#[test]
fn recovery_keeps_previous_state_when_the_tag_never_moved() {
    assert_eq!(
        decide_recovery(Some("old"), &pending(Some("old"))),
        Recovery::KeepPrevious
    );
    assert_eq!(
        decide_recovery(None, &pending(None)),
        Recovery::KeepPrevious
    );
}

#[test]
fn recovery_reports_external_changes() {
    assert_eq!(
        decide_recovery(Some("other"), &pending(Some("old"))),
        Recovery::External("other".to_owned())
    );
    assert!(matches!(
        decide_recovery(None, &pending(Some("old"))),
        Recovery::External(_)
    ));
}

#[test]
fn component_images_cover_every_committed_component() {
    let state = sample_state();
    let names: Vec<String> = component_images(&state)
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    assert_eq!(
        names,
        [
            "base",
            "foundation",
            "os-baseline",
            "os",
            "rust",
            "pnpm",
            "glimpse",
            "vendor-br"
        ]
    );

    let mut unbuilt = sample_state();
    unbuilt.foundation.image_id.clear();
    assert!(
        !component_images(&unbuilt)
            .iter()
            .any(|(name, _)| name == "foundation")
    );
}
