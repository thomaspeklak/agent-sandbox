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
    pub environment: Option<String>,
    pub all: bool,
}

pub(super) fn parse_args(mut args: impl Iterator<Item = String>) -> Result<T3Options, CliError> {
    let command = args.next().ok_or(CliError::MissingT3Action)?;
    let action = match command.as_str() {
        "status" => T3Action::Status,
        "stop" => T3Action::Stop,
        "upgrade" => T3Action::Upgrade,
        "_owner" => T3Action::Owner,
        "_proxy" => T3Action::Proxy,
        "-h" | "--help" => return Err(CliError::T3HelpRequested(None)),
        _ => return Err(CliError::UnexpectedPositional(command)),
    };
    let mut options = T3Options {
        action,
        repository: None,
        registration: None,
        environment: None,
        all: false,
    };
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--repository" if !matches!(action, T3Action::Owner | T3Action::Proxy) => {
                if options.repository.is_some() {
                    return Err(CliError::ConflictingT3Selectors);
                }
                options.repository = Some(
                    super::required_value(
                        args.next().filter(|value| !value.starts_with('-')),
                        CliError::MissingRepositoryValue,
                    )?
                    .into(),
                )
            }
            "--all" if matches!(action, T3Action::Stop | T3Action::Upgrade) => {
                options.all = true;
            }
            "--registration" if matches!(action, T3Action::Owner | T3Action::Proxy) => {
                options.registration =
                    Some(args.next().ok_or(CliError::MissingConfigValue)?.into());
            }
            "-h" | "--help" => return Err(CliError::T3HelpRequested(Some(action))),
            value
                if !value.starts_with('-')
                    && !value.is_empty()
                    && !matches!(action, T3Action::Owner | T3Action::Proxy) =>
            {
                if options.environment.replace(value.to_owned()).is_some() {
                    return Err(CliError::ConflictingT3Selectors);
                }
            }
            _ => return Err(CliError::UnexpectedFlag(arg)),
        }
    }
    if matches!(action, T3Action::Owner | T3Action::Proxy) && options.registration.is_none() {
        return Err(CliError::MissingConfigValue);
    }
    if usize::from(options.repository.is_some())
        + usize::from(options.environment.is_some())
        + usize::from(options.all)
        > 1
    {
        return Err(CliError::ConflictingT3Selectors);
    }
    Ok(options)
}
