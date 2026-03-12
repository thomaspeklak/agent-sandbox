use std::fs;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

const REPO: &str = "thomaspeklak/agent-sandbox";
const CHECK_INTERVAL_SECS: u64 = 24 * 60 * 60;
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Cached update-check result read at startup.
pub struct UpdateCheck {
    pub latest_version: Option<String>,
}

impl UpdateCheck {
    /// Read the cache and spawn a background refresh if stale.
    /// Returns immediately — never blocks on network.
    pub fn start(cache_dir: &Path) -> Self {
        let cache_path = cache_dir.join("update-check");
        let (latest_version, stale) = read_cache(&cache_path);

        if stale {
            let path = cache_path.clone();
            std::thread::spawn(move || {
                if let Some(tag) = fetch_latest_tag() {
                    write_cache(&path, &tag);
                }
            });
        }

        UpdateCheck { latest_version }
    }

    /// Read from a known cache directory without spawning a refresh.
    /// Use this when you don't have cache_dir until after config is loaded.
    pub fn from_default_cache() -> Self {
        let cache_dir = dirs::cache_dir()
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join("ags");
        Self::start(&cache_dir)
    }

    /// Print an update notice to stderr if a newer version is available.
    /// Does nothing if stdout is not a terminal.
    pub fn notify_if_available(&self) {
        if !std::io::stderr().is_terminal() {
            return;
        }
        let Some(latest) = &self.latest_version else {
            return;
        };
        if !is_newer(latest, CURRENT_VERSION) {
            return;
        }
        eprintln!(
            "\n\x1b[2mA new release of ags is available: v{CURRENT_VERSION} \u{2192} v{latest}\n\
             https://github.com/{REPO}/releases/tag/v{latest}\x1b[0m"
        );
    }
}

fn read_cache(path: &Path) -> (Option<String>, bool) {
    let contents = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(_) => return (None, true),
    };
    let mut lines = contents.lines();
    let timestamp: u64 = match lines.next().and_then(|l| l.parse().ok()) {
        Some(t) => t,
        None => return (None, true),
    };
    let version = match lines.next() {
        Some(v) if !v.is_empty() => v.to_owned(),
        _ => return (None, true),
    };

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let stale = now.saturating_sub(timestamp) > CHECK_INTERVAL_SECS;

    (Some(version), stale)
}

fn write_cache(path: &Path, version: &str) {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let _ = fs::write(path, format!("{now}\n{version}\n"));
}

fn fetch_latest_tag() -> Option<String> {
    let output = Command::new("curl")
        .args([
            "-sf",
            "--max-time",
            "5",
            &format!("https://api.github.com/repos/{REPO}/releases/latest"),
        ])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    // Minimal JSON parsing — extract "tag_name": "vX.Y.Z"
    let body = String::from_utf8_lossy(&output.stdout);
    let tag = body.split("\"tag_name\"").nth(1)?.split('"').nth(1)?;

    Some(tag.trim_start_matches('v').to_owned())
}

/// Returns true if `latest` is strictly newer than `current` (semver comparison).
fn is_newer(latest: &str, current: &str) -> bool {
    let parse = |s: &str| -> Option<(u32, u32, u32)> {
        let mut parts = s.trim_start_matches('v').splitn(3, '.');
        Some((
            parts.next()?.parse().ok()?,
            parts.next()?.parse().ok()?,
            parts.next()?.parse().ok()?,
        ))
    };
    match (parse(latest), parse(current)) {
        (Some(l), Some(c)) => l > c,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn is_newer_compares_semver() {
        assert!(is_newer("0.4.0", "0.3.0"));
        assert!(is_newer("1.0.0", "0.9.9"));
        assert!(!is_newer("0.3.0", "0.3.0"));
        assert!(!is_newer("0.2.0", "0.3.0"));
    }

    #[test]
    fn is_newer_handles_v_prefix() {
        assert!(is_newer("v0.4.0", "v0.3.0"));
    }

    #[test]
    fn read_cache_returns_stale_on_missing_file() {
        let (version, stale) = read_cache(Path::new("/nonexistent/path"));
        assert!(version.is_none());
        assert!(stale);
    }

    #[test]
    fn write_and_read_cache_roundtrips() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("update-check");
        write_cache(&path, "0.5.0");
        let (version, stale) = read_cache(&path);
        assert_eq!(version.as_deref(), Some("0.5.0"));
        assert!(!stale);
    }
}
