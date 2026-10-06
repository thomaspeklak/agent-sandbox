use super::*;

pub(crate) fn sample_state() -> ImageState {
    let platform = Platform::from_podman("linux", "amd64").unwrap();
    let id = |c: &str| c.repeat(64);
    ImageState {
        schema: STATE_SCHEMA,
        output: OutputIdentity {
            image: "localhost/agent-sandbox:latest".to_owned(),
            platform,
            storage: "/home/dev/.local/share/containers/storage".to_owned(),
        },
        base: BaseRecord {
            reference: "registry.fedoraproject.org/fedora:44".to_owned(),
            digest: format!("sha256:{}", id("0")),
            image_id: id("1"),
        },
        foundation: FoundationRecord {
            key: "foundation-x".to_owned(),
            image_id: id("2"),
            epoch: 0,
        },
        os: OsRecord {
            baseline_key: "os-baseline-x".to_owned(),
            baseline_id: id("3"),
            checkpoint_id: id("4"),
            inventory: id("5"),
            packages: 420,
            generation: 1,
        },
        rust: RustRecord {
            release: RustRelease {
                rustc: "1.92.0 (abcdef012 2026-01-01)".to_owned(),
                cargo: "0.93.0".to_owned(),
                rustfmt: "1.8.0-stable".to_owned(),
                clippy: "0.1.92".to_owned(),
            },
            rustup: "1.28.2".to_owned(),
            key: "rust-x".to_owned(),
            image_id: id("6"),
        },
        pnpm: PnpmRecord {
            release: PnpmRelease {
                version: "10.20.0".to_owned(),
                integrity: "sha512-x".to_owned(),
                sha512: id("7"),
            },
            key: "pnpm-x".to_owned(),
            image_id: id("8"),
        },
        vendor: vec![VendorRecord {
            id: "br".to_owned(),
            version: "0.1.0".to_owned(),
            install_as: "br".to_owned(),
            key: "vendor-x".to_owned(),
            image_id: id("9"),
        }],
        glimpse: ArtifactRecord {
            key: "glimpse-x".to_owned(),
            image_id: id("a"),
        },
        final_image: ArtifactRecord {
            key: "final-x".to_owned(),
            image_id: id("b"),
        },
    }
}

#[test]
fn output_id_separates_image_platform_and_storage() {
    let state = sample_state();
    let id = state.output.id();
    assert_eq!(id.len(), 16);
    assert_eq!(id, state.output.id());

    let mut other = state.output.clone();
    other.image = "localhost/other:latest".to_owned();
    assert_ne!(other.id(), id);
    let mut other = state.output.clone();
    other.platform = Platform::from_podman("linux", "arm64").unwrap();
    assert_ne!(other.id(), id);
    let mut other = state.output.clone();
    other.storage = "/var/lib/containers/storage".to_owned();
    assert_ne!(other.id(), id);
}

#[test]
fn state_round_trips_through_atomic_save() {
    let dir = tempfile::tempdir().unwrap();
    let state = sample_state();
    let paths = StatePaths::new(dir.path(), &state.output);
    save_atomic(&paths.state, &state).unwrap();
    match load_state(&paths.state, &state.output) {
        Loaded::Found(loaded) => assert_eq!(loaded, state),
        other => panic!("unexpected {other:?}"),
    }
    // Replacing leaves no temporary files behind.
    save_atomic(&paths.state, &state).unwrap();
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[test]
fn missing_corrupt_or_foreign_state_is_a_cache_miss() {
    let dir = tempfile::tempdir().unwrap();
    let state = sample_state();
    let paths = StatePaths::new(dir.path(), &state.output);
    assert!(matches!(
        load_state(&paths.state, &state.output),
        Loaded::Missing
    ));

    std::fs::write(&paths.state, b"{ not json").unwrap();
    assert!(matches!(
        load_state(&paths.state, &state.output),
        Loaded::Unusable(_)
    ));

    let mut future = sample_state();
    future.schema = STATE_SCHEMA + 1;
    save_atomic(&paths.state, &future).unwrap();
    assert!(matches!(
        load_state(&paths.state, &state.output),
        Loaded::Unusable(reason) if reason.contains("schema")
    ));

    save_atomic(&paths.state, &state).unwrap();
    let mut other = state.output.clone();
    other.image = "localhost/other:latest".to_owned();
    assert!(matches!(
        load_state(&paths.state, &other),
        Loaded::Unusable(_)
    ));

    let mut value = serde_json::to_value(&state).unwrap();
    value["unexpected"] = serde_json::json!(true);
    std::fs::write(&paths.state, value.to_string()).unwrap();
    assert!(matches!(
        load_state(&paths.state, &state.output),
        Loaded::Unusable(_)
    ));
}

#[test]
fn state_paths_share_the_output_id() {
    let state = sample_state();
    let paths = StatePaths::new(Path::new("/state"), &state.output);
    let id = state.output.id();
    assert_eq!(paths.state, Path::new(&format!("/state/{id}.json")));
    assert_eq!(
        paths.pending,
        Path::new(&format!("/state/{id}.pending.json"))
    );
    assert_eq!(paths.lock, Path::new(&format!("/state/{id}.lock")));
}

#[test]
fn removing_absent_files_is_not_an_error() {
    let dir = tempfile::tempdir().unwrap();
    remove_if_present(&dir.path().join("absent")).unwrap();
}
