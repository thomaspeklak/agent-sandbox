//! Budgeted unlinking of quarantined trees. No size calculation or full-tree walk.
use std::fs;
use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

/// Returns true when removed. A crash/budget exhaustion leaves resumable trash.
/// Never follows symlinks or descends into another filesystem.
pub(super) fn remove(path: &Path, budget: &mut usize, device: u64) -> io::Result<bool> {
    remove_inner(path, budget, device, true)
}

fn remove_inner(
    path: &Path,
    budget: &mut usize,
    device: u64,
    coordination: bool,
) -> io::Result<bool> {
    if *budget == 0 {
        return Ok(false);
    }
    let metadata = fs::symlink_metadata(path)?;
    if metadata.dev() != device {
        return Err(io::Error::other(
            "refusing to cross a filesystem during cache cleanup",
        ));
    }
    if metadata.is_dir() {
        for entry in fs::read_dir(path)? {
            let entry = entry?;
            // Keep coordination metadata until the entire tree is gone, so the
            // next cron run can authenticate/resume a partially deleted tree.
            if coordination
                && (entry.file_name() == "identity.json" || entry.file_name() == ".lease")
            {
                continue;
            }
            if !remove_inner(&entry.path(), budget, device, false)? {
                return Ok(false);
            }
        }
        let identity = path.join("identity.json");
        let lease = path.join(".lease");
        let reserved = if coordination {
            [&identity, &lease].iter().try_fold(1, |count, marker| {
                match fs::symlink_metadata(marker) {
                    Ok(_) => Ok(count + 1),
                    Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(count),
                    Err(e) => Err(e),
                }
            })?
        } else {
            1
        };
        if *budget < reserved {
            return Ok(false);
        }
        for marker in [&identity, &lease].into_iter().filter(|_| coordination) {
            match fs::remove_file(marker) {
                Ok(()) => *budget -= 1,
                Err(e) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => return Err(e),
            }
        }
        fs::remove_dir(path)?;
    } else {
        fs::remove_file(path)?;
    }
    *budget -= 1;
    Ok(true)
}
