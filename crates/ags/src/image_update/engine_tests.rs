use super::*;

#[test]
fn parses_host_platform_and_storage() {
    let info = parse_host_info(
        br#"{"host": {"os": "linux", "arch": "arm64", "other": 1},
            "store": {"graphRoot": "/home/dev/.local/share/containers/storage"}}"#,
    )
    .unwrap();
    assert_eq!(info.platform.to_string(), "linux/arm64");
    assert_eq!(info.graph_root, "/home/dev/.local/share/containers/storage");

    assert!(matches!(
        parse_host_info(
            br#"{"host": {"os": "linux", "arch": "riscv64"}, "store": {"graphRoot": "/"}}"#
        ),
        Err(ImageUpdateError::Platform(_))
    ));
    assert!(parse_host_info(b"not json").is_err());
}

#[test]
fn parses_image_inspection() {
    let id = "a".repeat(64);
    let json = format!(
        r#"[{{"Id": "sha256:{id}", "Digest": "sha256:{digest}", "Os": "linux",
            "Architecture": "amd64", "Labels": {{"io.ags.image.component": "rust",
            "io.ags.image.key": "rust-k"}}, "Size": 1}}]"#,
        digest = "b".repeat(64)
    );
    let info = parse_inspect(json.as_bytes()).unwrap();
    assert_eq!(info.id, id);
    assert_eq!(info.arch, "amd64");

    let platform = Platform::from_podman("linux", "amd64").unwrap();
    assert!(info.is_component("rust", "rust-k", &platform));
    assert!(!info.is_component("pnpm", "rust-k", &platform));
    assert!(!info.is_component("rust", "rust-other", &platform));
    let arm = Platform::from_podman("linux", "arm64").unwrap();
    assert!(!info.is_component("rust", "rust-k", &arm));
}

#[test]
fn inspection_tolerates_null_labels_but_not_ambiguity() {
    let unlabeled = parse_inspect(br#"[{"Id": "abc", "Labels": null}]"#).unwrap();
    assert!(unlabeled.labels.is_empty());
    assert!(parse_inspect(b"[]").is_err());
    assert!(parse_inspect(br#"[{"Id": "a"}, {"Id": "b"}]"#).is_err());
}

#[test]
fn normalizes_image_ids() {
    assert_eq!(normalize_id(" sha256:ABC \n"), "abc");
    assert_eq!(normalize_id("abc"), "abc");
}

#[test]
fn label_lookup_and_removal_arguments() {
    assert_eq!(
        images_with_key_args("rust-k"),
        [
            "images",
            "--no-trunc",
            "--filter",
            "label=io.ags.image.key=rust-k",
            "--format",
            "{{.ID}}"
        ]
    );
    // Targeted removal never forces and never prunes parents.
    assert_eq!(
        remove_image_args("abc"),
        ["image", "rm", "--no-prune", "abc"]
    );
}
