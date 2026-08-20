use super::{CliError, UpdateImageOptions};

pub(super) fn parse_args<I>(mut iter: I) -> Result<UpdateImageOptions, CliError>
where
    I: Iterator<Item = String>,
{
    let mut keep_existing = false;
    let mut config_path = None;

    while let Some(arg) = iter.next() {
        if arg == "-h" || arg == "--help" {
            return Err(CliError::HelpRequested);
        }
        if arg == "--keep-existing" {
            keep_existing = true;
            continue;
        }
        if arg == "--config" {
            config_path = Some(iter.next().ok_or(CliError::MissingConfigValue)?.into());
            continue;
        }
        if let Some(value) = arg.strip_prefix("--config=") {
            if value.is_empty() {
                return Err(CliError::MissingConfigValue);
            }
            config_path = Some(value.into());
            continue;
        }
        if arg.starts_with('-') {
            return Err(CliError::UnexpectedFlag(arg));
        }
        return Err(CliError::UnexpectedPositional(arg));
    }

    Ok(UpdateImageOptions {
        keep_existing,
        config_path,
    })
}
