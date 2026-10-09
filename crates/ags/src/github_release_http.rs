use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

use time::OffsetDateTime;
use url::Url;

use crate::github_release::{FetchRequest, GitHubReleaseError, RELEASES_PER_PAGE};

#[path = "github_release_auth.rs"]
mod auth;

pub(super) struct Client {
    token: Option<String>,
}

impl Client {
    pub(super) fn new(repo: &str) -> Result<Self, GitHubReleaseError> {
        Ok(Self {
            token: auth::token().map_err(|message| fetch_error(repo, message))?,
        })
    }

    pub(super) fn fetch(
        &self,
        repo: &str,
        request: FetchRequest,
    ) -> Result<Vec<u8>, GitHubReleaseError> {
        match request {
            FetchRequest::ReleasesPage(page) => self.fetch_url(
                repo,
                &format!("https://api.github.com/repos/{repo}/releases?per_page={RELEASES_PER_PAGE}&page={page}"),
                false,
            ),
            FetchRequest::ReleaseByTag(tag) => {
                let mut url = Url::parse("https://api.github.com").expect("static GitHub API URL");
                url.path_segments_mut().expect("GitHub API path segments")
                    .extend(["repos"]).extend(repo.split('/')).extend(["releases", "tags", &tag]);
                self.fetch_url(repo, url.as_str(), false)
            }
            FetchRequest::Asset(url) => self.fetch_url(repo, &url, true),
        }
    }

    fn fetch_url(
        &self,
        repo: &str,
        url: &str,
        checksum: bool,
    ) -> Result<Vec<u8>, GitHubReleaseError> {
        self.fetch_with_curl(repo, url, checksum, Path::new("curl"))
    }

    fn fetch_with_curl(
        &self,
        repo: &str,
        url: &str,
        checksum: bool,
        curl: &Path,
    ) -> Result<Vec<u8>, GitHubReleaseError> {
        let body =
            tempfile::NamedTempFile::new().map_err(|error| fetch_error(repo, error.to_string()))?;
        let headers =
            tempfile::NamedTempFile::new().map_err(|error| fetch_error(repo, error.to_string()))?;
        let token = if !checksum && is_github_api(url) {
            self.token.as_deref()
        } else {
            None
        };
        let mut command = Command::new(curl);
        command.args([
            "--disable",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--tlsv1.2",
            "--fail-with-body",
            "-sSL",
            "--connect-timeout",
            "10",
            "--max-time",
            "30",
            "--retry",
            "2",
            "--retry-delay",
            "1",
            "--write-out",
            "%{http_code}",
            "-H",
            "Accept: application/vnd.github+json",
            "-H",
            "User-Agent: ags",
        ]);
        command
            .arg("--output")
            .arg(body.path())
            .arg("--dump-header")
            .arg(headers.path());
        if checksum {
            command.args(["--max-filesize", "1048576"]);
        }
        if token.is_some() {
            command.args(["--config", "-"]);
        }
        let mut child = command
            .arg(url)
            .env_remove("GH_TOKEN")
            .env_remove("GITHUB_TOKEN")
            .stdin(if token.is_some() {
                Stdio::piped()
            } else {
                Stdio::null()
            })
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|error| fetch_error(repo, error.to_string()))?;
        if let Some(token) = token {
            let escaped = token.replace('\\', "\\\\").replace('"', "\\\"");
            let config = format!("header = \"Authorization: Bearer {escaped}\"\n");
            if let Err(error) = child
                .stdin
                .take()
                .expect("piped curl config")
                .write_all(config.as_bytes())
            {
                let _ = child.kill();
                let _ = child.wait();
                return Err(fetch_error(
                    repo,
                    format!("could not pass GitHub authentication to curl: {error}"),
                ));
            }
        }
        let output = child
            .wait_with_output()
            .map_err(|error| fetch_error(repo, error.to_string()))?;
        let status = String::from_utf8_lossy(&output.stdout)
            .trim()
            .parse::<u16>()
            .unwrap_or(0);
        if !output.status.success() || !(200..300).contains(&status) {
            let header_text = std::fs::read_to_string(headers.path()).unwrap_or_default();
            let body_text = std::fs::read_to_string(body.path()).unwrap_or_default();
            let message = http_error(
                status,
                &header_text,
                &body_text,
                &String::from_utf8_lossy(&output.stderr),
                token,
            );
            return Err(fetch_error(repo, message));
        }
        std::fs::read(body.path()).map_err(|error| fetch_error(repo, error.to_string()))
    }
}

fn is_github_api(url: &str) -> bool {
    Url::parse(url).is_ok_and(|url| {
        url.scheme() == "https"
            && url.host_str() == Some("api.github.com")
            && url.port_or_known_default() == Some(443)
            && url.username().is_empty()
            && url.password().is_none()
    })
}

fn fetch_error(repo: &str, message: String) -> GitHubReleaseError {
    GitHubReleaseError::Fetch {
        repo: repo.to_owned(),
        message,
    }
}

fn http_error(status: u16, headers: &str, body: &str, stderr: &str, token: Option<&str>) -> String {
    let mut fields = BTreeMap::new();
    for line in headers.lines() {
        if line.starts_with("HTTP/") {
            fields.clear();
        } else if let Some((name, value)) = line.split_once(':') {
            fields.insert(name.to_ascii_lowercase(), value.trim().to_owned());
        }
    }
    let reason = serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|value| value["message"].as_str().map(str::to_owned))
        .unwrap_or_else(|| stderr.trim().to_owned());
    let reason = if let Some(token) = token {
        reason.replace(token, "[redacted]")
    } else {
        reason
    };
    let reason: String = reason
        .chars()
        .filter(|character| !character.is_control())
        .take(800)
        .collect();
    let mut message = if status == 0 {
        format!("curl request failed: {reason}")
    } else {
        format!("HTTP {status}: {reason}")
    };
    let rate_limited = status == 429
        || ((status == 403)
            && (fields
                .get("x-ratelimit-remaining")
                .is_some_and(|value| value == "0")
                || reason.to_ascii_lowercase().contains("rate limit")));
    if rate_limited {
        if let Some(reset) = fields
            .get("x-ratelimit-reset")
            .and_then(|value| value.parse::<i64>().ok())
            .and_then(|reset| OffsetDateTime::from_unix_timestamp(reset).ok())
            .map(|reset| {
                format!(
                    "{} {:02}:{:02}:{:02} UTC",
                    reset.date(),
                    reset.hour(),
                    reset.minute(),
                    reset.second()
                )
            })
        {
            message.push_str(&format!("; GitHub rate limit resets at {reset}"));
        }
        if let Some(seconds) = fields
            .get("retry-after")
            .and_then(|value| value.parse::<u64>().ok())
        {
            message.push_str(&format!("; retry after {seconds} seconds"));
        }
        if token.is_none() {
            message.push_str("; request was anonymous: run `gh auth login --hostname github.com` or set GH_TOKEN (GITHUB_TOKEN is also supported)");
        } else {
            message.push_str(
                "; the authenticated GitHub API quota is exhausted; retry after the limit resets",
            );
        }
    } else if status == 401 {
        message.push_str("; GitHub rejected the credentials: refresh your GitHub CLI login or replace GH_TOKEN/GITHUB_TOKEN");
    }
    message
}

#[cfg(test)]
#[path = "github_release_http_tests.rs"]
mod tests;
