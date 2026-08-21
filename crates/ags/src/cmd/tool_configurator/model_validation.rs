fn validate_catalog(catalog: &ToolCatalog) -> Result<(), ToolConfigError> {
    if catalog.tools.is_empty() {
        return Err(invalid("catalog must define at least one tool"));
    }
    let baseline_packages: BTreeSet<&str> = BASE_DNF_PACKAGES.iter().copied().collect();
    let mut tool_ids = BTreeSet::new();
    let mut claimed_packages = BTreeSet::new();
    let mut claimed_commands = BTreeSet::new();
    for tool in &catalog.tools {
        if !crate::config::valid_tool_id(&tool.id) {
            return Err(invalid(format!(
                "tool id '{}' must use lower-kebab-case",
                tool.id
            )));
        }
        if !tool_ids.insert(tool.id.as_str()) {
            return Err(invalid(format!(
                "tool id '{}' is defined more than once",
                tool.id
            )));
        }
        if tool.name.trim().is_empty() || tool.description.trim().is_empty() {
            return Err(invalid(format!(
                "tool '{}' must define a name and purpose-focused description",
                tool.id
            )));
        }
        let has_dnf_packages = !tool.dnf_packages.is_empty();
        let has_download = tool.download.is_some();
        if has_dnf_packages == has_download {
            return Err(invalid(format!(
                "tool '{}' must define exactly one of dnf_packages or download",
                tool.id
            )));
        }
        for package in &tool.dnf_packages {
            validate_dnf_package(&tool.id, package)?;
            if baseline_packages.contains(package.as_str()) {
                return Err(invalid(format!(
                    "tool '{}' claims fixed baseline package '{package}'",
                    tool.id
                )));
            }
            if !claimed_packages.insert(package.as_str()) {
                return Err(invalid(format!(
                    "dnf package '{package}' is assigned to more than one tool"
                )));
            }
        }
        if let Some(download) = &tool.download {
            crate::config::validate_tool_download_source(
                download,
                &format!("tool '{}'.download", tool.id),
            )
            .map_err(invalid)?;
            if !claimed_commands.insert(download.install_as.as_str()) {
                return Err(invalid(format!(
                    "downloaded command '{}' is assigned to more than one tool",
                    download.install_as
                )));
            }
        }
    }

    if catalog.groups.len() != PROFESSIONS.len() {
        return Err(invalid(format!(
            "catalog must define exactly {} profession groups",
            PROFESSIONS.len()
        )));
    }
    let mut referenced_tools = BTreeSet::new();
    for (group, (expected_id, expected_name)) in catalog.groups.iter().zip(PROFESSIONS) {
        if group.id != *expected_id {
            return Err(invalid(format!(
                "profession group '{}' must be '{}'",
                group.id, expected_id
            )));
        }
        if group.name != *expected_name {
            return Err(invalid(format!(
                "profession group '{}' must be named '{}'",
                group.id, expected_name
            )));
        }
        if group.subcategories.is_empty() {
            return Err(invalid(format!(
                "profession group '{}' must define a name and subcategories",
                group.id
            )));
        }
        let mut subcategory_names = BTreeSet::new();
        for subcategory in &group.subcategories {
            if subcategory.name.trim().is_empty() || subcategory.tools.is_empty() {
                return Err(invalid(format!(
                    "profession group '{}' contains an empty subcategory",
                    group.id
                )));
            }
            if !subcategory_names.insert(subcategory.name.as_str()) {
                return Err(invalid(format!(
                    "subcategory '{}' is repeated in profession group '{}'",
                    subcategory.name, group.id
                )));
            }
            let mut subcategory_tools = BTreeSet::new();
            for tool_id in &subcategory.tools {
                if !tool_ids.contains(tool_id.as_str()) {
                    return Err(invalid(format!(
                        "subcategory '{}' references unknown tool '{tool_id}'",
                        subcategory.name
                    )));
                }
                if !subcategory_tools.insert(tool_id.as_str()) {
                    return Err(invalid(format!(
                        "subcategory '{}' references tool '{tool_id}' more than once",
                        subcategory.name
                    )));
                }
                referenced_tools.insert(tool_id.as_str());
            }
        }
    }

    if let Some(tool_id) = tool_ids.difference(&referenced_tools).next() {
        return Err(invalid(format!(
            "tool '{tool_id}' is not assigned to a profession subcategory"
        )));
    }
    Ok(())
}

fn validate_dnf_package(tool: &str, package: &str) -> Result<(), ToolConfigError> {
    if !crate::config::is_valid_dnf_package_name(package) {
        return Err(invalid(format!(
            "dnf package for tool '{tool}' must be a package name, not an option or shell expression"
        )));
    }
    Ok(())
}

fn invalid(message: impl Into<String>) -> ToolConfigError {
    ToolConfigError::InvalidPackage(message.into())
}
