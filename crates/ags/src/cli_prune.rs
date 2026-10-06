use std::path::PathBuf;
use std::time::Duration;

use super::{CliError, required_value};
use crate::workspace_cache::PruneOptions;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PruneWorkspaceCachesOptions {
    pub config_path: Option<PathBuf>,
    pub dry_run: bool,
    pub grace_days: Option<u64>,
    pub max_caches: Option<usize>,
    pub max_deletions: Option<usize>,
    pub quiet: bool,
}

impl PruneWorkspaceCachesOptions {
    pub fn collection_options(&self) -> PruneOptions {
        let mut options = PruneOptions {
            dry_run: self.dry_run,
            ..Default::default()
        };
        if let Some(days) = self.grace_days {
            options.grace = Duration::from_secs(days * 86400);
        }
        if let Some(limit) = self.max_caches {
            options.max_caches = limit;
        }
        if let Some(limit) = self.max_deletions {
            options.max_deletions = limit;
        }
        options
    }
}

pub(super) fn parse_args(
    mut iter: impl Iterator<Item = String>,
) -> Result<PruneWorkspaceCachesOptions, CliError> {
    let mut options = PruneWorkspaceCachesOptions::default();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "-h" | "--help" => return Err(CliError::HelpRequested),
            "--dry-run" => {
                options.dry_run = true;
                continue;
            }
            "--quiet" => {
                options.quiet = true;
                continue;
            }
            _ => {}
        }
        let (flag, inline) = arg
            .split_once('=')
            .map_or((arg.as_str(), None), |(k, v)| (k, Some(v.to_owned())));
        if flag == "--config" {
            let value =
                required_value(inline.or_else(|| iter.next()), CliError::MissingConfigValue)?;
            options.config_path = Some(value.into());
            continue;
        }
        if matches!(flag, "--grace-days" | "--max-caches" | "--max-deletions") {
            let invalid = || CliError::InvalidPruneOption(flag.to_owned());
            let value = inline.or_else(|| iter.next()).ok_or_else(invalid)?;
            let number: u64 = value.parse().map_err(|_| invalid())?;
            match flag {
                "--grace-days" if number <= u64::MAX / 86400 => options.grace_days = Some(number),
                "--max-caches" if number > 0 => {
                    options.max_caches = Some(number.try_into().map_err(|_| invalid())?)
                }
                "--max-deletions" if number >= 3 => {
                    options.max_deletions = Some(number.try_into().map_err(|_| invalid())?)
                }
                _ => return Err(invalid()),
            }
            continue;
        }
        return Err(if arg.starts_with('-') {
            CliError::UnexpectedFlag(arg)
        } else {
            CliError::UnexpectedPositional(arg)
        });
    }
    Ok(options)
}
