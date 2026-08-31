use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io;

use serde::Deserialize;

use crate::cli::Agent;
use crate::config::{
    AgentProviderPolicy, GitHubReleaseSource, LockedAgentProvider, LockedToolDownload,
    ToolDownloadSource,
};

#[path = "model_persistence.rs"]
mod model_persistence;
#[path = "model_validation.rs"]
mod model_validation;

pub use model_persistence::{
    apply_selection_to_document, config_file_defines_agent_selection,
    config_file_defines_tool_selection, configured_packages_from_document, load_package_file,
    write_selected_tools, write_selected_tools_with_release_age,
};
use model_validation::validate_catalog;

const LEGACY_MANAGED_BY_KEY: &str = "ags_managed_by";
const LEGACY_MANAGED_BY_VALUE: &str = "tool-configurator";
const PROFESSIONS: &[(&str, &str)] = &[
    ("ai-tools", "AI Tools"),
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
    ReleaseResolve { item: String, message: String },
}

impl fmt::Display for ToolConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "I/O error: {error}"),
            Self::Json(error) => write!(f, "tool catalog JSON error: {error}"),
            Self::Config(error) => write!(f, "config error: {error}"),
            Self::ConfigParse(error) => write!(f, "config TOML parse error: {error}"),
            Self::InvalidPackage(error) => write!(f, "invalid tool catalog: {error}"),
            Self::ReleaseResolve { item, message } => {
                write!(f, "failed to resolve {item}: {message}")
            }
        }
    }
}

impl std::error::Error for ToolConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Json(error) => Some(error),
            Self::Config(_)
            | Self::ConfigParse(_)
            | Self::InvalidPackage(_)
            | Self::ReleaseResolve { .. } => None,
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
    pub agents: Vec<AgentDefinition>,
    pub tools: Vec<ToolDefinition>,
    pub groups: Vec<ToolGroupDefinition>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AgentDefinition {
    pub id: Agent,
    pub name: String,
    pub description: String,
    pub provider: AgentProviderPolicy,
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
    #[serde(default)]
    pub github_release: Option<GitHubReleaseSource>,
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
    pub definition: AgentDefinition,
    pub selected: bool,
    pub touched: bool,
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
    configured_agent_providers: Vec<LockedAgentProvider>,
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
        Self::from_catalog_with_config_and_agent_providers(
            catalog,
            configured_packages,
            configured_downloads,
            configured_agents,
            &[],
        )
    }

    pub fn from_catalog_with_config_and_agent_providers(
        catalog: ToolCatalog,
        configured_packages: Option<&[String]>,
        configured_downloads: Option<&[LockedToolDownload]>,
        configured_agents: &[Agent],
        configured_agent_providers: &[LockedAgentProvider],
    ) -> Result<Self, ToolConfigError> {
        validate_catalog(&catalog)?;
        let ToolCatalog {
            agents: catalog_agents,
            tools: catalog_tools,
            groups: catalog_groups,
        } = catalog;
        let mut agent_definitions = catalog_agents
            .into_iter()
            .map(|definition| (definition.id, definition))
            .collect::<BTreeMap<_, _>>();
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
        let tool_indices = catalog_tools
            .iter()
            .enumerate()
            .map(|(index, tool)| (tool.id.clone(), index))
            .collect::<BTreeMap<_, _>>();

        let tools = catalog_tools
            .into_iter()
            .map(|definition| ToolState {
                selected: if explicit_selection {
                    if definition.download.is_some() || definition.github_release.is_some() {
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
        let groups = catalog_groups
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
                .filter_map(|agent| agent_definitions.remove(&agent))
                .map(|definition| AgentState {
                    selected: configured_agents.contains(&definition.id),
                    definition,
                    touched: false,
                })
                .collect(),
            groups,
            configured_packages,
            configured_downloads,
            configured_agents: configured_agents.to_vec(),
            configured_agent_providers: configured_agent_providers.to_vec(),
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
            agent.touched = true;
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

    fn selected_downloads(
        &self,
        minimum_release_age: u32,
    ) -> Result<Vec<LockedToolDownload>, ToolConfigError> {
        let catalog_ids = self
            .tools
            .iter()
            .map(|tool| tool.definition.id.as_str())
            .collect::<BTreeSet<_>>();
        let selected_commands = self
            .tools
            .iter()
            .filter(|tool| tool.selected)
            .filter_map(|tool| {
                tool.definition
                    .download
                    .as_ref()
                    .map(|download| download.install_as.as_str())
                    .or_else(|| {
                        tool.definition
                            .github_release
                            .as_ref()
                            .map(|source| source.install_as.as_str())
                    })
            })
            .collect::<BTreeSet<_>>();
        let mut downloads = Vec::new();
        for tool in self.tools.iter().filter(|tool| tool.selected) {
            let download = if let Some(download) = &tool.definition.download {
                Some(download.clone())
            } else if let Some(source) = &tool.definition.github_release {
                Some(
                    crate::github_release::resolve_github_release_source(
                        source,
                        minimum_release_age,
                    )
                    .map_err(|error| ToolConfigError::ReleaseResolve {
                        item: format!("tool '{}'", tool.definition.id),
                        message: error.to_string(),
                    })?,
                )
            } else {
                None
            };
            if let Some(download) = download {
                downloads.push(LockedToolDownload {
                    id: tool.definition.id.clone(),
                    download,
                });
            }
        }
        downloads.extend(
            self.configured_downloads
                .iter()
                .filter(|tool| {
                    !catalog_ids.contains(tool.id.as_str())
                        && !selected_commands.contains(tool.download.install_as.as_str())
                })
                .cloned(),
        );
        Ok(downloads)
    }

    fn selected_agents(&self) -> Vec<Agent> {
        self.agents
            .iter()
            .filter(|agent| agent.selected)
            .map(|agent| agent.definition.id)
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
