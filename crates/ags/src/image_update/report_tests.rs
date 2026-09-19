use super::*;

fn report(image: ImageOutcome) -> UpdateReport {
    UpdateReport {
        recovery: None,
        base: "retained recorded Fedora 44 digest".to_owned(),
        os: "current; checkpoint reused".to_owned(),
        rust: "current; artifact reused".to_owned(),
        rustup: "current".to_owned(),
        pnpm: "updated to 10.21.0; artifact rebuilt".to_owned(),
        vendor: VendorSummary {
            reused: 7,
            changed: 1,
            removed: 0,
        },
        glimpse: "reused".to_owned(),
        image,
        cleanup: PreviousImageCleanup::NotNeeded,
    }
}

#[test]
fn summary_has_one_aligned_line_per_component() {
    let lines = report(ImageOutcome::Published {
        image_id: "sha256:1234567890abcdef".to_owned(),
    })
    .lines();
    assert_eq!(
        lines,
        [
            "Base:       retained recorded Fedora 44 digest",
            "OS:         current; checkpoint reused",
            "Rust:       current; artifact reused",
            "rustup:     current",
            "pnpm:       updated to 10.21.0; artifact rebuilt",
            "Vendor:     7 reused, 1 changed",
            "Glimpse:    reused",
            "Image:      verified and published 1234567890ab",
        ]
    );
}

#[test]
fn no_op_says_the_existing_image_was_retained() {
    let lines = report(ImageOutcome::Retained).lines();
    assert_eq!(
        lines.last().unwrap(),
        "Image:      no changes; existing image retained"
    );
}

#[test]
fn recovery_and_cleanup_notes_are_reported() {
    let mut report = report(ImageOutcome::Retained);
    report.recovery = Some("completed an interrupted publication".to_owned());
    report.cleanup = PreviousImageCleanup::Removed("abc".to_owned());
    let lines = report.lines();
    assert!(lines[0].starts_with("Recovery:   completed"));
    assert!(
        lines
            .last()
            .unwrap()
            .starts_with("Cleanup:    removed previous image")
    );
}

#[test]
fn vendor_summary_mentions_removals_and_empty_selection() {
    let summary = VendorSummary {
        reused: 2,
        changed: 0,
        removed: 1,
    };
    assert_eq!(summary.describe(), "2 reused, 1 removed");
    assert_eq!(VendorSummary::default().describe(), "none selected");
}
