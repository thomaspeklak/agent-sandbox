mod defaults;
mod error;
mod package;
mod parse;
mod raw;
mod tool_download;
mod types;

pub use defaults::{
    BASE_DNF_PACKAGES, DEFAULT_CONFIG, DEFAULT_EXTRA_DNF_PACKAGES, DEFAULT_PI_SPEC,
    LEGACY_PI_SPECS, create_default_config, default_config_path,
};
pub use error::ConfigError;
pub(crate) use package::is_valid_dnf_package_name;
pub use parse::{parse_and_validate, parse_and_validate_with_overlay, parse_toml_str};
pub(crate) use tool_download::{
    valid_tool_id, validate_github_release_source, validate_locked_agent_release_sources,
    validate_locked_tool_downloads, validate_tool_download_source,
};

/// Root-level TOML keys whose arrays are concatenated (not replaced) during overlay merge.
pub const ADDITIVE_ARRAY_KEYS: &[&str] = &["mount", "agent_mount", "tool", "secret"];
pub use raw::RawConfig;
pub use tool_download::{
    ArchiveMemberMatch, GitHubReleaseAssetSelector, GitHubReleaseAssetSelectors,
    GitHubReleaseSelection, GitHubReleaseSource, LockedAgentReleaseSource, LockedToolDownload,
    ToolArchiveFormat, ToolDownloadArtifact, ToolDownloadSource,
};
pub use types::{
    AuthProxyConfig, BrowserConfig, ClipboardConfig, ClipboardMode, DesktopPassthroughConfig,
    HostUiConfig, MountKind, MountMode, MountWhen, PspConfig, SecretSource, UpdateConfig,
    ValidatedConfig, ValidatedMount, ValidatedSandbox, ValidatedSecret, ValidatedTool,
};
