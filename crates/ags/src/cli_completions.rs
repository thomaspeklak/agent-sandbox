use crate::cli::{CliError, Shell};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionsOptions {
    pub shell: Shell,
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
