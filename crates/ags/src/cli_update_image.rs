use super::{CliError, UpdateImageOptions};

pub(super) fn parse_args<I>(iter: I) -> Result<UpdateImageOptions, CliError>
where
    I: Iterator<Item = String>,
{
    let mut keep_existing = false;

    for arg in iter {
        if arg == "-h" || arg == "--help" {
            return Err(CliError::HelpRequested);
        }
        if arg == "--keep-existing" {
            keep_existing = true;
            continue;
        }
        if arg.starts_with('-') {
            return Err(CliError::UnexpectedFlag(arg));
        }
        return Err(CliError::UnexpectedPositional(arg));
    }

    Ok(UpdateImageOptions { keep_existing })
}
