//! Thin Podman wrappers used by the image pipeline. Argument construction is
//! kept in pure functions so it can be tested without an engine.

use std::collections::BTreeMap;
use std::io::Write;
use std::process::{Command, Output, Stdio};

use serde::Deserialize;

use super::error::ImageUpdateError;
use super::platform::Platform;
use crate::podman::{ImageBuild, build_image_args};

pub const LABEL_COMPONENT: &str = "io.ags.image.component";
pub const LABEL_KEY: &str = "io.ags.image.key";

/// Identity of the Podman host whose storage holds the images.
#[derive(Debug, Clone)]
pub struct HostInfo {
    pub platform: Platform,
    pub graph_root: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImageInfo {
    pub id: String,
    pub digest: String,
    pub os: String,
    pub arch: String,
    pub labels: BTreeMap<String, String>,
}

impl ImageInfo {
    /// Whether this image is the AGS `component` built for `key` on `platform`.
    pub fn is_component(&self, component: &str, key: &str, platform: &Platform) -> bool {
        self.labels.get(LABEL_COMPONENT).map(String::as_str) == Some(component)
            && self.labels.get(LABEL_KEY).map(String::as_str) == Some(key)
            && platform.matches_image(&self.os, &self.arch)
    }
}

/// Strip an optional `sha256:` prefix from an image ID.
pub fn normalize_id(id: &str) -> String {
    let id = id.trim();
    id.strip_prefix("sha256:")
        .unwrap_or(id)
        .to_ascii_lowercase()
}

fn run(args: &[String]) -> Result<Output, ImageUpdateError> {
    Command::new("podman")
        .args(args)
        .output()
        .map_err(|error| ImageUpdateError::podman(&args.join(" "), error))
}

fn failure(context: &str, output: &Output) -> ImageUpdateError {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    ImageUpdateError::podman(
        context,
        format!("exited with {}{}", output.status, suffix(&stderr)),
    )
}

fn suffix(stderr: &str) -> String {
    if stderr.is_empty() {
        String::new()
    } else {
        format!(" ({stderr})")
    }
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

pub fn host_info() -> Result<HostInfo, ImageUpdateError> {
    let output = run(&strings(&["info", "--format", "json"]))?;
    if !output.status.success() {
        return Err(failure("podman info", &output));
    }
    parse_host_info(&output.stdout)
}

pub fn parse_host_info(stdout: &[u8]) -> Result<HostInfo, ImageUpdateError> {
    #[derive(Deserialize)]
    struct Info {
        host: Host,
        store: Store,
    }
    #[derive(Deserialize)]
    struct Host {
        os: String,
        arch: String,
    }
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Store {
        graph_root: String,
    }
    let info: Info = serde_json::from_slice(stdout)
        .map_err(|error| ImageUpdateError::podman("podman info", error))?;
    Ok(HostInfo {
        platform: Platform::from_podman(&info.host.os, &info.host.arch)
            .map_err(ImageUpdateError::Platform)?,
        graph_root: info.store.graph_root,
    })
}

/// Whether a local image exists, without inspecting it.
pub fn exists(reference: &str) -> Result<bool, ImageUpdateError> {
    let exists = run(&strings(&["image", "exists", reference]))?;
    match exists.status.code() {
        Some(0) => Ok(true),
        Some(1) => Ok(false),
        _ => Err(failure("podman image exists", &exists)),
    }
}

/// Inspect a local image by name or ID; `None` when it does not exist.
pub fn inspect(reference: &str) -> Result<Option<ImageInfo>, ImageUpdateError> {
    if !exists(reference)? {
        return Ok(None);
    }
    let output = run(&strings(&["image", "inspect", reference]))?;
    if !output.status.success() {
        return Err(failure("podman image inspect", &output));
    }
    parse_inspect(&output.stdout).map(Some)
}

pub fn parse_inspect(stdout: &[u8]) -> Result<ImageInfo, ImageUpdateError> {
    #[derive(Deserialize)]
    #[serde(rename_all = "PascalCase")]
    struct Inspect {
        id: String,
        #[serde(default)]
        digest: String,
        #[serde(default)]
        os: String,
        #[serde(default)]
        architecture: String,
        #[serde(default)]
        labels: Option<BTreeMap<String, String>>,
    }
    let mut images: Vec<Inspect> = serde_json::from_slice(stdout)
        .map_err(|error| ImageUpdateError::podman("podman image inspect", error))?;
    if images.len() != 1 {
        return Err(ImageUpdateError::podman(
            "podman image inspect",
            format!("expected one image, got {}", images.len()),
        ));
    }
    let image = images.remove(0);
    Ok(ImageInfo {
        id: normalize_id(&image.id),
        digest: image.digest,
        os: image.os,
        arch: image.architecture,
        labels: image.labels.unwrap_or_default(),
    })
}

/// IDs of local images carrying `key`, newest first (Podman's default order).
pub fn images_with_key(key: &str) -> Result<Vec<String>, ImageUpdateError> {
    let output = run(&images_with_key_args(key))?;
    if !output.status.success() {
        return Err(failure("podman images", &output));
    }
    let mut ids: Vec<String> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(normalize_id)
        .filter(|id| !id.is_empty())
        .collect();
    ids.dedup();
    Ok(ids)
}

pub fn images_with_key_args(key: &str) -> Vec<String> {
    vec![
        "images".to_owned(),
        "--no-trunc".to_owned(),
        "--filter".to_owned(),
        format!("label={LABEL_KEY}={key}"),
        "--format".to_owned(),
        "{{.ID}}".to_owned(),
    ]
}

/// Run `podman build` with inherited output and return the built image ID.
pub fn build(component: &str, build: &ImageBuild<'_>) -> Result<String, ImageUpdateError> {
    let status = Command::new("podman")
        .args(build_image_args(build))
        .status()
        .map_err(|error| ImageUpdateError::build(component, error))?;
    if !status.success() {
        return Err(ImageUpdateError::build(
            component,
            format!("podman build exited with {status}"),
        ));
    }
    let id = std::fs::read_to_string(build.iidfile).map_err(|error| {
        ImageUpdateError::build(component, format!("missing image ID: {error}"))
    })?;
    let id = normalize_id(&id);
    if id.is_empty() {
        return Err(ImageUpdateError::build(component, "empty image ID"));
    }
    Ok(id)
}

pub fn pull(reference: &str) -> Result<(), String> {
    let output = Command::new("podman")
        .args(["pull", "--quiet", reference])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|error| error.to_string())?;
    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        Err(format!(
            "podman pull exited with {}{}",
            output.status,
            suffix(&stderr)
        ))
    }
}

pub fn tag(id: &str, name: &str) -> Result<(), ImageUpdateError> {
    let output = run(&strings(&["tag", id, name]))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(failure("podman tag", &output))
    }
}

/// Remove one AGS-owned name without touching other names of the image.
/// A name that does not exist is not an error.
pub fn untag(name: &str) -> Result<(), ImageUpdateError> {
    if !exists(name)? {
        return Ok(());
    }
    let output = run(&strings(&["untag", name, name]))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(failure("podman untag", &output))
    }
}

/// `podman image rm --no-prune <id>` without force: parents, other tags, and
/// images used by containers are never removed.
pub fn remove_image(id: &str) -> Result<Output, ImageUpdateError> {
    run(&remove_image_args(id))
}

pub fn remove_image_args(id: &str) -> Vec<String> {
    strings(&["image", "rm", "--no-prune", id])
}

/// Run a disposable container and capture its output. `stdin` is written to
/// the container when given.
pub fn run_container(args: &[String], stdin: Option<&str>) -> Result<Output, ImageUpdateError> {
    let mut child = Command::new("podman")
        .args(args)
        .stdin(if stdin.is_some() {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| ImageUpdateError::podman("podman run", error))?;
    if let (Some(input), Some(mut pipe)) = (stdin, child.stdin.take()) {
        pipe.write_all(input.as_bytes())
            .map_err(|error| ImageUpdateError::podman("podman run stdin", error))?;
    }
    child
        .wait_with_output()
        .map_err(|error| ImageUpdateError::podman("podman run", error))
}

#[cfg(test)]
#[path = "engine_tests.rs"]
mod tests;
