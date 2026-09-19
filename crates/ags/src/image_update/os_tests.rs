use super::*;

fn record(baseline_key: &str) -> OsRecord {
    OsRecord {
        baseline_key: baseline_key.to_owned(),
        baseline_id: "a".repeat(64),
        checkpoint_id: "b".repeat(64),
        inventory: "c".repeat(64),
        packages: 400,
        generation: 3,
    }
}

#[test]
fn lineage_continues_only_for_the_same_baseline_without_rebase() {
    assert_eq!(
        select_lineage(Some(&record("k")), "k", false),
        Lineage::Continue(record("k"))
    );
    assert!(matches!(
        select_lineage(Some(&record("k")), "k", true),
        Lineage::Restart(_)
    ));
    assert!(matches!(
        select_lineage(Some(&record("old")), "k", false),
        Lineage::Restart(_)
    ));
    assert!(matches!(
        select_lineage(None, "k", false),
        Lineage::Restart(_)
    ));
}

#[test]
fn check_exit_codes_distinguish_updates_from_failures() {
    assert_eq!(classify_check(Some(0)), CheckResult::Current);
    assert_eq!(classify_check(Some(100)), CheckResult::UpdatesAvailable);
    for code in [Some(1), Some(125), Some(127), None] {
        assert_eq!(classify_check(code), CheckResult::Failed);
    }
}

#[test]
fn check_refreshes_metadata_as_root_without_pulling() {
    let args = check_args("abc");
    assert_eq!(&args[..4], ["run", "--rm", "--pull=never", "--user=0"]);
    assert!(args.ends_with(&[
        "abc".to_owned(),
        "dnf".to_owned(),
        "check-upgrade".to_owned(),
        "--refresh".to_owned(),
        "--setopt=skip_if_unavailable=False".to_owned(),
    ]));
}

#[test]
fn inventory_runs_offline_and_lists_full_package_identities() {
    let args = inventory_args("abc");
    assert!(args.contains(&"--network=none".to_owned()));
    assert!(args.contains(&"--pull=never".to_owned()));
    let format = args.last().unwrap();
    for field in [
        "%{NAME}",
        "%{EPOCHNUM}",
        "%{VERSION}",
        "%{RELEASE}",
        "%{ARCH}",
    ] {
        assert!(format.contains(field), "{field}");
    }
}

#[test]
fn inventory_identity_ignores_order_and_counts_changed_versions() {
    let before = Inventory::parse("bash\t0\t5.2\t1.fc44\tx86_64\ncurl\t0\t8.0\t1.fc44\tx86_64\n");
    let reordered =
        Inventory::parse("\ncurl\t0\t8.0\t1.fc44\tx86_64\nbash\t0\t5.2\t1.fc44\tx86_64\n\n");
    assert_eq!(before, reordered);
    assert_eq!(before.identity(), reordered.identity());

    let after = Inventory::parse("bash\t0\t5.2\t1.fc44\tx86_64\ncurl\t0\t8.1\t1.fc44\tx86_64\n");
    assert_ne!(before.identity(), after.identity());
    assert_eq!(after.changed_since(&before), 1);
    assert_eq!(before.changed_since(&before), 0);
}
