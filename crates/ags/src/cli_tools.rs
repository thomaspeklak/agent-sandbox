use std::path::PathBuf;

use super::{CliError, required_value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ToolConfigOptions {
    pub packages_path: PathBuf,
    pub config_path: Option<PathBuf>,
}

pub(super) fn parse_tools_args<I>(mut iter: I) -> Result<ToolConfigOptions, CliError>
where
    I: Iterator<Item = String>,
{
    let mut packages_path = None;
    let mut config_path = None;

    while let Some(arg) = iter.next() {
        if arg == "-h" || arg == "--help" {
            return Err(CliError::HelpRequested);
        }
        if arg == "--packages" {
            let value = required_value(iter.next(), CliError::MissingToolPackagesValue)?;
            packages_path = Some(PathBuf::from(value));
            continue;
        }
        if let Some(value) = arg.strip_prefix("--packages=") {
            let value = required_value(Some(value), CliError::MissingToolPackagesValue)?;
            packages_path = Some(PathBuf::from(value));
            continue;
        }
        if arg == "--config" {
            let value = required_value(iter.next(), CliError::MissingConfigValue)?;
            config_path = Some(PathBuf::from(value));
            continue;
        }
        if let Some(value) = arg.strip_prefix("--config=") {
            let value = required_value(Some(value), CliError::MissingConfigValue)?;
            config_path = Some(PathBuf::from(value));
            continue;
        }
        if arg.starts_with('-') {
            return Err(CliError::UnexpectedFlag(arg));
        }
        if packages_path.is_some() {
            return Err(CliError::UnexpectedPositional(arg));
        }
        packages_path = Some(PathBuf::from(arg));
    }

    let packages_path = packages_path.ok_or(CliError::MissingToolPackagesPath)?;
    Ok(ToolConfigOptions {
        packages_path,
        config_path,
    })
}
