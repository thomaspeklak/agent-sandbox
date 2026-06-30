use std::fmt;

pub const HOST_SERVICES_HOST: &str = "host.containers.internal";
pub const PASTA_HOST_LOOPBACK_NETWORK: &str = "pasta:--map-host-loopback=169.254.1.2";
pub const SLIRP_HOST_GATEWAY: &str = "10.0.2.2";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PodmanNetwork {
    Pasta,
    Slirp4netns,
}

impl PodmanNetwork {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "pasta" => Ok(Self::Pasta),
            "slirp4netns" | "slirp4net" => Ok(Self::Slirp4netns),
            _ => Err(format!(
                "invalid Podman network '{value}' (expected pasta|slirp4netns)"
            )),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pasta => "pasta",
            Self::Slirp4netns => "slirp4netns",
        }
    }

    pub fn podman_mode(self, browser_mode: bool) -> &'static str {
        match (self, browser_mode) {
            (Self::Pasta, true) => PASTA_HOST_LOOPBACK_NETWORK,
            (Self::Pasta, false) => "pasta",
            (Self::Slirp4netns, true) => "slirp4netns:allow_host_loopback=true",
            (Self::Slirp4netns, false) => "slirp4netns:allow_host_loopback=false",
        }
    }

    pub fn browser_host(self) -> &'static str {
        match self {
            Self::Pasta => HOST_SERVICES_HOST,
            Self::Slirp4netns => SLIRP_HOST_GATEWAY,
        }
    }
}

impl Default for PodmanNetwork {
    fn default() -> Self {
        Self::Pasta
    }
}

impl fmt::Display for PodmanNetwork {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
