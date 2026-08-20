use super::{AliasMode, CliError, CompletionsOptions, CreateAliasesOptions, InstallOptions, Shell};

pub(super) fn parse_install_args<I>(iter: I) -> Result<InstallOptions, CliError>
where
    I: Iterator<Item = String>,
{
    let mut link_self = false;
    let mut force = false;
    let mut add_agent_mounts = false;

    for arg in iter {
        if arg == "-h" || arg == "--help" {
            return Err(CliError::HelpRequested);
        }
        if arg == "--link-self" {
            link_self = true;
            continue;
        }
        if arg == "--force" {
            force = true;
            continue;
        }
        if arg == "--add-agent-mounts" {
            add_agent_mounts = true;
            continue;
        }
        if arg.starts_with('-') {
            return Err(CliError::UnexpectedFlag(arg));
        }
        return Err(CliError::UnexpectedPositional(arg));
    }

    Ok(InstallOptions {
        link_self,
        force,
        add_agent_mounts,
    })
}

pub(super) fn parse_create_aliases_args<I>(mut iter: I) -> Result<CreateAliasesOptions, CliError>
where
    I: Iterator<Item = String>,
{
    let mut shell = None;
    let mut mode = AliasMode::Wrappers;
    let mut force = false;

    while let Some(arg) = iter.next() {
        if arg == "-h" || arg == "--help" {
            return Err(CliError::HelpRequested);
        }
        if arg == "--force" {
            force = true;
            continue;
        }
        if arg == "--shell" {
            let value = iter.next().ok_or(CliError::MissingShellValue)?;
            shell = Some(Shell::parse(&value)?);
            continue;
        }
        if let Some(value) = arg.strip_prefix("--shell=") {
            if value.is_empty() {
                return Err(CliError::MissingShellValue);
            }
            shell = Some(Shell::parse(value)?);
            continue;
        }
        if arg == "--mode" {
            let value = iter.next().ok_or(CliError::MissingAliasModeValue)?;
            mode = AliasMode::parse(&value)?;
            continue;
        }
        if let Some(value) = arg.strip_prefix("--mode=") {
            if value.is_empty() {
                return Err(CliError::MissingAliasModeValue);
            }
            mode = AliasMode::parse(value)?;
            continue;
        }

        if arg.starts_with('-') {
            return Err(CliError::UnexpectedFlag(arg));
        }
        return Err(CliError::UnexpectedPositional(arg));
    }

    Ok(CreateAliasesOptions { shell, mode, force })
}

pub(super) fn parse_completions_args<I>(mut iter: I) -> Result<CompletionsOptions, CliError>
where
    I: Iterator<Item = String>,
{
    let mut shell = None;

    while let Some(arg) = iter.next() {
        if arg == "-h" || arg == "--help" {
            return Err(CliError::HelpRequested);
        }
        if arg == "--shell" {
            let value = iter.next().ok_or(CliError::MissingShellValue)?;
            shell = Some(Shell::parse(&value)?);
            continue;
        }
        if let Some(value) = arg.strip_prefix("--shell=") {
            if value.is_empty() {
                return Err(CliError::MissingShellValue);
            }
            shell = Some(Shell::parse(value)?);
            continue;
        }
        if arg.starts_with('-') {
            return Err(CliError::UnexpectedFlag(arg));
        }
        return Err(CliError::UnexpectedPositional(arg));
    }

    let shell = shell.ok_or(CliError::MissingShellValue)?;
    Ok(CompletionsOptions { shell })
}
