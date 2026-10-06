//! Host-side Node runtime management through a short-lived Linux container.

use std::fmt;
use std::fs;
use std::path::Path;
use std::process::Command;

use crate::cli::{NodeCommand, NodeOptions};
use crate::config::ValidatedConfig;
use crate::node_runtime::{NODE_STORE_CONTAINER, NODE_STORE_SUFFIX, validate_node_version};
use crate::util::shell_quote;

// Omitting `--network` lets Podman select the compatible rootless backend
// (slirp4netns or pasta) rather than hard-coding a backend in this helper.
const INSTALL_NETWORK: Option<&str> = None;
const LIST_NETWORK: Option<&str> = Some("none");

#[derive(Debug)]
pub enum NodeError {
    InvalidVersion(String),
    HostDirCreate {
        path: String,
        error: String,
    },
    ImageContext(String),
    Image(crate::podman::PodmanError),
    Spawn(std::io::Error),
    Failed {
        action: &'static str,
        status: String,
    },
}

impl fmt::Display for NodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidVersion(error) => f.write_str(error),
            Self::HostDirCreate { path, error } => {
                write!(f, "failed to create Node runtime store {path}: {error}")
            }
            Self::ImageContext(error) => {
                write!(f, "could not prepare sandbox image context: {error}")
            }
            Self::Image(error) => write!(f, "{error}"),
            Self::Spawn(error) => write!(f, "failed to start Node runtime helper: {error}"),
            Self::Failed { action, status } => {
                write!(f, "Node runtime {action} helper failed ({status})")
            }
        }
    }
}

impl std::error::Error for NodeError {}

/// Install or list user-managed Node versions in the persistent AGS store.
pub fn run(config: &ValidatedConfig, options: &NodeOptions) -> Result<(), NodeError> {
    let store = config.sandbox.cache_dir.join(NODE_STORE_SUFFIX);
    fs::create_dir_all(&store).map_err(|error| NodeError::HostDirCreate {
        path: store.display().to_string(),
        error: error.to_string(),
    })?;

    let (action, script, network, mode) = match &options.command {
        NodeCommand::Install { version } => {
            let version = validate_requested_version(version)?;
            ("install", install_script(&version), INSTALL_NETWORK, "rw")
        }
        NodeCommand::List => ("list", list_script().to_owned(), LIST_NETWORK, "ro"),
    };

    crate::assets::ensure_image_build_context(&config.sandbox.containerfile)
        .map_err(|error| NodeError::ImageContext(error.to_string()))?;
    crate::podman::ensure_image(
        &config.sandbox.image,
        &config.sandbox.extra_dnf_packages,
        &config.sandbox.tool_downloads,
    )
    .map_err(NodeError::Image)?;
    match crate::podman::image_has_binary(&config.sandbox.image, "mise") {
        Ok(true) => {}
        Ok(false) => {
            return Err(NodeError::ImageContext(
                "sandbox image does not contain mise; run `ags update-image`".to_owned(),
            ));
        }
        Err(error) => return Err(NodeError::Image(error)),
    }

    println!("Running mise {action} in the persistent AGS Node store...");
    let status = Command::new("podman")
        .args(build_helper_run_args(
            &config.sandbox.image,
            &store,
            &script,
            network,
            mode,
        ))
        .status()
        .map_err(NodeError::Spawn)?;
    if !status.success() {
        return Err(NodeError::Failed {
            action,
            status: status.to_string(),
        });
    }
    Ok(())
}

fn validate_requested_version(raw: &str) -> Result<String, NodeError> {
    validate_node_version(raw).map_err(NodeError::InvalidVersion)
}

fn install_script(version: &str) -> String {
    format!(
        "set -eu; export MISE_DATA_DIR={store}; export MISE_CACHE_DIR={store}/cache; mise --no-config install node@{version}",
        store = shell_quote(NODE_STORE_CONTAINER),
        version = shell_quote(version),
    )
}

fn list_script() -> &'static str {
    "set -eu; export MISE_DATA_DIR=/opt/ags/mise; export MISE_CACHE_DIR=/tmp/ags-mise-cache; mise --no-config ls --installed node"
}

pub(crate) fn build_helper_run_args(
    image: &str,
    store: &Path,
    script: &str,
    network: Option<&str>,
    mode: &str,
) -> Vec<String> {
    let mut args = vec![
        "run".to_owned(),
        "--rm".to_owned(),
        "--userns=keep-id".to_owned(),
        "--security-opt=no-new-privileges".to_owned(),
        "--security-opt=label=disable".to_owned(),
        "--cap-drop=all".to_owned(),
    ];
    if let Some(network) = network {
        args.extend(["--network".to_owned(), network.to_owned()]);
    }
    args.extend([
        "-v".to_owned(),
        format!("{}:{NODE_STORE_CONTAINER}:{mode}", store.display()),
        image.to_owned(),
        "bash".to_owned(),
        "-lc".to_owned(),
        script.to_owned(),
    ]);
    args
}

#[cfg(test)]
#[path = "node_tests.rs"]
mod tests;
