pub mod model;
mod ui;

use std::path::Path;

use model::PathToolResolver;

pub fn run(config_path: &Path, packages_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let mut app = ui::App::new(config_path, packages_path, &PathToolResolver)?;
    let report = app.run()?;

    if let Some(report) = report {
        println!(
            "Configured {} tools in {} (replaced {} managed entries).",
            report.added_tools,
            config_path.display(),
            report.removed_tools
        );
    }

    Ok(())
}
