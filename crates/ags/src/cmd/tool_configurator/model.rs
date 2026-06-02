use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::Deserialize;
use toml_edit::{ArrayOfTables, DocumentMut, InlineTable, Item, Table, Value};

use super::install::{
    InstallCommand, InstallDefinition, ToolInstaller, find_on_path, validate_install_definition,
};

pub const MANAGED_BY_KEY: &str = "ags_managed_by";
pub const MANAGED_BY_VALUE: &str = "tool-configurator";
pub const MANAGED_PACKAGE_KEY: &str = "ags_package";

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
    #[serde(default)]
    pub secrets: BTreeMap<String, SecretInput>,
    #[serde(default)]
    pub install: InstallDefinition,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum SecretInput {
    Description(String),
    Spec(SecretDefinition),
}

#[derive(Debug, Clone, Deserialize)]
pub struct SecretDefinition {
    #[serde(default)]
    pub description: String,
    #[serde(default = "default_required")]
    pub required: bool,
    #[serde(default)]
    pub from_env: Option<String>,
    #[serde(default)]
    pub secret_store: Option<BTreeMap<String, String>>,
}

fn default_required() -> bool {
    true
}

impl SecretInput {
    pub fn normalized(&self, env: &str) -> SecretDefinition {
        let mut spec = match self {
            Self::Description(description) => SecretDefinition {
                description: description.clone(),
                required: true,
                from_env: None,
                secret_store: None,
            },
            Self::Spec(spec) => spec.clone(),
        };

        let has_from_env = spec
            .from_env
            .as_deref()
            .is_some_and(|value| !value.trim().is_empty());
        let has_secret_store = spec
            .secret_store
            .as_ref()
            .is_some_and(|store| !store.is_empty());

        if !has_from_env && !has_secret_store {
            spec.from_env = Some(env.to_owned());
        }

        spec
    }
}

pub trait ToolResolver {
    fn resolve_tool(&self, name: &str) -> Option<PathBuf>;
}

#[derive(Debug, Clone, Copy)]
pub struct PathToolResolver;

impl ToolResolver for PathToolResolver {
    fn resolve_tool(&self, name: &str) -> Option<PathBuf> {
        find_on_path(name)
    }
}

#[derive(Debug, Clone)]
pub struct ToolState {
    pub definition: ToolDefinition,
    pub host_path: Option<PathBuf>,
    pub selected: bool,
}

impl ToolState {
    pub fn available(&self) -> bool {
        self.host_path.is_some()
    }

    pub fn install_command(&self, installer: ToolInstaller) -> Option<InstallCommand> {
        if self.available() {
            return None;
        }

        let package = self.definition.install.package_for(installer.manager)?;
        Some(installer.command_for(package))
    }
}

#[derive(Debug, Clone)]
pub struct PackageState {
    pub package: String,
    pub tools: Vec<ToolState>,
}

impl PackageState {
    pub fn available_count(&self) -> usize {
        self.tools.iter().filter(|tool| tool.available()).count()
    }

    pub fn selected_count(&self) -> usize {
        self.tools
            .iter()
            .filter(|tool| tool.available() && tool.selected)
            .count()
    }

    pub fn missing_count(&self) -> usize {
        self.tools.iter().filter(|tool| !tool.available()).count()
    }

    pub fn all_available_selected(&self) -> bool {
        let available = self.available_count();
        available > 0 && self.selected_count() == available
    }
}

#[derive(Debug, Clone)]
pub struct ToolSelectionState {
    pub packages: Vec<PackageState>,
}

impl ToolSelectionState {
    pub fn from_packages(
        packages: Vec<ToolPackage>,
        resolver: &dyn ToolResolver,
        installer: Option<ToolInstaller>,
    ) -> Result<Self, ToolConfigError> {
        validate_packages(&packages)?;

        let packages = packages
            .into_iter()
            .map(|package| {
                let tools = package
                    .tools
                    .into_iter()
                    .map(|definition| {
                        let host_path = resolve_tool_path(&definition, resolver, installer);
                        let selected = host_path.is_some();
                        ToolState {
                            definition,
                            host_path,
                            selected,
                        }
                    })
                    .collect();

                PackageState {
                    package: package.package,
                    tools,
                }
            })
            .collect();

        Ok(Self { packages })
    }

    pub fn selected_tool_count(&self) -> usize {
        self.packages
            .iter()
            .flat_map(|package| package.tools.iter())
            .filter(|tool| tool.available() && tool.selected)
            .count()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SaveReport {
    pub added_tools: usize,
    pub removed_tools: usize,
}

pub fn load_package_file(path: &Path) -> Result<Vec<ToolPackage>, ToolConfigError> {
    let content = fs::read_to_string(path)?;
    let packages = serde_json::from_str::<Vec<ToolPackage>>(&content)?;
    validate_packages(&packages)?;
    Ok(packages)
}

pub fn write_selected_tools(
    config_path: &Path,
    state: &ToolSelectionState,
) -> Result<SaveReport, ToolConfigError> {
    let content = fs::read_to_string(config_path)?;
    let mut doc: DocumentMut = content
        .parse::<DocumentMut>()
        .map_err(|error| ToolConfigError::ConfigParse(error.to_string()))?;

    let report = apply_selection_to_document(&mut doc, state);
    backup_file(config_path)?;
    atomic_write(config_path, &doc.to_string())?;
    Ok(report)
}

pub fn apply_selection_to_document(
    doc: &mut DocumentMut,
    state: &ToolSelectionState,
) -> SaveReport {
    let removed_tools = remove_managed_tools(doc);
    let added_tools = append_selected_tools(doc, state);
    SaveReport {
        added_tools,
        removed_tools,
    }
}

pub fn container_path_for_tool(name: &str) -> String {
    format!("/usr/local/bin/{name}")
}

fn validate_packages(packages: &[ToolPackage]) -> Result<(), ToolConfigError> {
    if packages.is_empty() {
        return Err(ToolConfigError::InvalidPackage(
            "JSON must contain at least one package".to_owned(),
        ));
    }

    for package in packages {
        if package.package.trim().is_empty() {
            return Err(ToolConfigError::InvalidPackage(
                "package name must not be empty".to_owned(),
            ));
        }
        for tool in &package.tools {
            validate_tool_name(&package.package, &tool.name)?;
            validate_install_definition(&package.package, &tool.name, &tool.install)?;
        }
    }

    Ok(())
}

fn validate_tool_name(package: &str, name: &str) -> Result<(), ToolConfigError> {
    if name.trim().is_empty() {
        return Err(ToolConfigError::InvalidPackage(format!(
            "tool name in package '{package}' must not be empty"
        )));
    }

    if name.contains('/') || name.contains('\\') || name.chars().any(char::is_whitespace) {
        return Err(ToolConfigError::InvalidPackage(format!(
            "tool '{name}' in package '{package}' must be a command name, not a path or shell expression"
        )));
    }

    Ok(())
}

pub fn resolve_tool_path(
    definition: &ToolDefinition,
    resolver: &dyn ToolResolver,
    installer: Option<ToolInstaller>,
) -> Option<PathBuf> {
    if let Some(binary) =
        installer.and_then(|installer| definition.install.binary_for(installer.manager))
        && let Some(path) = resolver.resolve_tool(binary)
    {
        return Some(path);
    }

    resolver.resolve_tool(&definition.name)
}

fn remove_managed_tools(doc: &mut DocumentMut) -> usize {
    let Some(tools) = doc["tool"].as_array_of_tables_mut() else {
        return 0;
    };

    let mut removed = 0;
    let mut index = 0;
    while index < tools.len() {
        let is_managed = tools
            .get(index)
            .and_then(|tool| tool.get(MANAGED_BY_KEY))
            .and_then(|item| item.as_str())
            == Some(MANAGED_BY_VALUE);

        if is_managed {
            tools.remove(index);
            removed += 1;
        } else {
            index += 1;
        }
    }

    removed
}

fn append_selected_tools(doc: &mut DocumentMut, state: &ToolSelectionState) -> usize {
    if doc.get("tool").is_none() || doc["tool"].as_array_of_tables().is_none() {
        doc["tool"] = Item::ArrayOfTables(ArrayOfTables::new());
    }

    let Some(tools) = doc["tool"].as_array_of_tables_mut() else {
        return 0;
    };

    let mut added = 0;
    for package in &state.packages {
        for tool in &package.tools {
            if !tool.available() || !tool.selected {
                continue;
            }
            tools.push(tool_table(&package.package, tool));
            added += 1;
        }
    }

    added
}

fn tool_table(package: &str, tool: &ToolState) -> Table {
    let mut table = Table::new();
    table["name"] = toml_edit::value(tool.definition.name.as_str());
    table["path"] = toml_edit::value(
        tool.host_path
            .as_ref()
            .expect("selected tools must be available")
            .to_string_lossy()
            .as_ref(),
    );
    table["container_path"] = toml_edit::value(container_path_for_tool(&tool.definition.name));
    table["mode"] = toml_edit::value("ro");
    table["when"] = toml_edit::value("always");
    table["optional"] = toml_edit::value(false);
    table[MANAGED_BY_KEY] = toml_edit::value(MANAGED_BY_VALUE);
    table[MANAGED_PACKAGE_KEY] = toml_edit::value(package);

    if !tool.definition.description.trim().is_empty() {
        table["description"] = toml_edit::value(tool.definition.description.as_str());
    }

    let secrets = secret_tables(&tool.definition.secrets);
    if !secrets.is_empty() {
        table["secret"] = Item::ArrayOfTables(secrets);
    }

    table
}

fn secret_tables(secrets: &BTreeMap<String, SecretInput>) -> ArrayOfTables {
    let mut tables = ArrayOfTables::new();
    for (env, input) in secrets {
        let spec = input.normalized(env);
        let mut table = Table::new();
        table["env"] = toml_edit::value(env.as_str());

        if let Some(from_env) = spec
            .from_env
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            table["from_env"] = toml_edit::value(from_env);
        }

        if let Some(store) = spec.secret_store.as_ref().filter(|store| !store.is_empty()) {
            let mut inline = InlineTable::new();
            for (key, value) in store {
                inline.insert(key, Value::from(value.as_str()));
            }
            table["secret_store"] = Item::Value(Value::InlineTable(inline));
        }

        tables.push(table);
    }

    tables
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
