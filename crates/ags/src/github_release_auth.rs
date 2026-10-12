use std::fs::File;
use std::io::{Read, Seek};
use std::ops::ControlFlow;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::Duration;

pub(super) fn token() -> Result<Option<String>, String> {
    resolve_token(|name| std::env::var(name).ok(), stored_token)
}

fn resolve_token(
    mut environment: impl FnMut(&str) -> Option<String>,
    stored: impl FnOnce() -> Result<Option<String>, String>,
) -> Result<Option<String>, String> {
    for name in ["GH_TOKEN", "GITHUB_TOKEN"] {
        if let Some(value) = environment(name).filter(|value| !value.trim().is_empty()) {
            return validate_token(&value).map(Some);
        }
    }
    stored()?.map(|value| validate_token(&value)).transpose()
}

fn validate_token(value: &str) -> Result<String, String> {
    let value = value.trim();
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_graphic()) {
        return Err("GitHub authentication token is empty or contains invalid characters".into());
    }
    Ok(value.to_owned())
}

// Capture only the active github.com token, once per resolution, without prompts
// or network/update checks. A regular file avoids blocking on inherited pipes.
fn stored_token() -> Result<Option<String>, String> {
    stored_token_with(Path::new("gh"), Duration::from_secs(5))
}

fn stored_token_with(gh: &Path, timeout: Duration) -> Result<Option<String>, String> {
    let mut output = tempfile::tempfile().map_err(|error| error.to_string())?;
    let mut command = Command::new(gh);
    command
        .args(["auth", "token", "--hostname", "github.com"])
        .env("GH_PROMPT_DISABLED", "1")
        .env("GH_NO_UPDATE_NOTIFIER", "1")
        .env("GH_NO_EXTENSION_UPDATE_NOTIFIER", "1")
        .env_remove("GH_DEBUG")
        .env_remove("DEBUG")
        .env_remove("GH_TOKEN")
        .env_remove("GITHUB_TOKEN")
        .stdin(Stdio::null())
        .stdout(Stdio::from(
            output.try_clone().map_err(|error| error.to_string())?,
        ))
        .stderr(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        command.process_group(0);
    }
    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("could not look up GitHub CLI credentials: {error}")),
    };
    let status = crate::util::poll_until(timeout, Duration::from_millis(10), || {
        match child.try_wait() {
            Ok(Some(status)) => ControlFlow::Break(Ok(status)),
            Ok(None) => ControlFlow::Continue(()),
            Err(error) => ControlFlow::Break(Err(error)),
        }
    });
    // Also terminate helpers that inherited stdout or outlived the CLI.
    #[cfg(unix)]
    unsafe {
        libc::kill(-(child.id() as i32), libc::SIGKILL);
    }
    #[cfg(not(unix))]
    if status.is_none() {
        let _ = child.kill();
    }
    let _ = child.wait();
    let status = status
        .ok_or_else(|| {
            "GitHub CLI credential lookup timed out; unlock its credential store or set GH_TOKEN"
                .to_owned()
        })?
        .map_err(|error| error.to_string())?;
    if !status.success() {
        return Ok(None);
    }
    output.rewind().map_err(|error| error.to_string())?;
    read_token(output).map(Some)
}

fn read_token(output: File) -> Result<String, String> {
    let mut value = String::new();
    output
        .take(16 * 1024 + 1)
        .read_to_string(&mut value)
        .map_err(|_| "GitHub CLI returned invalid credential data".to_owned())?;
    if value.len() > 16 * 1024 {
        return Err("GitHub CLI credential output exceeded the size limit".into());
    }
    validate_token(&value)
}

#[cfg(test)]
#[path = "github_release_auth_tests.rs"]
mod tests;
