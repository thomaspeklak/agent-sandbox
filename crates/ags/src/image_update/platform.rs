use serde::{Deserialize, Serialize};

/// Container platform of the Podman host that builds and runs the sandbox.
/// Every component key includes it, so an amd64 artifact is never reused on
/// arm64.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Platform {
    pub os: String,
    pub arch: String,
}

impl Platform {
    /// Normalize Podman/OCI platform names; only Linux amd64 and arm64 are
    /// supported by the vendor-tool locks and Rust toolchain mapping.
    pub fn from_podman(os: &str, arch: &str) -> Result<Self, String> {
        if os != "linux" {
            return Err(format!("operating system '{os}' (expected linux)"));
        }
        let arch = match arch {
            "amd64" | "x86_64" => "amd64",
            "arm64" | "aarch64" => "arm64",
            other => return Err(format!("architecture '{other}' (expected amd64 or arm64)")),
        };
        Ok(Self {
            os: os.to_owned(),
            arch: arch.to_owned(),
        })
    }

    /// Rust host triple of the image-owned toolchain.
    pub fn rust_triple(&self) -> &'static str {
        if self.arch == "arm64" {
            "aarch64-unknown-linux-gnu"
        } else {
            "x86_64-unknown-linux-gnu"
        }
    }

    /// Architecture key used by `LockedToolDownload` artifacts.
    pub fn vendor_arch(&self) -> &'static str {
        if self.arch == "arm64" {
            "aarch64"
        } else {
            "x86_64"
        }
    }

    /// Whether an image reporting `os`/`arch` was built for this platform.
    pub fn matches_image(&self, os: &str, arch: &str) -> bool {
        Self::from_podman(os, arch).is_ok_and(|image| image == *self)
    }
}

impl std::fmt::Display for Platform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}/{}", self.os, self.arch)
    }
}

#[cfg(test)]
#[path = "platform_tests.rs"]
mod tests;
