pub mod model;
mod ui;

use std::path::Path;

pub fn run(config_path: &Path, packages_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let config = crate::config::parse_and_validate(config_path)
        .map_err(|error| model::ToolConfigError::Config(error.to_string()))?;
    let has_configured_tool_selection = model::config_file_defines_tool_selection(config_path)?;
    let mut app = ui::App::new(
        config_path,
        packages_path,
        has_configured_tool_selection.then_some(config.sandbox.extra_dnf_packages.as_slice()),
        has_configured_tool_selection.then_some(config.sandbox.tool_downloads.as_slice()),
        config.sandbox.enabled_agents.as_slice(),
        config.sandbox.agent_providers.as_slice(),
        config.update.minimum_release_age,
    )?;
    let report = app.run()?;

    if let Some(report) = report {
        println!(
            "Configured {} tools and {} agent CLIs in {} ({} image components added, {} removed; {} agents added, {} removed; {} legacy host mounts removed).",
            report.selected_tools,
            report.selected_agents,
            config_path.display(),
            report.added_components,
            report.removed_components,
            report.added_agents,
            report.removed_agents,
            report.removed_legacy_tools
        );
        if let Some(warning) = report.cleanup_warning {
            eprintln!("warning: {warning}");
        }
        println!(
            "Run `ags update-image --config {}` to apply the tool changes.",
            crate::util::shell_quote(&config_path.display().to_string())
        );
        println!(
            "Run `ags update-agents --config {}` to reconcile the selected agent CLIs.",
            crate::util::shell_quote(&config_path.display().to_string())
        );
    }

    Ok(())
}
