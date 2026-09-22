use super::*;

fn inventory(update: &Update, image: &str, files: &[(&str, &str, &str)]) -> RuntimeManifest {
    let mut entries = Vec::new();
    for (key, relative, text) in files {
        let path = update.path.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, text).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        entries.push(Entry {
            key: key.to_string(),
            path: relative.to_string(),
            kind: Kind::File,
            mode: 0o644,
            size: text.len() as u64,
            raw: digest(&path).unwrap(),
            digest: digest(&path).unwrap(),
        });
    }
    RuntimeManifest::from_inventory(
        image.to_owned(),
        serde_json::json!({"pi": "provider"}),
        &serde_json::to_vec(&entries).unwrap(),
    )
    .unwrap()
}

#[test]
fn identical_candidate_preserves_current_previous_and_is_removed() {
    let cache = tempfile::tempdir().unwrap();
    let first = Update::begin(cache.path()).unwrap();
    first
        .finish(inventory(&first, "image", &[("pi", "pnpm-home/pi", "v1")]))
        .unwrap();
    let first_path = first.path.clone();
    drop(first);
    let second = Update::begin(cache.path()).unwrap();
    second
        .finish(inventory(&second, "image", &[("pi", "pnpm-home/pi", "v2")]))
        .unwrap();
    let second_path = second.path.clone();
    drop(second);
    let noop = Update::begin(cache.path()).unwrap();
    assert!(matches!(
        noop.finish(inventory(&noop, "image", &[("pi", "pnpm-home/pi", "v2")]))
            .unwrap(),
        Publication::Unchanged
    ));
    assert!(!noop.path.exists());
    assert_eq!(
        read_selection(&noop.root, "current").unwrap(),
        Some(second_path)
    );
    assert_eq!(
        read_selection(&noop.root, "previous").unwrap(),
        Some(first_path)
    );
}

#[test]
fn physical_pnpm_installation_ids_do_not_rotate_generations() {
    let cache = tempfile::tempdir().unwrap();
    let first = Update::begin(cache.path()).unwrap();
    first
        .finish(inventory(
            &first,
            "image",
            &[("pi", "pnpm-home/global/nonce-a/pi", "same")],
        ))
        .unwrap();
    drop(first);
    let second = Update::begin(cache.path()).unwrap();
    assert!(matches!(
        second
            .finish(inventory(
                &second,
                "image",
                &[("pi", "pnpm-home/global/nonce-b/pi", "same")]
            ))
            .unwrap(),
        Publication::Unchanged
    ));
}

#[test]
fn dependency_only_change_publishes_and_shares_identical_files_without_mutating_old() {
    let cache = tempfile::tempdir().unwrap();
    let first = Update::begin(cache.path()).unwrap();
    first
        .finish(inventory(
            &first,
            "image",
            &[
                ("pi", "pnpm-home/pi", "same top-level version"),
                ("dep", "pnpm-home/dep", "old dependency"),
            ],
        ))
        .unwrap();
    let old = first.path.clone();
    drop(first);
    let second = Update::begin(cache.path()).unwrap();
    let manifest = inventory(
        &second,
        "image",
        &[
            ("pi", "pnpm-home/pi", "same top-level version"),
            ("dep", "pnpm-home/dep", "new dependency"),
        ],
    );
    // Before sealing, installer writes are on independent inodes.
    assert_ne!(
        fs::metadata(old.join("pnpm-home/pi")).unwrap().ino(),
        fs::metadata(second.path.join("pnpm-home/pi"))
            .unwrap()
            .ino()
    );
    let Publication::Published(shared) = second.finish(manifest).unwrap() else {
        panic!("must publish")
    };
    assert_eq!(shared.files, 1);
    assert_eq!(shared.bytes, "same top-level version".len() as u64);
    assert_eq!(
        fs::metadata(old.join("pnpm-home/pi")).unwrap().ino(),
        fs::metadata(second.path.join("pnpm-home/pi"))
            .unwrap()
            .ino()
    );
    assert_eq!(
        fs::read_to_string(old.join("pnpm-home/dep")).unwrap(),
        "old dependency"
    );
    // Unlinking an old generation cannot remove content still used by the new one.
    fs::remove_dir_all(&old).unwrap();
    assert_eq!(
        fs::read_to_string(second.path.join("pnpm-home/pi")).unwrap(),
        "same top-level version"
    );
}

#[test]
fn normalized_shims_with_different_physical_targets_are_never_hard_linked() {
    let cache = tempfile::tempdir().unwrap();
    let first = Update::begin(cache.path()).unwrap();
    let mut manifest = inventory(
        &first,
        "image1",
        &[("shim", "pnpm-home/pi", "exec nonce-one")],
    );
    let semantic = format!("{:x}", Sha256::digest(b"exec normalized"));
    manifest.entries[0].digest = semantic.clone();
    first.finish(manifest).unwrap();
    drop(first);
    let second = Update::begin(cache.path()).unwrap();
    let mut manifest = inventory(
        &second,
        "image2",
        &[("shim", "pnpm-home/pi", "exec nonce-two")],
    );
    manifest.entries[0].digest = semantic;
    let Publication::Published(shared) = second.finish(manifest).unwrap() else {
        panic!("new image must publish")
    };
    assert_eq!(shared.files, 0);
    assert_eq!(
        fs::read_to_string(second.path.join("pnpm-home/pi")).unwrap(),
        "exec nonce-two"
    );
}

#[test]
fn image_and_provider_changes_are_not_noops() {
    let cache = tempfile::tempdir().unwrap();
    let first = Update::begin(cache.path()).unwrap();
    first
        .finish(inventory(
            &first,
            "image1",
            &[("pi", "pnpm-home/pi", "same")],
        ))
        .unwrap();
    drop(first);
    let second = Update::begin(cache.path()).unwrap();
    assert!(matches!(
        second
            .finish(inventory(
                &second,
                "image2",
                &[("pi", "pnpm-home/pi", "same")]
            ))
            .unwrap(),
        Publication::Published(_)
    ));
    drop(second);
    let third = Update::begin(cache.path()).unwrap();
    let mut manifest = inventory(&third, "image2", &[("pi", "pnpm-home/pi", "same")]);
    manifest.request = serde_json::json!({"pi": "other provider"});
    assert!(matches!(
        third.finish(manifest).unwrap(),
        Publication::Published(_)
    ));
}

#[test]
fn pre_manifest_generations_are_rebuilt_without_sharing_their_files() {
    let cache = tempfile::tempdir().unwrap();
    let first = Update::begin(cache.path()).unwrap();
    inventory(&first, "image", &[("pi", "pnpm-home/pi", "same")]);
    first.publish().unwrap(); // Published by the earlier AGS layout, without a manifest.
    drop(first);
    let second = Update::begin(cache.path()).unwrap();
    let Publication::Published(shared) = second
        .finish(inventory(
            &second,
            "image",
            &[("pi", "pnpm-home/pi", "same")],
        ))
        .unwrap()
    else {
        panic!("must migrate")
    };
    assert_eq!(shared.files, 0);
}

#[test]
fn damaged_current_is_rebuilt_and_never_shared() {
    let cache = tempfile::tempdir().unwrap();
    let first = Update::begin(cache.path()).unwrap();
    first
        .finish(inventory(
            &first,
            "image",
            &[("pi", "pnpm-home/pi", "good")],
        ))
        .unwrap();
    fs::write(first.path.join("pnpm-home/pi"), "bad").unwrap();
    drop(first);
    let second = Update::begin(cache.path()).unwrap();
    let Publication::Published(shared) = second
        .finish(inventory(
            &second,
            "image",
            &[("pi", "pnpm-home/pi", "good")],
        ))
        .unwrap()
    else {
        panic!("must repair")
    };
    assert_eq!(shared.files, 0);
}

#[test]
fn candidate_tampering_and_path_traversal_are_rejected() {
    let cache = tempfile::tempdir().unwrap();
    let update = Update::begin(cache.path()).unwrap();
    let manifest = inventory(&update, "image", &[("pi", "pnpm-home/pi", "good")]);
    fs::write(update.path.join("pnpm-home/pi"), "bad").unwrap();
    assert!(update.finish(manifest.clone()).is_err());
    let mut entries = manifest.entries;
    entries[0].path = "pnpm-home/../../outside".to_owned();
    assert!(
        RuntimeManifest::from_inventory(
            "image".to_owned(),
            serde_json::Value::Null,
            &serde_json::to_vec(&entries).unwrap()
        )
        .is_err()
    );
}

#[test]
fn permissions_change_prevents_sharing_and_noop() {
    let cache = tempfile::tempdir().unwrap();
    let first = Update::begin(cache.path()).unwrap();
    first
        .finish(inventory(
            &first,
            "image",
            &[("pi", "pnpm-home/pi", "same")],
        ))
        .unwrap();
    drop(first);
    let second = Update::begin(cache.path()).unwrap();
    let mut manifest = inventory(&second, "image", &[("pi", "pnpm-home/pi", "same")]);
    fs::set_permissions(
        second.path.join("pnpm-home/pi"),
        fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    manifest.entries[0].mode = 0o755;
    let Publication::Published(shared) = second.finish(manifest).unwrap() else {
        panic!("must publish")
    };
    assert_eq!(shared.files, 0);
}
