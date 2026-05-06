use std::collections::BTreeMap;
use std::path::PathBuf;

use ags::cmd::tool_configurator::model::{
    MANAGED_BY_KEY, MANAGED_BY_VALUE, SecretDefinition, SecretInput, ToolDefinition, ToolPackage,
    ToolResolver, ToolSelectionState, apply_selection_to_document, container_path_for_tool,
    load_package_file,
};
use toml_edit::DocumentMut;

struct MockResolver {
    paths: BTreeMap<String, PathBuf>,
}

impl MockResolver {
    fn new(items: &[(&str, &str)]) -> Self {
        Self {
            paths: items
                .iter()
                .map(|(name, path)| ((*name).to_owned(), PathBuf::from(path)))
                .collect(),
        }
    }
}

impl ToolResolver for MockResolver {
    fn resolve_tool(&self, name: &str) -> Option<PathBuf> {
        self.paths.get(name).cloned()
    }
}

fn tool(name: &str) -> ToolDefinition {
    ToolDefinition {
        name: name.to_owned(),
        description: String::new(),
        secrets: BTreeMap::new(),
    }
}

#[test]
fn unavailable_tools_are_not_selected() {
    let packages = vec![ToolPackage {
        package: "development".to_owned(),
        tools: vec![tool("gh"), tool("missing")],
    }];
    let resolver = MockResolver::new(&[("gh", "/usr/bin/gh")]);

    let state = ToolSelectionState::from_packages(packages, &resolver).unwrap();
    let package = &state.packages[0];

    assert_eq!(package.available_count(), 1);
    assert_eq!(package.selected_count(), 1);
    assert!(package.tools[0].selected);
    assert!(!package.tools[1].selected);
    assert!(!package.tools[1].available());
}

#[test]
fn apply_selection_replaces_only_managed_tool_entries() {
    let mut secrets = BTreeMap::new();
    secrets.insert(
        "GH_TOKEN".to_owned(),
        SecretInput::Spec(SecretDefinition {
            description: "GitHub API token".to_owned(),
            required: true,
            from_env: Some("GH_TOKEN".to_owned()),
            secret_store: None,
        }),
    );
    let packages = vec![ToolPackage {
        package: "development".to_owned(),
        tools: vec![
            ToolDefinition {
                name: "gh".to_owned(),
                description: "GitHub CLI".to_owned(),
                secrets,
            },
            tool("jq"),
        ],
    }];
    let resolver = MockResolver::new(&[("gh", "/usr/bin/gh"), ("jq", "/usr/bin/jq")]);
    let mut state = ToolSelectionState::from_packages(packages, &resolver).unwrap();
    state.packages[0].tools[1].selected = false;

    let mut doc: DocumentMut = r#"
[sandbox]
image = "test"

[[tool]]
name = "custom"
path = "/opt/custom"
container_path = "/usr/local/bin/custom"

[[tool]]
name = "old-managed"
path = "/tmp/old"
container_path = "/usr/local/bin/old-managed"
ags_managed_by = "tool-configurator"
"#
    .parse()
    .unwrap();

    let report = apply_selection_to_document(&mut doc, &state);
    assert_eq!(report.removed_tools, 1);
    assert_eq!(report.added_tools, 1);

    let tools = doc["tool"].as_array_of_tables().unwrap();
    assert_eq!(tools.len(), 2);
    assert_eq!(tools.get(0).unwrap()["name"].as_str().unwrap(), "custom");

    let configured = tools.get(1).unwrap();
    assert_eq!(configured["name"].as_str().unwrap(), "gh");
    assert_eq!(configured["path"].as_str().unwrap(), "/usr/bin/gh");
    assert_eq!(
        configured["container_path"].as_str().unwrap(),
        container_path_for_tool("gh")
    );
    assert_eq!(
        configured[MANAGED_BY_KEY].as_str().unwrap(),
        MANAGED_BY_VALUE
    );

    let tool_secrets = configured["secret"].as_array_of_tables().unwrap();
    let secret = tool_secrets.get(0).unwrap();
    assert_eq!(secret["env"].as_str().unwrap(), "GH_TOKEN");
    assert_eq!(secret["from_env"].as_str().unwrap(), "GH_TOKEN");
}

#[test]
fn package_validation_rejects_binary_paths() {
    let packages = vec![ToolPackage {
        package: "development".to_owned(),
        tools: vec![tool("/usr/bin/gh")],
    }];
    let resolver = MockResolver::new(&[]);

    let error = ToolSelectionState::from_packages(packages, &resolver).unwrap_err();
    assert!(error.to_string().contains("must be a command name"));
}

#[test]
fn example_package_json_loads() {
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../config/tool-packages.example.json");

    let packages = load_package_file(&path).unwrap();
    assert_eq!(packages.len(), 2);
    assert_eq!(packages[0].package, "development");
    assert_eq!(packages[1].package, "ops");
}
