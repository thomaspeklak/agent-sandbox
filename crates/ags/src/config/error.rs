use std::path::PathBuf;

/// Errors that can occur during config loading and validation.
#[derive(Debug)]
pub enum ConfigError {
    /// Failed to read the config file.
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    /// Failed to parse TOML syntax.
    Toml {
        path: PathBuf,
        source: toml::de::Error,
    },
    /// A config value failed validation.
    Validation(String),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io { path, source } => {
                write!(f, "failed to read {}: {source}", path.display())
            }
            Self::Toml { path, source } => {
                write!(f, "invalid TOML in {}: {source}", path.display())
            }
            Self::Validation(msg) => f.write_str(msg),
        }
    }
}

impl std::error::Error for ConfigError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Toml { source, .. } => Some(source),
            Self::Validation(_) => None,
        }
    }
}
