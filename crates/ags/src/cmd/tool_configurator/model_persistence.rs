use std::collections::BTreeSet;
use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

use sha2::{Digest, Sha256};
use toml_edit::{Array, DocumentMut, Item, Table};

use crate::config::{
    BASE_DNF_PACKAGES, DEFAULT_EXTRA_DNF_PACKAGES, validate_locked_tool_downloads,
};

use super::model_validation::validate_catalog;
use super::{
    LEGACY_MANAGED_BY_KEY, LEGACY_MANAGED_BY_VALUE, SaveReport, ToolCatalog, ToolConfigError,
    ToolSelectionState,
};

pub fn load_package_file(path: &Path) -> Result<ToolCatalog, ToolConfigError> {
    let content = fs::read_to_string(path)?;
    let catalog = serde_json::from_str::<ToolCatalog>(&content)?;
    validate_catalog(&catalog)?;
    Ok(catalog)
}

pub fn config_file_defines_tool_selection(path: &Path) -> Result<bool, ToolConfigError> {
    let content = fs::read_to_string(path)?;
    let doc = content
        .parse::<DocumentMut>()
        .map_err(|error| ToolConfigError::ConfigParse(error.to_string()))?;
    Ok(doc.get("sandbox").is_some_and(|sandbox| {
        sandbox.get("extra_dnf_packages").is_some()
            || sandbox.get("tool_download_lock").is_some()
            || sandbox.get("enabled_agents").is_some()
    }))
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
    let previous_lock_name = configured_lock_name(&doc);
    let backup_path = config_path.with_extension("toml.bak");
    let displaced_backup_lock_name = configured_lock_name_from_file(&backup_path);

    let mut report = apply_selection_to_document(&mut doc, state);
    let downloads = state.selected_downloads();
    validate_locked_tool_downloads(&downloads, "selected tool downloads")
        .map_err(ToolConfigError::InvalidPackage)?;
    let previous_download_ids = state
        .configured_downloads
        .iter()
        .map(|tool| tool.id.as_str())
        .collect::<BTreeSet<_>>();
    let download_ids = downloads
        .iter()
        .map(|tool| tool.id.as_str())
        .collect::<BTreeSet<_>>();
    report.added_components += download_ids.difference(&previous_download_ids).count();
    report.removed_components += previous_download_ids.difference(&download_ids).count();

    let lock_content = serde_json::to_string_pretty(&downloads)? + "\n";
    let lock_digest = format!("{:x}", Sha256::digest(lock_content.as_bytes()));
    let lock_name = format!("tool-downloads.{lock_digest}.lock.json");
    let lock_path = config_path.with_file_name(&lock_name);
    doc["sandbox"]["tool_download_lock"] = toml_edit::value(lock_name.clone());
    backup_file(config_path)?;
    atomic_write(&lock_path, &lock_content)?;
    atomic_write(config_path, &doc.to_string())?;
    if let Some(stale_lock_name) = displaced_backup_lock_name
        .filter(|name| name != &lock_name && previous_lock_name.as_deref() != Some(name.as_str()))
    {
        match remove_managed_lock(config_path, &stale_lock_name) {
            Ok(()) => {}
            Err(error) => {
                add_cleanup_warning(
                    &mut report,
                    format!(
                        "saved tool selection, but could not remove stale tool download lock {stale_lock_name}: {error}"
                    ),
                );
            }
        }
    }
    if let Some(path) = legacy_cleanup_path {
        match remove_legacy_managed_tools_from_file(path) {
            Ok(removed) => report.removed_legacy_tools += removed,
            Err(error) => {
                add_cleanup_warning(
                    &mut report,
                    format!(
                        "saved tool selection, but could not remove legacy tool mounts from {}: {error}",
                        path.display()
                    ),
                );
            }
        }
    }
    Ok(report)
}

fn configured_lock_name(doc: &DocumentMut) -> Option<String> {
    doc.get("sandbox")
        .and_then(|sandbox| sandbox.get("tool_download_lock"))
        .and_then(Item::as_str)
        .map(str::to_owned)
}

fn configured_lock_name_from_file(path: &Path) -> Option<String> {
    let content = fs::read_to_string(path).ok()?;
    let doc = content.parse::<DocumentMut>().ok()?;
    configured_lock_name(&doc)
}

fn managed_lock_path(config_path: &Path, lock_name: &str) -> Option<(PathBuf, String)> {
    let mut components = Path::new(lock_name).components();
    let Component::Normal(file_name) = components.next()? else {
        return None;
    };
    if components.next().is_some() {
        return None;
    }
    let file_name = file_name.to_str()?;
    let digest = file_name
        .strip_prefix("tool-downloads.")?
        .strip_suffix(".lock.json")?;
    if digest.len() != 64
        || !digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return None;
    }
    Some((config_path.with_file_name(file_name), digest.to_owned()))
}

fn remove_managed_lock(config_path: &Path, lock_name: &str) -> io::Result<()> {
    let Some((lock_path, expected_digest)) = managed_lock_path(config_path, lock_name) else {
        return Ok(());
    };
    let metadata = match fs::symlink_metadata(&lock_path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
        return Ok(());
    }
    let content = fs::read(&lock_path)?;
    let actual_digest = format!("{:x}", Sha256::digest(&content));
    if actual_digest != expected_digest {
        return Ok(());
    }
    fs::remove_file(lock_path)
}

fn add_cleanup_warning(report: &mut SaveReport, warning: String) {
    match &mut report.cleanup_warning {
        Some(existing) => {
            existing.push_str("; ");
            existing.push_str(&warning);
        }
        None => report.cleanup_warning = Some(warning),
    }
}

pub fn configured_packages_from_document(doc: &DocumentMut) -> Vec<String> {
    configured_packages_if_present(doc).unwrap_or_else(|| {
        DEFAULT_EXTRA_DNF_PACKAGES
            .iter()
            .map(|package| (*package).to_owned())
            .collect()
    })
}

fn configured_packages_if_present(doc: &DocumentMut) -> Option<Vec<String>> {
    doc.get("sandbox")
        .and_then(|sandbox| sandbox.get("extra_dnf_packages"))
        .and_then(Item::as_array)
        .map(|packages| {
            packages
                .iter()
                .filter_map(|package| package.as_str().map(str::to_owned))
                .collect()
        })
}

pub fn apply_selection_to_document(
    doc: &mut DocumentMut,
    state: &ToolSelectionState,
) -> SaveReport {
    let previous = configured_packages_if_present(doc)
        .or_else(|| state.configured_packages.clone())
        .unwrap_or_else(|| state.default_packages());
    let catalog_packages = state.catalog_packages();
    let baseline_packages: BTreeSet<&str> = BASE_DNF_PACKAGES.iter().copied().collect();
    let mut seen = BTreeSet::new();
    let mut configured = Vec::new();

    for package in state
        .selected_packages()
        .chain(previous.iter().map(String::as_str).filter(|package| {
            !baseline_packages.contains(package)
                && (!catalog_packages.contains(package)
                    || state.preserves_untouched_package(package))
        }))
    {
        if seen.insert(package.to_owned()) {
            configured.push(package.to_owned());
        }
    }

    let previous_set: BTreeSet<&str> = previous
        .iter()
        .map(String::as_str)
        .filter(|package| !baseline_packages.contains(package))
        .collect();
    let configured_set: BTreeSet<&str> = configured.iter().map(String::as_str).collect();
    let previous_agents = state
        .configured_agents
        .iter()
        .copied()
        .collect::<BTreeSet<_>>();
    let selected_agents = state.selected_agents();
    let selected_agent_set = selected_agents.iter().copied().collect::<BTreeSet<_>>();
    let report = SaveReport {
        selected_tools: state.selected_tool_count(),
        selected_agents: selected_agents.len(),
        added_components: configured_set.difference(&previous_set).count(),
        removed_components: previous_set.difference(&configured_set).count(),
        added_agents: selected_agent_set.difference(&previous_agents).count(),
        removed_agents: previous_agents.difference(&selected_agent_set).count(),
        removed_legacy_tools: remove_legacy_managed_tools(doc),
        cleanup_warning: None,
    };

    if doc.get("sandbox").and_then(Item::as_table_like).is_none() {
        doc["sandbox"] = Item::Table(Table::new());
    }
    let array = Array::from_iter(configured.iter().map(String::as_str));
    doc["sandbox"]["extra_dnf_packages"] = Item::Value(toml_edit::Value::Array(array));
    let agents = Array::from_iter(selected_agents.iter().map(|agent| agent.as_str()));
    doc["sandbox"]["enabled_agents"] = Item::Value(toml_edit::Value::Array(agents));
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
