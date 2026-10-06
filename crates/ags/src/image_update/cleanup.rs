//! Targeted removal of the superseded final image after a committed
//! publication. Nothing is forced or pruned; problems are reported as
//! warnings because the update itself has already succeeded.

use std::process::Command;

use super::engine;

#[derive(Debug, PartialEq, Eq)]
pub enum PreviousImageCleanup {
    NotNeeded,
    Kept(String),
    Removed(String),
    Retained {
        image_id: String,
        container_ids: Vec<String>,
    },
    Failed {
        image_id: String,
        message: String,
    },
}

impl PreviousImageCleanup {
    pub fn describe(&self) -> Option<String> {
        match self {
            Self::NotNeeded => None,
            Self::Kept(id) => Some(format!(
                "kept previous image {} because --keep-existing was provided",
                short_image_id(id)
            )),
            Self::Removed(id) => Some(format!("removed previous image {}", short_image_id(id))),
            Self::Retained {
                image_id,
                container_ids,
            } => Some(format!(
                "warning: previous image {} is still used by container(s) {}; keeping it\n  remove those containers when no longer needed, then run: podman image rm {image_id}",
                short_image_id(image_id),
                container_ids.join(", ")
            )),
            Self::Failed { image_id, message } => Some(format!(
                "warning: could not remove previous image {} ({message}); the update succeeded. Remove it later with: podman image rm {image_id}",
                short_image_id(image_id)
            )),
        }
    }
}

pub fn remove_previous_image(
    previous_id: Option<&str>,
    published_id: &str,
    keep_existing: bool,
) -> PreviousImageCleanup {
    let Some(previous_id) = previous_id else {
        return PreviousImageCleanup::NotNeeded;
    };
    if normalized_image_id(previous_id) == normalized_image_id(published_id) {
        return PreviousImageCleanup::NotNeeded;
    }
    if keep_existing {
        return PreviousImageCleanup::Kept(previous_id.to_owned());
    }
    let failed = |message: String| PreviousImageCleanup::Failed {
        image_id: previous_id.to_owned(),
        message,
    };
    match containers_using_image(previous_id) {
        Ok(ids) if !ids.is_empty() => return retain(previous_id, ids),
        Ok(_) => {}
        Err(message) => return failed(message),
    }
    let output = match engine::remove_image(previous_id) {
        Ok(output) => output,
        Err(error) => return failed(error.to_string()),
    };
    if output.status.success() {
        return PreviousImageCleanup::Removed(previous_id.to_owned());
    }
    if is_image_reference_conflict(output.status.code())
        && let Ok(ids) = containers_using_image(previous_id)
        && !ids.is_empty()
    {
        return retain(previous_id, ids);
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    failed(
        format!("image rm exited with {} {stderr}", output.status)
            .trim_end()
            .to_owned(),
    )
}

fn retain(image_id: &str, container_ids: Vec<String>) -> PreviousImageCleanup {
    PreviousImageCleanup::Retained {
        image_id: image_id.to_owned(),
        container_ids,
    }
}

fn containers_using_image(image_id: &str) -> Result<Vec<String>, String> {
    let output = Command::new("podman")
        .args(container_image_refs_args())
        .output()
        .map_err(|error| error.to_string())?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(format!(
            "container lookup exited with {} {stderr}",
            output.status
        ));
    }
    let stdout = String::from_utf8(output.stdout)
        .map_err(|error| format!("container lookup returned non-UTF8 output: {error}"))?;
    parse_container_image_refs(&stdout, image_id)
}

pub fn parse_container_image_refs(stdout: &str, image_id: &str) -> Result<Vec<String>, String> {
    let mut container_ids = Vec::new();
    for line in stdout.lines().filter(|line| !line.trim().is_empty()) {
        let (container_id, container_image_id) = line
            .split_once('\t')
            .ok_or_else(|| format!("unexpected container lookup output: {line}"))?;
        let container_id = container_id.trim();
        if container_id.is_empty() {
            return Err("container lookup returned an empty container ID".to_owned());
        }
        if normalized_image_id(container_image_id) == normalized_image_id(image_id) {
            container_ids.push(container_id.to_owned());
        }
    }
    Ok(container_ids)
}

fn normalized_image_id(image_id: &str) -> &str {
    image_id
        .trim()
        .strip_prefix("sha256:")
        .unwrap_or(image_id.trim())
}

pub fn is_image_reference_conflict(exit_code: Option<i32>) -> bool {
    exit_code == Some(2)
}

pub fn short_image_id(id: &str) -> String {
    id.strip_prefix("sha256:")
        .unwrap_or(id)
        .chars()
        .take(12)
        .collect()
}

pub fn container_image_refs_args() -> Vec<String> {
    [
        "ps",
        "--all",
        "--external",
        "--no-trunc",
        "--format",
        "{{.ID}}\t{{.ImageID}}",
    ]
    .map(str::to_owned)
    .to_vec()
}

#[cfg(test)]
#[path = "cleanup_tests.rs"]
mod tests;
