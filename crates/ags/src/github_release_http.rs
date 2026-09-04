use std::process::Command;

use crate::github_release::GitHubReleaseError;

pub(super) fn fetch_url(
    repo: &str,
    url: &str,
    checksum: bool,
) -> Result<Vec<u8>, GitHubReleaseError> {
    let mut command = Command::new("curl");
    command.args([
        "--proto",
        "=https",
        "--proto-redir",
        "=https",
        "--tlsv1.2",
        "-fsSL",
        "--connect-timeout",
        "10",
        "--max-time",
        "30",
        "--retry",
        "2",
        "--retry-delay",
        "1",
        "-H",
        "Accept: application/vnd.github+json",
        "-H",
        "User-Agent: ags",
    ]);
    if checksum {
        command.args(["--max-filesize", "1048576"]);
    }
    let output = command
        .arg(url)
        .output()
        .map_err(|error| GitHubReleaseError::Fetch {
            repo: repo.to_owned(),
            message: error.to_string(),
        })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(GitHubReleaseError::Fetch {
            repo: repo.to_owned(),
            message: format!(
                "curl exited with {}{}",
                output.status,
                if stderr.is_empty() {
                    String::new()
                } else {
                    format!(" ({stderr})")
                }
            ),
        });
    }
    Ok(output.stdout)
}
