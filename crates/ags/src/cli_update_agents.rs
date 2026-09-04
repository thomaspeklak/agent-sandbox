use super::{CliError, UpdateAgentsCliOptions, required_value};

pub(super) fn parse_args<I>(mut iter: I) -> Result<UpdateAgentsCliOptions, CliError>
where
    I: Iterator<Item = String>,
{
    let mut config_path = None;

    while let Some(arg) = iter.next() {
        if arg == "-h" || arg == "--help" {
            return Err(CliError::HelpRequested);
        }
        if arg == "--config" {
            config_path = Some(required_value(iter.next(), CliError::MissingConfigValue)?.into());
            continue;
        }
        if let Some(value) = arg.strip_prefix("--config=") {
            let value = required_value(Some(value), CliError::MissingConfigValue)?;
            config_path = Some(value.into());
            continue;
        }
        if arg.starts_with('-') {
            return Err(CliError::UnexpectedFlag(arg));
        }
        return Err(CliError::UnexpectedPositional(arg));
    }

    Ok(UpdateAgentsCliOptions { config_path })
}
