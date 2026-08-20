use std::path::PathBuf;

use ags::{
    cmd::tool_configurator::model::{
        ToolDefinition, ToolPackage, ToolSelectionState, apply_selection_to_document,
        config_file_defines_dnf_packages, configured_packages_from_document, load_package_file,
        write_selected_tools,
    },
    config::DEFAULT_EXTRA_DNF_PACKAGES,
};
use toml_edit::DocumentMut;

fn tool(name: &str, dnf_packages: &[&str]) -> ToolDefinition {
    ToolDefinition {
        name: name.to_owned(),
        description: String::new(),
        dnf_packages: dnf_packages
            .iter()
            .map(|package| (*package).to_owned())
            .collect(),
    }
}

#[test]
fn configured_packages_preselect_catalog_tools() {
    let packages = vec![ToolPackage {
        package: "development".to_owned(),
        tools: vec![tool("GitHub CLI", &["gh"]), tool("ripgrep", &["ripgrep"])],
    }];

    let state = ToolSelectionState::from_packages(packages, &["gh".to_owned()]).unwrap();
    let package = &state.packages[0];

    assert_eq!(package.selected_count(), 1);
    assert!(package.tools[0].selected);
    assert!(!package.tools[1].selected);
}

#[test]
fn bundled_tool_is_selected_only_when_all_packages_are_configured() {
    let packages = vec![ToolPackage {
        package: "development".to_owned(),
        tools: vec![tool("GCC toolchain", &["gcc", "gcc-c++"])],
    }];

    let state = ToolSelectionState::from_packages(packages, &["gcc".to_owned()]).unwrap();
    assert!(!state.packages[0].tools[0].selected);
}

#[test]
fn apply_selection_updates_dnf_packages_and_preserves_unknown_entries() {
    let packages = vec![ToolPackage {
        package: "development".to_owned(),
        tools: vec![tool("GitHub CLI", &["gh"]), tool("ripgrep", &["ripgrep"])],
    }];
    let mut state = ToolSelectionState::from_packages(
        packages,
        &["gh".to_owned(), "custom-package".to_owned()],
    )
    .unwrap();
    state.packages[0].tools[0].selected = false;
    state.packages[0].tools[0].touched = true;
    state.packages[0].tools[1].selected = true;
    state.packages[0].tools[1].touched = true;

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
    assert_eq!(report.added_packages, 1);
    assert_eq!(report.removed_packages, 1);
    assert_eq!(report.removed_legacy_tools, 1);
    assert_eq!(
        configured_packages_from_document(&doc),
        vec!["ripgrep", "custom-package"]
    );
    assert_eq!(doc["tool"].as_array_of_tables().unwrap().len(), 1);
}

#[test]
fn untouched_partial_bundle_is_preserved_until_explicitly_deselected() {
    let packages = vec![ToolPackage {
        package: "development".to_owned(),
        tools: vec![tool("GCC toolchain", &["gcc", "gcc-c++"])],
    }];
    let mut state = ToolSelectionState::from_packages(packages, &["gcc".to_owned()]).unwrap();
    let mut doc: DocumentMut = "[sandbox]\nextra_dnf_packages = [\"gcc\"]\n"
        .parse()
        .unwrap();

    apply_selection_to_document(&mut doc, &state);
    assert_eq!(configured_packages_from_document(&doc), vec!["gcc"]);

    state.packages[0].tools[0].touched = true;
    apply_selection_to_document(&mut doc, &state);
    assert!(configured_packages_from_document(&doc).is_empty());
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
    let state = ToolSelectionState::from_packages(
        vec![ToolPackage {
            package: "source-control".to_owned(),
            tools: vec![tool("GitHub CLI", &["gh"])],
        }],
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
fn detects_the_config_layer_that_defines_packages() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path().join("base.toml");
    let overlay = dir.path().join("overlay.toml");
    std::fs::write(&base, "[sandbox]\nextra_dnf_packages = []\n").unwrap();
    std::fs::write(&overlay, "[sandbox]\nimage = \"overlay\"\n").unwrap();

    assert!(config_file_defines_dnf_packages(&base).unwrap());
    assert!(!config_file_defines_dnf_packages(&overlay).unwrap());
}

#[test]
fn apply_selection_materializes_defaults_when_config_field_is_missing() {
    let packages = vec![ToolPackage {
        package: "quality".to_owned(),
        tools: vec![tool("ansible-lint", &["python3-ansible-lint"])],
    }];
    let mut state = ToolSelectionState::from_packages(packages, &[]).unwrap();
    state.packages[0].tools[0].selected = true;
    let mut doc: DocumentMut = "[sandbox]\nimage = \"test\"\n".parse().unwrap();

    let report = apply_selection_to_document(&mut doc, &state);
    let configured = configured_packages_from_document(&doc);

    assert_eq!(report.added_packages, 0);
    assert_eq!(
        configured,
        DEFAULT_EXTRA_DNF_PACKAGES
            .iter()
            .map(|package| (*package).to_owned())
            .collect::<Vec<_>>()
    );
}

#[test]
fn package_validation_rejects_shell_expressions() {
    let packages = vec![ToolPackage {
        package: "development".to_owned(),
        tools: vec![tool("unsafe", &["--setopt=tsflags=nodocs"])],
    }];

    let error = ToolSelectionState::from_packages(packages, &[]).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("not an option or shell expression")
    );
}

#[test]
fn package_validation_rejects_shell_globs() {
    let packages = vec![ToolPackage {
        package: "development".to_owned(),
        tools: vec![tool("unsafe", &["python3*"])],
    }];

    let error = ToolSelectionState::from_packages(packages, &[]).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("not an option or shell expression")
    );
}

#[test]
fn package_validation_rejects_duplicate_dnf_ownership() {
    let packages = vec![ToolPackage {
        package: "development".to_owned(),
        tools: vec![tool("one", &["shared"]), tool("two", &["shared"])],
    }];

    let error = ToolSelectionState::from_packages(packages, &[]).unwrap_err();
    assert!(error.to_string().contains("assigned to more than one tool"));
}

#[test]
fn example_package_json_loads() {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../config/tool-packages.example.json");

    let packages = load_package_file(&path).unwrap();
    assert_eq!(packages.len(), 8);
    assert_eq!(packages[0].package, "general");
    assert_eq!(packages[7].package, "quality");

    let mut catalog_packages = packages
        .iter()
        .flat_map(|group| &group.tools)
        .flat_map(|tool| &tool.dnf_packages)
        .map(String::as_str)
        .collect::<Vec<_>>();
    let mut default_packages = DEFAULT_EXTRA_DNF_PACKAGES.to_vec();
    catalog_packages.sort_unstable();
    default_packages.sort_unstable();
    assert_eq!(catalog_packages, default_packages);
}
