mod defaults;
mod error;
mod parse;
mod raw;
mod types;

pub use defaults::{
    DEFAULT_CONFIG, DEFAULT_PI_SPEC, LEGACY_PI_SPECS, create_default_config, default_config_path,
};
pub use error::ConfigError;
pub use parse::{parse_and_validate, parse_and_validate_with_overlay, parse_toml_str};

/// Root-level TOML keys whose arrays are concatenated (not replaced) during overlay merge.
pub const ADDITIVE_ARRAY_KEYS: &[&str] = &["mount", "agent_mount", "tool", "secret"];
pub use raw::RawConfig;
pub use types::{
    AuthProxyConfig, BrowserConfig, ClipboardConfig, ClipboardMode, DesktopPassthroughConfig,
    HostUiConfig, MountKind, MountMode, MountWhen, PspConfig, SecretSource, UpdateConfig,
    ValidatedConfig, ValidatedMount, ValidatedSandbox, ValidatedSecret, ValidatedTool,
};
