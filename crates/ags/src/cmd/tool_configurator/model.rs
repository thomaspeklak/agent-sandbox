use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io;

use serde::Deserialize;

use crate::cli::Agent;
use crate::config::{LockedToolDownload, ToolDownloadSource};

#[path = "model_persistence.rs"]
mod model_persistence;
#[path = "model_validation.rs"]
mod model_validation;

pub use model_persistence::{
    apply_selection_to_document, config_file_defines_agent_selection,
    config_file_defines_tool_selection, configured_packages_from_document, load_package_file,
    write_selected_tools,
};
use model_validation::validate_catalog;

const LEGACY_MANAGED_BY_KEY: &str = "ags_managed_by";
const LEGACY_MANAGED_BY_VALUE: &str = "tool-configurator";
const PROFESSIONS: &[(&str, &str)] = &[
    ("general", "General"),
    ("software-development", "Software Development"),
    ("operations-devops", "Operations and DevOps"),
];

#[derive(Debug)]
pub enum ToolConfigError {
    Io(io::Error),
    Json(serde_json::Error),
    Config(String),
    ConfigParse(String),
    InvalidPackage(String),
}

impl fmt::Display for ToolConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "I/O error: {error}"),
            Self::Json(error) => write!(f, "tool catalog JSON error: {error}"),
            Self::Config(error) => write!(f, "config error: {error}"),
            Self::ConfigParse(error) => write!(f, "config TOML parse error: {error}"),
            Self::InvalidPackage(error) => write!(f, "invalid tool catalog: {error}"),
        }
    }
}

impl std::error::Error for ToolConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Json(error) => Some(error),
            Self::Config(_) | Self::ConfigParse(_) | Self::InvalidPackage(_) => None,
        }
    }
}

impl From<io::Error> for ToolConfigError {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for ToolConfigError {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolCatalog {
    pub tools: Vec<ToolDefinition>,
    pub groups: Vec<ToolGroupDefinition>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolDefinition {
    pub id: String,
    pub name: String,
    pub description: String,
    pub default: bool,
    #[serde(default)]
    pub dnf_packages: Vec<String>,
    #[serde(default)]
    pub download: Option<ToolDownloadSource>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolGroupDefinition {
    pub id: String,
    pub name: String,
    pub subcategories: Vec<ToolSubcategoryDefinition>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolSubcategoryDefinition {
    pub name: String,
    pub tools: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ToolState {
    pub definition: ToolDefinition,
    pub selected: bool,
    pub touched: bool,
}

#[derive(Debug, Clone)]
pub struct AgentState {
    pub agent: Agent,
    pub selected: bool,
}

#[derive(Debug, Clone)]
pub struct ToolGroup {
    pub id: String,
    pub name: String,
    pub subcategories: Vec<ToolSubcategory>,
}

#[derive(Debug, Clone)]
pub struct ToolSubcategory {
    pub name: String,
    pub tool_indices: Vec<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupRow<'a> {
    Divider(&'a str),
    Tool(usize),
}

#[derive(Debug, Clone)]
pub struct ToolSelectionState {
    pub tools: Vec<ToolState>,
    pub agents: Vec<AgentState>,
    pub groups: Vec<ToolGroup>,
    configured_packages: Option<Vec<String>>,
    configured_downloads: Vec<LockedToolDownload>,
    configured_agents: Vec<Agent>,
}

impl ToolSelectionState {
    pub fn from_catalog(
        catalog: ToolCatalog,
        configured_packages: &[String],
    ) -> Result<Self, ToolConfigError> {
        Self::from_catalog_with_config(catalog, Some(configured_packages), Some(&[]))
    }

    pub fn from_catalog_or_defaults(
        catalog: ToolCatalog,
        configured_packages: Option<&[String]>,
    ) -> Result<Self, ToolConfigError> {
        let configured_downloads = configured_packages.map(|_| &[][..]);
        Self::from_catalog_with_config(catalog, configured_packages, configured_downloads)
    }

    pub fn from_catalog_with_config(
        catalog: ToolCatalog,
        configured_packages: Option<&[String]>,
        configured_downloads: Option<&[LockedToolDownload]>,
    ) -> Result<Self, ToolConfigError> {
        Self::from_catalog_with_config_and_agents(
            catalog,
            configured_packages,
            configured_downloads,
            &Agent::INSTALLABLE,
        )
    }

    pub fn from_catalog_with_config_and_agents(
        catalog: ToolCatalog,
        configured_packages: Option<&[String]>,
        configured_downloads: Option<&[LockedToolDownload]>,
        configured_agents: &[Agent],
    ) -> Result<Self, ToolConfigError> {
        validate_catalog(&catalog)?;
        let explicit_selection = configured_packages.is_some() || configured_downloads.is_some();
        let configured_packages = configured_packages.map(<[String]>::to_vec);
        let configured = configured_packages
            .as_deref()
            .unwrap_or_default()
            .iter()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        let configured_download_ids = configured_downloads
            .unwrap_or_default()
            .iter()
            .map(|download| download.id.as_str())
            .collect::<BTreeSet<_>>();
        let configured_downloads = configured_downloads.unwrap_or_default().to_vec();
        let tool_indices = catalog
            .tools
            .iter()
            .enumerate()
            .map(|(index, tool)| (tool.id.clone(), index))
            .collect::<BTreeMap<_, _>>();

        let tools = catalog
            .tools
            .into_iter()
            .map(|definition| ToolState {
                selected: if explicit_selection {
                    if definition.download.is_some() {
                        configured_download_ids.contains(definition.id.as_str())
                    } else {
                        definition
                            .dnf_packages
                            .iter()
                            .all(|package| configured.contains(package.as_str()))
                    }
                } else {
                    definition.default
                },
                definition,
                touched: false,
            })
            .collect();
        let groups = catalog
            .groups
            .into_iter()
            .map(|group| ToolGroup {
                id: group.id,
                name: group.name,
                subcategories: group
                    .subcategories
                    .into_iter()
                    .map(|subcategory| ToolSubcategory {
                        name: subcategory.name,
                        tool_indices: subcategory
                            .tools
                            .iter()
                            .map(|id| tool_indices[id])
                            .collect(),
                    })
                    .collect(),
            })
            .collect();

        Ok(Self {
            tools,
            agents: Agent::INSTALLABLE
                .into_iter()
                .map(|agent| AgentState {
                    agent,
                    selected: configured_agents.contains(&agent),
                })
                .collect(),
            groups,
            configured_packages,
            configured_downloads,
            configured_agents: configured_agents.to_vec(),
        })
    }

    pub fn selected_tool_count(&self) -> usize {
        self.tools.iter().filter(|tool| tool.selected).count()
    }

    pub fn selected_agent_count(&self) -> usize {
        self.agents.iter().filter(|agent| agent.selected).count()
    }

    pub fn group_rows(&self, group_index: usize) -> Vec<GroupRow<'_>> {
        let Some(group) = self.groups.get(group_index) else {
            return Vec::new();
        };
        group
            .subcategories
            .iter()
            .flat_map(|subcategory| {
                std::iter::once(GroupRow::Divider(subcategory.name.as_str()))
                    .chain(subcategory.tool_indices.iter().copied().map(GroupRow::Tool))
            })
            .collect()
    }

    pub fn group_tool_count(&self, group_index: usize) -> usize {
        self.group_tool_indices(group_index).len()
    }

    pub fn group_selected_count(&self, group_index: usize) -> usize {
        self.group_tool_indices(group_index)
            .into_iter()
            .filter(|index| self.tools[*index].selected)
            .count()
    }

    pub fn reset_to_defaults(&mut self) {
        for tool in &mut self.tools {
            tool.selected = tool.definition.default;
            tool.touched = true;
        }
        for agent in &mut self.agents {
            agent.selected = true;
        }
    }

    pub fn placements_for_tool(&self, tool_index: usize) -> Vec<String> {
        self.groups
            .iter()
            .flat_map(|group| {
                group
                    .subcategories
                    .iter()
                    .filter(|subcategory| subcategory.tool_indices.contains(&tool_index))
                    .map(|subcategory| format!("{} / {}", group.name, subcategory.name))
            })
            .collect()
    }

    fn group_tool_indices(&self, group_index: usize) -> BTreeSet<usize> {
        self.groups
            .get(group_index)
            .into_iter()
            .flat_map(|group| &group.subcategories)
            .flat_map(|subcategory| subcategory.tool_indices.iter().copied())
            .collect()
    }

    fn catalog_packages(&self) -> BTreeSet<&str> {
        self.tools
            .iter()
            .flat_map(|tool| tool.definition.dnf_packages.iter().map(String::as_str))
            .collect()
    }

    fn selected_packages(&self) -> impl Iterator<Item = &str> {
        self.tools
            .iter()
            .filter(|tool| tool.selected)
            .flat_map(|tool| tool.definition.dnf_packages.iter().map(String::as_str))
    }

    fn default_packages(&self) -> Vec<String> {
        self.tools
            .iter()
            .filter(|tool| tool.definition.default)
            .flat_map(|tool| tool.definition.dnf_packages.iter().cloned())
            .collect()
    }

    fn selected_downloads(&self) -> Vec<LockedToolDownload> {
        let catalog_ids = self
            .tools
            .iter()
            .map(|tool| tool.definition.id.as_str())
            .collect::<BTreeSet<_>>();
        let selected_commands = self
            .tools
            .iter()
            .filter(|tool| tool.selected)
            .filter_map(|tool| tool.definition.download.as_ref())
            .map(|download| download.install_as.as_str())
            .collect::<BTreeSet<_>>();
        self.tools
            .iter()
            .filter(|tool| tool.selected)
            .filter_map(|tool| {
                tool.definition
                    .download
                    .clone()
                    .map(|download| LockedToolDownload {
                        id: tool.definition.id.clone(),
                        download,
                    })
            })
            .chain(
                self.configured_downloads
                    .iter()
                    .filter(|tool| {
                        !catalog_ids.contains(tool.id.as_str())
                            && !selected_commands.contains(tool.download.install_as.as_str())
                    })
                    .cloned(),
            )
            .collect()
    }

    fn selected_agents(&self) -> Vec<Agent> {
        self.agents
            .iter()
            .filter(|agent| agent.selected)
            .map(|agent| agent.agent)
            .collect()
    }

    fn preserves_untouched_package(&self, package: &str) -> bool {
        self.tools
            .iter()
            .find(|tool| {
                tool.definition
                    .dnf_packages
                    .iter()
                    .any(|candidate| candidate == package)
            })
            .is_some_and(|tool| !tool.selected && !tool.touched)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SaveReport {
    pub selected_tools: usize,
    pub selected_agents: usize,
    pub added_components: usize,
    pub removed_components: usize,
    pub added_agents: usize,
    pub removed_agents: usize,
    pub removed_legacy_tools: usize,
    pub cleanup_warning: Option<String>,
}
