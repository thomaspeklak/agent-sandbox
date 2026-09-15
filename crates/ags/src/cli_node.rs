use std::path::PathBuf;

use super::{CliError, required_value};

/// Node runtime management command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NodeCommand {
    Install { version: String },
    List,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeOptions {
    pub command: NodeCommand,
    pub config_path: Option<PathBuf>,
}

pub(super) fn parse_args<I>(mut iter: I) -> Result<NodeOptions, CliError>
where
    I: Iterator<Item = String>,
{
    let mut config_path = None;
    let mut positional = Vec::new();

    while let Some(arg) = iter.next() {
        if arg == "-h" || arg == "--help" {
            return Err(CliError::HelpRequested);
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
            if arg == "--version" {
                return Err(CliError::MissingNodeVersion);
            }
            return Err(CliError::UnexpectedFlag(arg));
        }
        positional.push(arg);
    }

    // `runtime node install ...` is accepted as a discoverable alias for the
    // Node-only command without adding a second runtime implementation.
    if positional.first().is_some_and(|value| value == "node") {
        positional.remove(0);
    }
    let action = positional.first().ok_or(CliError::MissingNodeCommand)?;
    match action.as_str() {
        "list" => {
            if positional.len() > 2 || positional.get(1).is_some_and(|value| value != "node") {
                return Err(CliError::UnexpectedPositional(positional[1].clone()));
            }
            Ok(NodeOptions {
                command: NodeCommand::List,
                config_path,
            })
        }
        "install" => {
            let version_index = if positional.get(1).is_some_and(|value| value == "node") {
                2
            } else {
                1
            };
            let version = positional
                .get(version_index)
                .ok_or(CliError::MissingNodeVersion)?;
            if positional.len() > version_index + 1 {
                return Err(CliError::UnexpectedPositional(
                    positional[version_index + 1].clone(),
                ));
            }
            Ok(NodeOptions {
                command: NodeCommand::Install {
                    version: version.clone(),
                },
                config_path,
            })
        }
        _ => Err(CliError::UnexpectedPositional(action.clone())),
    }
}
