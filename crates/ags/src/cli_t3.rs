use super::CliError;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum T3Action {
    Status,
    Stop,
    Upgrade,
    Owner,
    Proxy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct T3Options {
    pub action: T3Action,
    pub repository: Option<PathBuf>,
    pub registration: Option<PathBuf>,
}

pub(super) fn parse_args(mut args: impl Iterator<Item = String>) -> Result<T3Options, CliError> {
    let command = args
        .next()
        .ok_or_else(|| CliError::UnexpectedPositional("expected t3 status|stop|upgrade".into()))?;
    let action = match command.as_str() {
        "status" => T3Action::Status,
        "stop" => T3Action::Stop,
        "upgrade" => T3Action::Upgrade,
        "_owner" => T3Action::Owner,
        "_proxy" => T3Action::Proxy,
        "-h" | "--help" => return Err(CliError::HelpRequested),
        _ => return Err(CliError::UnexpectedPositional(command)),
    };
    let mut options = T3Options {
        action,
        repository: None,
        registration: None,
    };
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--repository" => {
                options.repository =
                    Some(args.next().ok_or(CliError::MissingMountPathValue)?.into())
            }
            "--registration" if matches!(action, T3Action::Owner | T3Action::Proxy) => {
                options.registration =
                    Some(args.next().ok_or(CliError::MissingConfigValue)?.into());
            }
            "-h" | "--help" => return Err(CliError::HelpRequested),
            _ => return Err(CliError::UnexpectedFlag(arg)),
        }
    }
    if matches!(action, T3Action::Owner | T3Action::Proxy) && options.registration.is_none() {
        return Err(CliError::MissingConfigValue);
    }
    Ok(options)
}
