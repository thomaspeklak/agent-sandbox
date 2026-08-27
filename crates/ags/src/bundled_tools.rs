use std::process::Command;

use crate::github_release::{ReleaseSelection, resolve_latest_mature_release};

const BR_REPO: &str = "Dicklesworthstone/beads_rust";
const DCG_REPO: &str = "Dicklesworthstone/destructive_command_guard";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct BundledToolVersions {
    pub(crate) br: String,
    pub(crate) dcg: String,
}

#[derive(Clone, Copy, Debug)]
enum BuildArchitecture {
    X86_64,
    Aarch64,
}

impl BuildArchitecture {
    fn detect() -> Result<Self, String> {
        let output = Command::new("podman")
            .args(["info", "--format", "{{.Host.Arch}}"])
            .output()
            .map_err(|error| format!("could not inspect Podman build architecture: {error}"))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            return Err(format!(
                "could not inspect Podman build architecture: podman info exited with {}{}",
                output.status,
                if stderr.is_empty() {
                    String::new()
                } else {
                    format!(" ({stderr})")
                }
            ));
        }

        let arch = String::from_utf8(output.stdout)
            .map_err(|error| format!("Podman returned a non-UTF8 build architecture: {error}"))?;
        Self::from_podman_arch(arch.trim())
    }

    fn from_podman_arch(arch: &str) -> Result<Self, String> {
        match arch {
            "amd64" | "x86_64" => Ok(Self::X86_64),
            "aarch64" | "arm64" => Ok(Self::Aarch64),
            _ => Err(format!(
                "unsupported architecture for bundled GitHub dependencies: {arch}"
            )),
        }
    }
}

/// Resolve the mature, compatible br and dcg versions for an image build.
pub(crate) fn resolve_bundled_tool_versions(
    minimum_release_age: u32,
) -> Result<BundledToolVersions, String> {
    let arch = BuildArchitecture::detect()?;
    let br_assets = br_required_assets(arch);
    let dcg_assets = dcg_required_assets(arch);
    let br = resolve_latest_mature_release(BR_REPO, minimum_release_age, &br_assets)
        .map_err(|error| error.to_string())?;
    let dcg = resolve_latest_mature_release(DCG_REPO, minimum_release_age, &dcg_assets)
        .map_err(|error| error.to_string())?;

    warn_if_fallback("br", &br);
    warn_if_fallback("dcg", &dcg);

    Ok(BundledToolVersions {
        br: br.tag_name,
        dcg: dcg.tag_name,
    })
}

fn br_required_assets(arch: BuildArchitecture) -> [String; 2] {
    let arch = match arch {
        BuildArchitecture::X86_64 => "amd64",
        BuildArchitecture::Aarch64 => "arm64",
    };
    let archive = format!("br-{{version}}-linux_{arch}.tar.gz");
    [archive.clone(), format!("{archive}.sha256")]
}

fn dcg_required_assets(arch: BuildArchitecture) -> [String; 2] {
    let target = match arch {
        BuildArchitecture::X86_64 => "x86_64-unknown-linux-musl",
        BuildArchitecture::Aarch64 => "aarch64-unknown-linux-gnu",
    };
    let archive = format!("dcg-{target}.tar.xz");
    [archive.clone(), format!("{archive}.sha256")]
}

fn warn_if_fallback(name: &str, selection: &ReleaseSelection) {
    if selection.tag_name != selection.latest_tag_name {
        eprintln!(
            "warning: latest {name} release {} is ineligible under the configured release policy; using {}",
            selection.latest_tag_name, selection.tag_name
        );
    }
}

#[cfg(test)]
#[path = "bundled_tools_tests.rs"]
mod tests;
