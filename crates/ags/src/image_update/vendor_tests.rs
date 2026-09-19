use super::*;

fn tool(id: &str, install_as: &str, sha: &str) -> LockedToolDownload {
    serde_json::from_value(serde_json::json!({
        "id": id,
        "download": {
            "version": "1.0.0",
            "archive": "tar.gz",
            "member": id,
            "install_as": install_as,
            "artifacts": {
                "x86_64": {"url": format!("https://example.com/{id}-x86_64.tar.gz"), "sha256": sha.repeat(64)},
                "aarch64": {"url": format!("https://example.com/{id}-aarch64.tar.gz"), "sha256": "f".repeat(64)}
            }
        }
    }))
    .unwrap()
}

fn amd64() -> Platform {
    Platform::from_podman("linux", "amd64").unwrap()
}

#[test]
fn selects_the_host_artifact_sorted_by_destination() {
    let tools = [tool("dcg", "dcg", "A"), tool("br", "br", "b")];
    let selected = select(&amd64(), &tools).unwrap();
    let names: Vec<&str> = selected.iter().map(|tool| tool.inputs.install_as).collect();
    assert_eq!(names, ["br", "dcg"]);
    assert_eq!(selected[0].url, "https://example.com/br-x86_64.tar.gz");
    // Lock checksums are compared in lowercase.
    assert_eq!(selected[1].inputs.sha256, "a".repeat(64));

    let arm = select(&Platform::from_podman("linux", "arm64").unwrap(), &tools).unwrap();
    assert_eq!(arm[0].url, "https://example.com/br-aarch64.tar.gz");
    assert_ne!(arm[0].key, selected[0].key);
}

#[test]
fn tool_id_and_version_do_not_affect_the_artifact_key() {
    let original = [tool("br", "br", "a")];
    let mut renamed = tool("beads", "br", "a");
    renamed.download.member = "br".to_owned();
    renamed.download.version = "2.0.0".to_owned();
    let renamed = [renamed];
    assert_eq!(
        select(&amd64(), &original).unwrap()[0].key,
        select(&amd64(), &renamed).unwrap()[0].key
    );
}

#[test]
fn rejects_destination_conflicts_and_reserved_commands() {
    let duplicate = [tool("br", "br", "a"), tool("other", "br", "b")];
    assert!(
        select(&amd64(), &duplicate)
            .unwrap_err()
            .to_string()
            .contains("also installs /usr/local/bin/br")
    );
    let reserved = [tool("pnpm", "pnpm", "a")];
    assert!(
        select(&amd64(), &reserved)
            .unwrap_err()
            .to_string()
            .contains("owned by the sandbox image")
    );
}

#[test]
fn rejects_tools_without_a_host_artifact() {
    let mut missing = tool("br", "br", "a");
    missing.download.artifacts.remove("x86_64");
    assert!(select(&amd64(), &[missing]).is_err());
}

#[test]
fn download_store_entries_are_revalidated_before_reuse() {
    let store = tempfile::tempdir().unwrap();
    let payload = b"archive bytes";
    let sha = crate::image_update::inputs::sha256_hex(payload);
    let path = store.path().join(format!("sha256-{sha}"));

    assert_eq!(cached_archive(store.path(), &sha), None);
    std::fs::write(&path, payload).unwrap();
    assert_eq!(file_sha256(&path).unwrap(), sha);
    assert_eq!(cached_archive(store.path(), &sha), Some(path.clone()));

    // A poisoned entry is removed instead of being used.
    std::fs::write(&path, b"tampered").unwrap();
    assert_eq!(cached_archive(store.path(), &sha), None);
    assert!(!path.exists());
}

#[test]
fn archive_names_match_the_recipe_contract() {
    assert_eq!(archive_name(crate::config::ToolArchiveFormat::Zip), "zip");
    assert_eq!(
        archive_name(crate::config::ToolArchiveFormat::TarGz),
        "tar.gz"
    );
    assert_eq!(
        archive_name(crate::config::ToolArchiveFormat::TarXz),
        "tar.xz"
    );
    assert_eq!(
        member_match_name(crate::config::ArchiveMemberMatch::UniqueBasename),
        "unique_basename"
    );
}
