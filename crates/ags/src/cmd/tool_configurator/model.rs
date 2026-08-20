use std::collections::BTreeSet;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use toml_edit::{Array, DocumentMut, Item, Table};

use crate::config::DEFAULT_EXTRA_DNF_PACKAGES;

const LEGACY_MANAGED_BY_KEY: &str = "ags_managed_by";
const LEGACY_MANAGED_BY_VALUE: &str = "tool-configurator";

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
            Self::Json(error) => write!(f, "package JSON error: {error}"),
            Self::Config(error) => write!(f, "config error: {error}"),
            Self::ConfigParse(error) => write!(f, "config TOML parse error: {error}"),
            Self::InvalidPackage(error) => write!(f, "invalid tool package: {error}"),
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
pub struct ToolPackage {
    pub package: String,
    #[serde(default)]
    pub tools: Vec<ToolDefinition>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub dnf_packages: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ToolState {
    pub definition: ToolDefinition,
    pub selected: bool,
    pub touched: bool,
}

#[derive(Debug, Clone)]
pub struct PackageState {
    pub package: String,
    pub tools: Vec<ToolState>,
}

impl PackageState {
    pub fn selected_count(&self) -> usize {
        self.tools.iter().filter(|tool| tool.selected).count()
    }

    pub fn all_selected(&self) -> bool {
        !self.tools.is_empty() && self.selected_count() == self.tools.len()
    }
}

#[derive(Debug, Clone)]
pub struct ToolSelectionState {
    pub packages: Vec<PackageState>,
}

impl ToolSelectionState {
    pub fn from_packages(
        packages: Vec<ToolPackage>,
        configured_packages: &[String],
    ) -> Result<Self, ToolConfigError> {
        validate_packages(&packages)?;
        let configured: BTreeSet<&str> = configured_packages.iter().map(String::as_str).collect();

        let packages = packages
            .into_iter()
            .map(|package| PackageState {
                package: package.package,
                tools: package
                    .tools
                    .into_iter()
                    .map(|definition| ToolState {
                        selected: definition
                            .dnf_packages
                            .iter()
                            .all(|package| configured.contains(package.as_str())),
                        definition,
                        touched: false,
                    })
                    .collect(),
            })
            .collect();

        Ok(Self { packages })
    }

    pub fn selected_tool_count(&self) -> usize {
        self.packages
            .iter()
            .flat_map(|package| package.tools.iter())
            .filter(|tool| tool.selected)
            .count()
    }

    fn catalog_packages(&self) -> BTreeSet<&str> {
        self.packages
            .iter()
            .flat_map(|package| package.tools.iter())
            .flat_map(|tool| tool.definition.dnf_packages.iter().map(String::as_str))
            .collect()
    }

    fn selected_packages(&self) -> impl Iterator<Item = &str> {
        self.packages
            .iter()
            .flat_map(|package| package.tools.iter())
            .filter(|tool| tool.selected)
            .flat_map(|tool| tool.definition.dnf_packages.iter().map(String::as_str))
    }

    fn preserves_untouched_package(&self, package: &str) -> bool {
        self.packages
            .iter()
            .flat_map(|group| &group.tools)
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
    pub added_packages: usize,
    pub removed_packages: usize,
    pub removed_legacy_tools: usize,
    pub cleanup_warning: Option<String>,
}

pub fn load_package_file(path: &Path) -> Result<Vec<ToolPackage>, ToolConfigError> {
    let content = fs::read_to_string(path)?;
    let packages = serde_json::from_str::<Vec<ToolPackage>>(&content)?;
    validate_packages(&packages)?;
    Ok(packages)
}

pub fn config_file_defines_dnf_packages(path: &Path) -> Result<bool, ToolConfigError> {
    let content = fs::read_to_string(path)?;
    let doc = content
        .parse::<DocumentMut>()
        .map_err(|error| ToolConfigError::ConfigParse(error.to_string()))?;
    Ok(doc
        .get("sandbox")
        .and_then(|sandbox| sandbox.get("extra_dnf_packages"))
        .is_some())
}

pub fn write_selected_tools(
    config_path: &Path,
    legacy_cleanup_path: Option<&Path>,
    state: &ToolSelectionState,
) -> Result<SaveReport, ToolConfigError> {
    let content = fs::read_to_string(config_path)?;
    let mut doc: DocumentMut = content
        .parse::<DocumentMut>()
        .map_err(|error| ToolConfigError::ConfigParse(error.to_string()))?;

    let mut report = apply_selection_to_document(&mut doc, state);
    backup_file(config_path)?;
    atomic_write(config_path, &doc.to_string())?;
    if let Some(path) = legacy_cleanup_path {
        match remove_legacy_managed_tools_from_file(path) {
            Ok(removed) => report.removed_legacy_tools += removed,
            Err(error) => {
                report.cleanup_warning = Some(format!(
                    "saved package selection, but could not remove legacy tool mounts from {}: {error}",
                    path.display()
                ));
            }
        }
    }
    Ok(report)
}

pub fn configured_packages_from_document(doc: &DocumentMut) -> Vec<String> {
    doc.get("sandbox")
        .and_then(|sandbox| sandbox.get("extra_dnf_packages"))
        .and_then(Item::as_array)
        .map(|packages| {
            packages
                .iter()
                .filter_map(|package| package.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_else(|| {
            DEFAULT_EXTRA_DNF_PACKAGES
                .iter()
                .map(|package| (*package).to_owned())
                .collect()
        })
}

pub fn apply_selection_to_document(
    doc: &mut DocumentMut,
    state: &ToolSelectionState,
) -> SaveReport {
    let previous = configured_packages_from_document(doc);
    let catalog_packages = state.catalog_packages();
    let mut seen = BTreeSet::new();
    let mut configured = Vec::new();

    for package in state
        .selected_packages()
        .chain(previous.iter().map(String::as_str).filter(|package| {
            !catalog_packages.contains(package) || state.preserves_untouched_package(package)
        }))
    {
        if seen.insert(package.to_owned()) {
            configured.push(package.to_owned());
        }
    }

    let previous_set: BTreeSet<&str> = previous.iter().map(String::as_str).collect();
    let configured_set: BTreeSet<&str> = configured.iter().map(String::as_str).collect();
    let report = SaveReport {
        selected_tools: state.selected_tool_count(),
        added_packages: configured_set.difference(&previous_set).count(),
        removed_packages: previous_set.difference(&configured_set).count(),
        removed_legacy_tools: remove_legacy_managed_tools(doc),
        cleanup_warning: None,
    };

    if doc.get("sandbox").and_then(Item::as_table_like).is_none() {
        doc["sandbox"] = Item::Table(Table::new());
    }
    let array = Array::from_iter(configured.iter().map(String::as_str));
    doc["sandbox"]["extra_dnf_packages"] = Item::Value(toml_edit::Value::Array(array));
    report
}

fn remove_legacy_managed_tools(doc: &mut DocumentMut) -> usize {
    let Some(tools) = doc.get_mut("tool").and_then(Item::as_array_of_tables_mut) else {
        return 0;
    };
    let mut removed = 0;
    let mut index = 0;
    while index < tools.len() {
        let is_managed = tools
            .get(index)
            .and_then(|tool| tool.get(LEGACY_MANAGED_BY_KEY))
            .and_then(Item::as_str)
            == Some(LEGACY_MANAGED_BY_VALUE);
        if is_managed {
            tools.remove(index);
            removed += 1;
        } else {
            index += 1;
        }
    }
    removed
}

fn remove_legacy_managed_tools_from_file(path: &Path) -> Result<usize, ToolConfigError> {
    let content = fs::read_to_string(path)?;
    let mut doc = content
        .parse::<DocumentMut>()
        .map_err(|error| ToolConfigError::ConfigParse(error.to_string()))?;
    let removed = remove_legacy_managed_tools(&mut doc);
    if removed > 0 {
        backup_file(path)?;
        atomic_write(path, &doc.to_string())?;
    }
    Ok(removed)
}

fn validate_packages(packages: &[ToolPackage]) -> Result<(), ToolConfigError> {
    if packages.is_empty() {
        return Err(ToolConfigError::InvalidPackage(
            "JSON must contain at least one package group".to_owned(),
        ));
    }

    let mut claimed_packages = BTreeSet::new();
    for package in packages {
        if package.package.trim().is_empty() {
            return Err(ToolConfigError::InvalidPackage(
                "package group name must not be empty".to_owned(),
            ));
        }
        for tool in &package.tools {
            if tool.name.trim().is_empty() {
                return Err(ToolConfigError::InvalidPackage(format!(
                    "tool name in package group '{}' must not be empty",
                    package.package
                )));
            }
            if tool.dnf_packages.is_empty() {
                return Err(ToolConfigError::InvalidPackage(format!(
                    "tool '{}' in package group '{}' must define at least one dnf_packages entry",
                    tool.name, package.package
                )));
            }
            for dnf_package in &tool.dnf_packages {
                validate_dnf_package(&package.package, &tool.name, dnf_package)?;
                if !claimed_packages.insert(dnf_package.as_str()) {
                    return Err(ToolConfigError::InvalidPackage(format!(
                        "dnf package '{dnf_package}' is assigned to more than one tool"
                    )));
                }
            }
        }
    }

    Ok(())
}

fn validate_dnf_package(group: &str, tool: &str, package: &str) -> Result<(), ToolConfigError> {
    if !crate::config::is_valid_dnf_package_name(package) {
        return Err(ToolConfigError::InvalidPackage(format!(
            "dnf package for tool '{tool}' in group '{group}' must be a package name, not an option or shell expression"
        )));
    }
    Ok(())
}

fn backup_file(path: &Path) -> io::Result<PathBuf> {
    let backup = path.with_extension("toml.bak");
    match fs::copy(path, &backup) {
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    Ok(backup)
}

fn atomic_write(path: &Path, content: &str) -> io::Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(dir)?;
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    io::Write::write_all(&mut tmp, content.as_bytes())?;
    tmp.persist(path).map_err(|error| error.error)?;
    Ok(())
}
