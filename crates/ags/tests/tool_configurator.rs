use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use ags::{
    cmd::tool_configurator::model::{
        GroupRow, ToolCatalog, ToolDefinition, ToolGroupDefinition, ToolSelectionState,
        ToolSubcategoryDefinition, apply_selection_to_document, config_file_defines_tool_selection,
        configured_packages_from_document, load_package_file, write_selected_tools,
    },
    config::{
        BASE_DNF_PACKAGES, DEFAULT_EXTRA_DNF_PACKAGES, LockedToolDownload, ToolArchiveFormat,
        ToolDownloadArtifact, ToolDownloadSource,
    },
};
use toml_edit::DocumentMut;

fn tool(id: &str, dnf_packages: &[&str], default: bool) -> ToolDefinition {
    ToolDefinition {
        id: id.to_owned(),
        name: id.to_owned(),
        description: format!("Use {id} to complete work."),
        default,
        dnf_packages: dnf_packages
            .iter()
            .map(|package| (*package).to_owned())
            .collect(),
        download: None,
    }
}

fn download_source() -> ToolDownloadSource {
    ToolDownloadSource {
        version: "1.0.0".to_owned(),
        archive: ToolArchiveFormat::Zip,
        member: "tool".to_owned(),
        install_as: "tool".to_owned(),
        artifacts: BTreeMap::from([
            (
                "aarch64".to_owned(),
                ToolDownloadArtifact {
                    url: "https://downloads.example.com/tool-arm64.zip".to_owned(),
                    sha256: "a".repeat(64),
                },
            ),
            (
                "x86_64".to_owned(),
                ToolDownloadArtifact {
                    url: "https://downloads.example.com/tool-amd64.zip".to_owned(),
                    sha256: "b".repeat(64),
                },
            ),
        ]),
    }
}

fn download_tool(id: &str, default: bool) -> ToolDefinition {
    let mut download = download_source();
    download.install_as = id.to_owned();
    ToolDefinition {
        id: id.to_owned(),
        name: id.to_owned(),
        description: format!("Use {id} to complete work."),
        default,
        dnf_packages: vec![],
        download: Some(download),
    }
}

fn group(id: &str, name: &str, tools: &[String]) -> ToolGroupDefinition {
    ToolGroupDefinition {
        id: id.to_owned(),
        name: name.to_owned(),
        subcategories: vec![ToolSubcategoryDefinition {
            name: "Area".to_owned(),
            tools: tools.to_vec(),
        }],
    }
}

fn catalog(tools: Vec<ToolDefinition>) -> ToolCatalog {
    let ids = tools.iter().map(|tool| tool.id.clone()).collect::<Vec<_>>();
    ToolCatalog {
        tools,
        groups: vec![
            group("general", "General", &ids),
            group("software-development", "Software Development", &ids),
            group("operations-devops", "Operations and DevOps", &ids),
        ],
    }
}

fn example_catalog() -> ToolCatalog {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../config/tool-packages.example.json");
    load_package_file(&path).unwrap()
}

fn saved_lock_name(config: &Path) -> String {
    let doc: DocumentMut = std::fs::read_to_string(config).unwrap().parse().unwrap();
    doc["sandbox"]["tool_download_lock"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[test]
fn configured_packages_preselect_canonical_tools() {
    let state = ToolSelectionState::from_catalog(
        catalog(vec![
            tool("github-cli", &["gh"], true),
            tool("ripgrep", &["ripgrep"], true),
        ]),
        &["gh".to_owned()],
    )
    .unwrap();

    assert_eq!(state.selected_tool_count(), 1);
    assert!(state.tools[0].selected);
    assert!(!state.tools[1].selected);
}

#[test]
fn configured_download_id_preselects_downloaded_tool() {
    let definition = download_tool("terraform", false);
    let locked = LockedToolDownload {
        id: definition.id.clone(),
        download: definition.download.clone().unwrap(),
    };
    let state = ToolSelectionState::from_catalog_with_config(
        catalog(vec![definition]),
        Some(&[]),
        Some(&[locked]),
    )
    .unwrap();

    assert!(state.tools[0].selected);
}

#[test]
fn bundled_tool_is_selected_only_when_all_packages_are_configured() {
    let state = ToolSelectionState::from_catalog(
        catalog(vec![tool("tmux", &["tmux", "kitty-terminfo"], true)]),
        &["tmux".to_owned()],
    )
    .unwrap();
    assert!(!state.tools[0].selected);
}

#[test]
fn profession_rows_reference_the_same_canonical_tool() {
    let state = ToolSelectionState::from_catalog(
        catalog(vec![tool("openssh-clients", &["openssh-clients"], true)]),
        &[],
    )
    .unwrap();

    assert_eq!(state.group_rows(0)[1], GroupRow::Tool(0));
    assert_eq!(state.group_rows(1)[1], GroupRow::Tool(0));
    assert_eq!(state.group_rows(2)[1], GroupRow::Tool(0));
}

#[test]
fn apply_selection_updates_tools_and_preserves_unknown_entries() {
    let mut state = ToolSelectionState::from_catalog(
        catalog(vec![
            tool("github-cli", &["gh"], true),
            tool("ripgrep", &["ripgrep"], true),
        ]),
        &["gh".to_owned(), "custom-package".to_owned()],
    )
    .unwrap();
    state.tools[0].selected = false;
    state.tools[0].touched = true;
    state.tools[1].selected = true;
    state.tools[1].touched = true;

    let mut doc: DocumentMut = r#"
[sandbox]
image = "test"
extra_dnf_packages = ["gh", "custom-package"]

[[tool]]
name = "legacy"
path = "/usr/bin/legacy"
container_path = "/usr/local/bin/legacy"
ags_managed_by = "tool-configurator"

[[tool]]
name = "custom"
path = "/opt/custom"
container_path = "/usr/local/bin/custom"
"#
    .parse()
    .unwrap();

    let report = apply_selection_to_document(&mut doc, &state);
    assert_eq!(report.selected_tools, 1);
    assert_eq!(report.added_components, 1);
    assert_eq!(report.removed_components, 1);
    assert_eq!(report.removed_legacy_tools, 1);
    assert_eq!(
        configured_packages_from_document(&doc),
        vec!["ripgrep", "custom-package"]
    );
    assert_eq!(doc["tool"].as_array_of_tables().unwrap().len(), 1);
}

#[test]
fn overlay_with_only_download_lock_preserves_effective_base_packages() {
    let state = ToolSelectionState::from_catalog_with_config(
        catalog(vec![tool("github-cli", &["gh"], true)]),
        Some(&["gh".to_owned(), "custom-package".to_owned()]),
        Some(&[]),
    )
    .unwrap();
    let mut overlay: DocumentMut =
        "[sandbox]\ntool_download_lock = \"tool-downloads.existing.lock.json\"\n"
            .parse()
            .unwrap();

    apply_selection_to_document(&mut overlay, &state);

    assert_eq!(
        configured_packages_from_document(&overlay),
        vec!["gh", "custom-package"]
    );
}

#[test]
fn save_removes_fixed_baseline_packages_from_extra_packages() {
    let state = ToolSelectionState::from_catalog(
        catalog(vec![tool("git", &["git"], true)]),
        &[
            "git".to_owned(),
            "bash".to_owned(),
            "sqlite-devel".to_owned(),
        ],
    )
    .unwrap();
    let mut doc: DocumentMut =
        "[sandbox]\nextra_dnf_packages = [\"git\", \"bash\", \"sqlite-devel\"]\n"
            .parse()
            .unwrap();

    let report = apply_selection_to_document(&mut doc, &state);

    assert_eq!(configured_packages_from_document(&doc), vec!["git"]);
    assert_eq!(report.removed_components, 0);
}

#[test]
fn untouched_partial_bundle_is_preserved_until_explicitly_deselected() {
    let mut state = ToolSelectionState::from_catalog(
        catalog(vec![tool("tmux", &["tmux", "kitty-terminfo"], true)]),
        &["tmux".to_owned()],
    )
    .unwrap();
    let mut doc: DocumentMut = "[sandbox]\nextra_dnf_packages = [\"tmux\"]\n"
        .parse()
        .unwrap();

    apply_selection_to_document(&mut doc, &state);
    assert_eq!(configured_packages_from_document(&doc), vec!["tmux"]);

    state.tools[0].touched = true;
    apply_selection_to_document(&mut doc, &state);
    assert!(configured_packages_from_document(&doc).is_empty());
}

#[test]
fn reset_to_defaults_uses_tool_metadata() {
    let mut state = ToolSelectionState::from_catalog(
        catalog(vec![
            tool("recommended", &["recommended"], true),
            tool("optional", &["optional"], false),
        ]),
        &["optional".to_owned()],
    )
    .unwrap();

    state.reset_to_defaults();

    assert!(state.tools[0].selected);
    assert!(!state.tools[1].selected);
    assert!(state.tools.iter().all(|tool| tool.touched));
}

#[test]
fn omitted_config_uses_the_supplied_catalog_defaults() {
    let state = ToolSelectionState::from_catalog_or_defaults(
        catalog(vec![
            tool("custom-default", &["custom-default"], true),
            tool("custom-optional", &["custom-optional"], false),
        ]),
        None,
    )
    .unwrap();
    let mut doc: DocumentMut = "[sandbox]\nimage = \"test\"\n".parse().unwrap();

    apply_selection_to_document(&mut doc, &state);

    assert!(state.tools[0].selected);
    assert!(!state.tools[1].selected);
    assert_eq!(
        configured_packages_from_document(&doc),
        vec!["custom-default"]
    );
}

#[test]
fn save_cleans_legacy_tools_from_the_other_config_layer() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().join("config.toml");
    let overlay = dir.path().join("overlay.toml");
    std::fs::write(&base, "[sandbox]\nextra_dnf_packages = [\"gh\"]\n").unwrap();
    std::fs::write(
        &overlay,
        r#"[[tool]]
name = "legacy"
path = "/usr/bin/legacy"
container_path = "/usr/local/bin/legacy"
ags_managed_by = "tool-configurator"
"#,
    )
    .unwrap();
    let state = ToolSelectionState::from_catalog(
        catalog(vec![tool("github-cli", &["gh"], true)]),
        &["gh".to_owned()],
    )
    .unwrap();

    let report = write_selected_tools(&base, Some(&overlay), &state).unwrap();

    assert_eq!(report.removed_legacy_tools, 1);
    assert!(report.cleanup_warning.is_none());
    assert!(
        !std::fs::read_to_string(overlay)
            .unwrap()
            .contains("ags_managed_by")
    );
}

#[test]
fn save_materializes_selected_downloads_as_a_verified_lock() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.toml");
    std::fs::write(&config, "[sandbox]\nextra_dnf_packages = []\n").unwrap();
    let mut state = ToolSelectionState::from_catalog_with_config(
        catalog(vec![download_tool("terraform", false)]),
        Some(&[]),
        Some(&[]),
    )
    .unwrap();
    state.tools[0].selected = true;
    state.tools[0].touched = true;

    let report = write_selected_tools(&config, None, &state).unwrap();

    assert_eq!(report.added_components, 1);
    let saved = std::fs::read_to_string(&config).unwrap();
    assert!(saved.contains("tool_download_lock"));
    let saved: DocumentMut = saved.parse().unwrap();
    let lock_name = saved["sandbox"]["tool_download_lock"].as_str().unwrap();
    assert!(lock_name.starts_with("tool-downloads."));
    assert!(lock_name.ends_with(".lock.json"));
    assert!(PathBuf::from(lock_name).is_relative());
    let lock_path = dir.path().join(lock_name);
    let lock: Vec<LockedToolDownload> =
        serde_json::from_str(&std::fs::read_to_string(lock_path).unwrap()).unwrap();
    assert_eq!(lock.len(), 1);
    assert_eq!(lock[0].id, "terraform");
    assert_eq!(lock[0].download.install_as, "terraform");
}

#[test]
fn save_keeps_previously_referenced_download_lock_immutable() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.toml");
    std::fs::write(&config, "[sandbox]\nextra_dnf_packages = []\n").unwrap();
    let mut state = ToolSelectionState::from_catalog_with_config(
        catalog(vec![download_tool("terraform", false)]),
        Some(&[]),
        Some(&[]),
    )
    .unwrap();
    state.tools[0].selected = true;
    write_selected_tools(&config, None, &state).unwrap();

    let first_config: DocumentMut = std::fs::read_to_string(&config).unwrap().parse().unwrap();
    let first_name = first_config["sandbox"]["tool_download_lock"]
        .as_str()
        .unwrap()
        .to_owned();
    let first_path = dir.path().join(&first_name);
    let first_content = std::fs::read_to_string(&first_path).unwrap();

    state.tools[0].selected = false;
    write_selected_tools(&config, None, &state).unwrap();

    let second_config: DocumentMut = std::fs::read_to_string(&config).unwrap().parse().unwrap();
    let second_name = second_config["sandbox"]["tool_download_lock"]
        .as_str()
        .unwrap();
    assert_ne!(first_name, second_name);
    assert_eq!(std::fs::read_to_string(first_path).unwrap(), first_content);
    assert!(dir.path().join(second_name).is_file());
}

#[test]
fn save_removes_lock_displaced_from_the_restorable_backup() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.toml");
    let backup = config.with_extension("toml.bak");
    std::fs::write(&config, "[sandbox]\nextra_dnf_packages = []\n").unwrap();
    let mut state = ToolSelectionState::from_catalog_with_config(
        catalog(vec![
            download_tool("terraform", false),
            download_tool("opentofu", false),
        ]),
        Some(&[]),
        Some(&[]),
    )
    .unwrap();

    state.tools[0].selected = true;
    write_selected_tools(&config, None, &state).unwrap();
    let first_name = saved_lock_name(&config);

    state.tools[1].selected = true;
    write_selected_tools(&config, None, &state).unwrap();
    let second_name = saved_lock_name(&config);
    assert_eq!(saved_lock_name(&backup), first_name);
    assert!(dir.path().join(&first_name).is_file());

    state.tools[0].selected = false;
    write_selected_tools(&config, None, &state).unwrap();
    let third_name = saved_lock_name(&config);

    assert_ne!(first_name, second_name);
    assert_ne!(second_name, third_name);
    assert_eq!(saved_lock_name(&backup), second_name);
    assert!(!dir.path().join(first_name).exists());
    assert!(dir.path().join(second_name).is_file());
    assert!(dir.path().join(third_name).is_file());
}

#[test]
fn save_does_not_delete_unmanaged_backup_lock_references() {
    for case in ["parent", "absolute", "nested"] {
        let dir = tempfile::tempdir().unwrap();
        let config_dir = dir.path().join("config");
        std::fs::create_dir(&config_dir).unwrap();
        let config = config_dir.join("config.toml");
        let backup = config.with_extension("toml.bak");
        let sentinel = match case {
            "parent" | "absolute" => dir.path().join("sentinel"),
            "nested" => config_dir.join("nested/sentinel"),
            _ => unreachable!(),
        };
        std::fs::create_dir_all(sentinel.parent().unwrap()).unwrap();
        let lock_name = match case {
            "parent" => "../sentinel".to_owned(),
            "absolute" => sentinel.display().to_string(),
            "nested" => "nested/sentinel".to_owned(),
            _ => unreachable!(),
        };
        std::fs::write(&config, "[sandbox]\nextra_dnf_packages = []\n").unwrap();
        std::fs::write(
            &backup,
            format!("[sandbox]\ntool_download_lock = {lock_name:?}\n"),
        )
        .unwrap();
        std::fs::write(&sentinel, "keep").unwrap();
        let state = ToolSelectionState::from_catalog_with_config(
            catalog(vec![download_tool("terraform", false)]),
            Some(&[]),
            Some(&[]),
        )
        .unwrap();

        write_selected_tools(&config, None, &state).unwrap();

        assert_eq!(std::fs::read_to_string(sentinel).unwrap(), "keep");
    }
}

#[test]
fn save_does_not_delete_managed_name_with_mismatched_content() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.toml");
    let backup = config.with_extension("toml.bak");
    let stale_name = format!("tool-downloads.{}.lock.json", "a".repeat(64));
    let stale_path = dir.path().join(&stale_name);
    std::fs::write(&config, "[sandbox]\nextra_dnf_packages = []\n").unwrap();
    std::fs::write(
        backup,
        format!("[sandbox]\ntool_download_lock = {stale_name:?}\n"),
    )
    .unwrap();
    std::fs::write(&stale_path, "not the named content").unwrap();
    let state = ToolSelectionState::from_catalog_with_config(
        catalog(vec![download_tool("terraform", false)]),
        Some(&[]),
        Some(&[]),
    )
    .unwrap();

    write_selected_tools(&config, None, &state).unwrap();

    assert!(stale_path.is_file());
}

#[cfg(unix)]
#[test]
fn save_does_not_remove_a_symlinked_managed_lock() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.toml");
    let backup = config.with_extension("toml.bak");
    let stale_name = format!("tool-downloads.{}.lock.json", "a".repeat(64));
    let stale_path = dir.path().join(&stale_name);
    let sentinel = dir.path().join("sentinel");
    std::fs::write(&config, "[sandbox]\nextra_dnf_packages = []\n").unwrap();
    std::fs::write(
        backup,
        format!("[sandbox]\ntool_download_lock = {stale_name:?}\n"),
    )
    .unwrap();
    std::fs::write(&sentinel, "keep").unwrap();
    std::os::unix::fs::symlink(&sentinel, &stale_path).unwrap();
    let state = ToolSelectionState::from_catalog_with_config(
        catalog(vec![download_tool("terraform", false)]),
        Some(&[]),
        Some(&[]),
    )
    .unwrap();

    write_selected_tools(&config, None, &state).unwrap();

    assert!(
        std::fs::symlink_metadata(stale_path)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(std::fs::read_to_string(sentinel).unwrap(), "keep");
}

#[test]
fn save_drops_unknown_download_that_collides_with_selected_catalog_command() {
    let dir = tempfile::tempdir().unwrap();
    let config = dir.path().join("config.toml");
    std::fs::write(&config, "[sandbox]\nextra_dnf_packages = []\n").unwrap();
    let mut unknown_download = download_source();
    unknown_download.install_as = "terraform".to_owned();
    let unknown = LockedToolDownload {
        id: "legacy-terraform".to_owned(),
        download: unknown_download,
    };
    let mut state = ToolSelectionState::from_catalog_with_config(
        catalog(vec![download_tool("terraform", false)]),
        Some(&[]),
        Some(&[unknown]),
    )
    .unwrap();
    state.tools[0].selected = true;

    let report = write_selected_tools(&config, None, &state).unwrap();

    let saved: DocumentMut = std::fs::read_to_string(&config).unwrap().parse().unwrap();
    let lock_name = saved["sandbox"]["tool_download_lock"].as_str().unwrap();
    let lock: Vec<LockedToolDownload> =
        serde_json::from_str(&std::fs::read_to_string(dir.path().join(lock_name)).unwrap())
            .unwrap();
    assert_eq!(
        lock.iter().map(|tool| tool.id.as_str()).collect::<Vec<_>>(),
        vec!["terraform"]
    );
    assert_eq!(report.removed_components, 1);
}

#[test]
fn detects_the_config_layer_that_defines_packages() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().join("base.toml");
    let overlay = dir.path().join("overlay.toml");
    std::fs::write(&base, "[sandbox]\nextra_dnf_packages = []\n").unwrap();
    std::fs::write(&overlay, "[sandbox]\nimage = \"overlay\"\n").unwrap();

    assert!(config_file_defines_tool_selection(&base).unwrap());
    assert!(!config_file_defines_tool_selection(&overlay).unwrap());
}

#[test]
fn apply_selection_materializes_defaults_when_config_field_is_missing() {
    let mut state = ToolSelectionState::from_catalog(
        example_catalog(),
        &DEFAULT_EXTRA_DNF_PACKAGES
            .iter()
            .map(|package| (*package).to_owned())
            .collect::<Vec<_>>(),
    )
    .unwrap();
    for tool in &mut state.tools {
        tool.touched = true;
    }
    let mut doc: DocumentMut = "[sandbox]\nimage = \"test\"\n".parse().unwrap();

    let report = apply_selection_to_document(&mut doc, &state);

    assert_eq!(report.added_components, 0);
    assert_eq!(
        configured_packages_from_document(&doc),
        DEFAULT_EXTRA_DNF_PACKAGES
    );
}

#[test]
fn catalog_validation_rejects_shell_expressions() {
    let error = ToolSelectionState::from_catalog(
        catalog(vec![tool("unsafe", &["--setopt=tsflags=nodocs"], false)]),
        &[],
    )
    .unwrap_err();
    assert!(
        error
            .to_string()
            .contains("not an option or shell expression")
    );
}

#[test]
fn catalog_validation_requires_exactly_one_install_provider() {
    let mut definition = tool("invalid", &["invalid"], false);
    definition.download = Some(download_source());
    let error = ToolSelectionState::from_catalog(catalog(vec![definition]), &[]).unwrap_err();
    assert!(error.to_string().contains("exactly one"));
}

#[test]
fn catalog_validation_rejects_unverified_downloads() {
    let mut definition = download_tool("download", false);
    definition
        .download
        .as_mut()
        .unwrap()
        .artifacts
        .get_mut("x86_64")
        .unwrap()
        .sha256 = "unverified".to_owned();
    let error = ToolSelectionState::from_catalog(catalog(vec![definition]), &[]).unwrap_err();
    assert!(error.to_string().contains("64 hexadecimal digits"));
}

#[test]
fn catalog_validation_rejects_option_and_glob_archive_members() {
    for member in ["-unsafe", "bin/*", "bin/te?t", "bin/[t]ool"] {
        let mut definition = download_tool("download", false);
        definition.download.as_mut().unwrap().member = member.to_owned();

        let error = ToolSelectionState::from_catalog(catalog(vec![definition]), &[]).unwrap_err();

        assert!(
            error
                .to_string()
                .contains("must not begin with '-' or contain archive glob characters"),
            "member {member:?} produced: {error}"
        );
    }
}

#[test]
fn catalog_validation_rejects_duplicate_dnf_ownership() {
    let error = ToolSelectionState::from_catalog(
        catalog(vec![
            tool("one", &["shared"], false),
            tool("two", &["shared"], false),
        ]),
        &[],
    )
    .unwrap_err();
    assert!(error.to_string().contains("assigned to more than one tool"));
}

#[test]
fn catalog_validation_rejects_baseline_packages_as_tools() {
    let error = ToolSelectionState::from_catalog(
        catalog(vec![tool("certificates", &["ca-certificates"], false)]),
        &[],
    )
    .unwrap_err();
    assert!(error.to_string().contains("fixed baseline package"));
}

#[test]
fn catalog_validation_rejects_unknown_tool_references() {
    let mut catalog = catalog(vec![tool("known", &["known"], false)]);
    catalog.groups[1].subcategories[0].tools[0] = "missing".to_owned();

    let error = ToolSelectionState::from_catalog(catalog, &[]).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("references unknown tool 'missing'")
    );
}

#[test]
fn catalog_validation_requires_exact_profession_names() {
    let mut catalog = catalog(vec![tool("known", &["known"], false)]);
    catalog.groups[1].name = "Development".to_owned();

    let error = ToolSelectionState::from_catalog(catalog, &[]).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("must be named 'Software Development'")
    );
}

#[test]
fn example_catalog_has_profession_views_and_canonical_defaults() {
    let catalog = example_catalog();
    assert_eq!(catalog.groups.len(), 3);
    assert_eq!(catalog.groups[0].name, "General");
    assert_eq!(catalog.groups[1].name, "Software Development");
    assert_eq!(catalog.groups[2].name, "Operations and DevOps");

    let mut catalog_defaults = catalog
        .tools
        .iter()
        .filter(|tool| tool.default)
        .flat_map(|tool| &tool.dnf_packages)
        .map(String::as_str)
        .collect::<Vec<_>>();
    let mut runtime_defaults = DEFAULT_EXTRA_DNF_PACKAGES.to_vec();
    catalog_defaults.sort_unstable();
    runtime_defaults.sort_unstable();
    assert_eq!(catalog_defaults, runtime_defaults);

    let baseline = BASE_DNF_PACKAGES.iter().copied().collect::<BTreeSet<_>>();
    let selectable_packages = catalog
        .tools
        .iter()
        .flat_map(|tool| &tool.dnf_packages)
        .map(String::as_str)
        .collect::<Vec<_>>();
    let selectable = selectable_packages.iter().copied().collect::<BTreeSet<_>>();
    assert_eq!(selectable.len(), selectable_packages.len());
    assert!(baseline.is_disjoint(&selectable));
    assert_eq!(baseline.len(), BASE_DNF_PACKAGES.len());
    for id in ["terraform", "openshift-cli"] {
        let tool = catalog.tools.iter().find(|tool| tool.id == id).unwrap();
        assert!(
            tool.download.is_some(),
            "{id} should use a verified download"
        );
        assert!(tool.dnf_packages.is_empty());
    }
    assert!(
        catalog
            .tools
            .iter()
            .filter(|tool| tool.download.is_some())
            .all(|tool| !tool.default),
        "downloaded tools should remain explicit opt-ins"
    );
    assert!(BASE_DNF_PACKAGES.contains(&"curl"));
    assert!(!catalog.tools.iter().any(|tool| tool.id == "curl"));
    for id in [
        "terraform",
        "openshift-cli",
        "ansible-playbook",
        "kubectl",
        "aws-cli",
        "helm",
        "dig",
        "hcloud",
        "uv",
        "black",
    ] {
        assert!(
            !catalog
                .tools
                .iter()
                .find(|tool| tool.id == id)
                .unwrap()
                .default
        );
    }
}

#[test]
fn requested_rpm_tools_use_fedora_package_names() {
    let catalog = example_catalog();
    let packages = |id: &str| {
        catalog
            .tools
            .iter()
            .find(|tool| tool.id == id)
            .unwrap()
            .dnf_packages
            .clone()
    };

    assert_eq!(packages("ansible-playbook"), vec!["ansible-core"]);
    assert_eq!(packages("kubectl"), vec!["kubernetes-client"]);
    assert_eq!(packages("aws-cli"), vec!["awscli2"]);
    assert_eq!(packages("helm"), vec!["helm"]);
    assert_eq!(packages("dig"), vec!["bind-utils"]);
    assert_eq!(packages("hcloud"), vec!["hcloud"]);
    assert_eq!(packages("uv"), vec!["uv"]);
    assert_eq!(packages("black"), vec!["black"]);
}

#[test]
fn languages_package_managers_and_clipboard_have_requested_placements() {
    let catalog = example_catalog();
    let placements = |group_id: &str, subcategory: &str| {
        catalog
            .groups
            .iter()
            .find(|group| group.id == group_id)
            .unwrap()
            .subcategories
            .iter()
            .find(|area| area.name == subcategory)
            .unwrap()
            .tools
            .clone()
    };

    let languages = vec!["go", "java", "ruby", "zig"];
    assert_eq!(placements("software-development", "Languages"), languages);
    assert_eq!(placements("operations-devops", "Languages"), languages);

    let package_managers = vec!["npm", "python-pip", "uv"];
    assert_eq!(
        placements("software-development", "Package managers"),
        package_managers
    );
    assert_eq!(
        placements("operations-devops", "Package managers"),
        package_managers
    );

    assert!(
        placements("software-development", "Media and desktop")
            .contains(&"wayland-clipboard".to_owned())
    );
    assert_eq!(
        placements("operations-devops", "Desktop integration"),
        vec!["wayland-clipboard"]
    );

    assert_eq!(placements("general", "Network"), vec!["dig"]);
    assert_eq!(
        placements("software-development", "Infrastructure automation"),
        vec!["terraform", "ansible-playbook", "ansible-lint"]
    );
    assert_eq!(
        placements("operations-devops", "Containers and orchestration"),
        vec!["openshift-cli", "kubectl", "helm"]
    );
    assert_eq!(
        placements("software-development", "Cloud platforms"),
        vec!["aws-cli", "hcloud"]
    );
    assert_eq!(
        placements("software-development", "Code quality"),
        vec!["black"]
    );
}
