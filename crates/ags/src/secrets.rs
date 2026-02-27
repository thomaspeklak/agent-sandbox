use std::collections::HashMap;
use std::fmt;
use std::process::Command;

use crate::config::{SecretSource, ValidatedSecret};

#[derive(Debug)]
pub enum SecretError {
    /// All sources for a required secret failed.
    Unresolved { env: String, sources_tried: usize },
}

impl fmt::Display for SecretError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unresolved { env, sources_tried } => write!(
                f,
                "secret '{env}' unresolved after {sources_tried} source(s)"
            ),
        }
    }
}

impl std::error::Error for SecretError {}

/// Abstraction over secret backends so tests can mock secret-tool.
pub trait SecretBackend {
    /// Look up an environment variable by name.
    fn env_var(&self, name: &str) -> Option<String>;

    /// Run `secret-tool lookup` with the given key-value attribute pairs.
    /// Returns `None` if secret-tool is not installed, command fails, or output is empty.
    fn secret_tool_lookup(&self, attributes: &[(&str, &str)]) -> Option<String>;
}

/// Real backend that delegates to the OS environment and `secret-tool` binary.
pub struct OsSecretBackend;

impl SecretBackend for OsSecretBackend {
    fn env_var(&self, name: &str) -> Option<String> {
        std::env::var(name).ok().filter(|v| !v.is_empty())
    }

    fn secret_tool_lookup(&self, attributes: &[(&str, &str)]) -> Option<String> {
        if attributes.is_empty() {
            return None;
        }

        let args: Vec<&str> = std::iter::once("lookup")
            .chain(attributes.iter().flat_map(|(k, v)| [*k, *v]))
            .collect();

        let output = Command::new("secret-tool").args(&args).output().ok()?;

        if !output.status.success() {
            return None;
        }

        let value = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        if value.is_empty() { None } else { Some(value) }
    }
}

/// Try each source for a single secret entry. Returns the resolved value or `None`.
fn try_resolve_one(secret: &ValidatedSecret, backend: &dyn SecretBackend) -> Option<String> {
    match &secret.source {
        SecretSource::Env { from_env } => backend.env_var(from_env),
        SecretSource::SecretTool { attributes } => {
            let pairs: Vec<(&str, &str)> = attributes
                .iter()
                .map(|(k, v)| (k.as_str(), v.as_str()))
                .collect();
            backend.secret_tool_lookup(&pairs)
        }
    }
}

/// Resolve all configured secrets using the given backend.
///
/// Secrets sharing the same `env` name are alternative sources tried in order.
/// The first source that produces a non-empty value wins; remaining sources are skipped.
///
/// Returns a map of env-var-name → resolved-value for every secret that resolved.
/// Secrets that cannot be resolved from any source are silently omitted (the caller
/// or a later validation step decides whether that is fatal).
pub fn resolve_secrets(
    secrets: &[ValidatedSecret],
    backend: &dyn SecretBackend,
) -> HashMap<String, String> {
    let mut resolved: HashMap<String, String> = HashMap::new();

    for secret in secrets {
        if resolved.contains_key(&secret.env) {
            continue;
        }
        if let Some(value) = try_resolve_one(secret, backend) {
            resolved.insert(secret.env.clone(), value);
        }
    }

    resolved
}
