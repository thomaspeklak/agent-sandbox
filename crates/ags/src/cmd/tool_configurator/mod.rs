pub mod model;
mod ui;

use std::path::Path;

pub fn run(
    config_path: &Path,
    overlay_path: Option<&Path>,
    packages_path: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let config = crate::config::parse_and_validate_with_overlay(config_path, overlay_path)
        .map_err(|error| model::ToolConfigError::Config(error.to_string()))?;
    let base_defines_selection = model::config_file_defines_tool_selection(config_path)?;
    let overlay_defines_selection = overlay_path
        .map(model::config_file_defines_tool_selection)
        .transpose()?
        .unwrap_or(false);
    let target_path = match overlay_path {
        Some(path) if overlay_defines_selection => path,
        _ => config_path,
    };
    let cleanup_path = overlay_path
        .filter(|path| *path != target_path)
        .or_else(|| (target_path != config_path).then_some(config_path));
    let mut app = ui::App::new(
        target_path,
        cleanup_path,
        packages_path,
        (base_defines_selection || overlay_defines_selection)
            .then_some(config.sandbox.extra_dnf_packages.as_slice()),
        (base_defines_selection || overlay_defines_selection)
            .then_some(config.sandbox.tool_downloads.as_slice()),
    )?;
    let report = app.run()?;

    if let Some(report) = report {
        println!(
            "Configured {} tools in {} ({} image components added, {} removed, {} legacy host mounts removed).",
            report.selected_tools,
            target_path.display(),
            report.added_components,
            report.removed_components,
            report.removed_legacy_tools
        );
        if let Some(warning) = report.cleanup_warning {
            eprintln!("warning: {warning}");
        }
        println!(
            "Run `ags update-image --config {}` to apply the tool changes.",
            crate::util::shell_quote(&config_path.display().to_string())
        );
    }

    Ok(())
}
