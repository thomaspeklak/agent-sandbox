//! Non-interactive maintenance: no overlays, config bootstrap, secrets, or network.
use std::io;

use crate::cli::PruneWorkspaceCachesOptions;

pub fn run(opts: &PruneWorkspaceCachesOptions) -> Result<(), Box<dyn std::error::Error>> {
    lower_priority()?;
    let path = opts
        .config_path
        .clone()
        .unwrap_or_else(crate::config::default_config_path);
    // Deliberately bypass lifecycle::load_config: cron must not prompt for repo
    // trust, create missing configuration, or depend on its working directory.
    let config = crate::config::parse_and_validate(&path)?;
    let report =
        crate::workspace_cache::prune(&config.sandbox.cache_dir, &opts.collection_options())?;
    if !opts.quiet {
        if report.busy {
            println!("workspace cache cleanup skipped: another launch or cleanup is busy");
        } else if opts.dry_run {
            for path in &report.eligible {
                println!("would prune {}", path.display());
            }
            println!(
                "{} orphaned; {} eligible (within this run's cache limit)",
                report.orphans,
                report.eligible.len()
            );
        } else {
            println!(
                "{} orphaned; {} removed; {} pending; {} filesystem deletions",
                report.orphans,
                report.removed.len(),
                report.pending.len(),
                report.deletions
            );
        }
    }
    Ok(())
}

fn lower_priority() -> io::Result<()> {
    // Only lower this maintenance process and its children, never a shared/host
    // setting. An unsupported priority policy fails before any cache mutation.
    if unsafe { libc::setpriority(libc::PRIO_PROCESS, 0, 19) } != 0 {
        return Err(io::Error::last_os_error());
    }
    #[cfg(target_os = "linux")]
    if unsafe { libc::syscall(libc::SYS_ioprio_set, 1, 0, 3 << 13) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
