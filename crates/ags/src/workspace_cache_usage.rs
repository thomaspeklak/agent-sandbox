//! One lightweight Podman inventory per prune, never a query per cache/file.
use std::fs;
use std::io::{self, Read};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::Deserialize;

#[derive(Deserialize)]
struct Container {
    #[serde(rename = "Mounts")]
    mounts: Vec<Mount>,
}

#[derive(Deserialize)]
struct Mount {
    #[serde(rename = "Source")]
    source: PathBuf,
}

pub(super) fn inspect() -> io::Result<Vec<PathBuf>> {
    // Bound the time launches can wait behind the inventory gate, including
    // all batches. A slow/broken Podman context is a skipped deletion, not a
    // reason to hold up foreground launches indefinitely.
    let deadline = Instant::now() + Duration::from_secs(5);
    let ids = output(&["ps", "--all", "--quiet", "--no-trunc"], deadline)?;
    let ids: Vec<_> = ids.split_whitespace().collect();
    let mut sources = Vec::new();
    for batch in ids.chunks(128) {
        let mut args = vec!["container", "inspect"];
        args.extend_from_slice(batch);
        let json = output(&args, deadline)?;
        let containers: Vec<Container> = serde_json::from_str(&json)?;
        if containers.len() != batch.len() {
            return Err(io::Error::other("incomplete Podman container inventory"));
        }
        sources.extend(
            containers
                .into_iter()
                .flat_map(|c| c.mounts)
                .filter_map(|m| m.source.is_absolute().then(|| canonical(&m.source))),
        );
    }
    Ok(sources)
}

fn output(args: &[&str], deadline: Instant) -> io::Result<String> {
    let mut child = Command::new("podman")
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0)
        .spawn()?;
    let stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take().expect("piped stderr");
    let read = |pipe: Box<dyn Read + Send>| {
        std::thread::spawn(move || {
            let mut bytes = Vec::new();
            pipe.take(16 * 1024 * 1024 + 1).read_to_end(&mut bytes)?;
            if bytes.len() > 16 * 1024 * 1024 {
                return Err(io::Error::other("Podman inventory output too large"));
            }
            Ok(bytes)
        })
    };
    let stdout = read(Box::new(stdout));
    let stderr = read(Box::new(stderr));
    let result = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Ok(status),
            Err(e) => break Err(e),
            Ok(None) if Instant::now() >= deadline => {
                break Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "Podman inventory exceeded five seconds",
                ));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
        }
    };
    // Also close pipe owners in descendants on timeout/error (and after exit).
    unsafe {
        libc::kill(-(child.id() as i32), libc::SIGKILL);
    }
    let _ = child.wait();
    let stdout = stdout
        .join()
        .map_err(|_| io::Error::other("Podman output reader failed"))?;
    let stderr = stderr
        .join()
        .map_err(|_| io::Error::other("Podman error reader failed"))?;
    let status = result?;
    let stdout = stdout?;
    let stderr = stderr?;
    if !status.success() {
        return Err(io::Error::other(format!(
            "podman {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&stderr)
        )));
    }
    String::from_utf8(stdout).map_err(io::Error::other)
}

pub(super) fn canonical(path: &Path) -> PathBuf {
    fs::canonicalize(path).unwrap_or_else(|_| path.to_owned())
}

pub(super) fn referenced(path: &Path, sources: &[PathBuf]) -> bool {
    let path = canonical(path);
    sources
        .iter()
        .any(|source| source.starts_with(&path) || path.starts_with(source))
}
