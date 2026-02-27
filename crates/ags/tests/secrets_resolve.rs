use std::collections::{BTreeMap, HashMap};

use ags::config::{SecretSource, ValidatedSecret};
use ags::secrets::{SecretBackend, resolve_secrets};

/// Fake backend that returns pre-configured values for env vars and secret-tool lookups.
struct FakeBackend {
    env_vars: HashMap<String, String>,
    /// Key = sorted attribute pairs as string, Value = secret value.
    secret_tool_values: HashMap<String, String>,
}

impl FakeBackend {
    fn new() -> Self {
        Self {
            env_vars: HashMap::new(),
            secret_tool_values: HashMap::new(),
        }
    }

    fn with_env(mut self, name: &str, value: &str) -> Self {
        self.env_vars.insert(name.to_owned(), value.to_owned());
        self
    }

    fn with_secret_tool(mut self, attributes: &[(&str, &str)], value: &str) -> Self {
        let key = attr_key(attributes);
        self.secret_tool_values.insert(key, value.to_owned());
        self
    }
}

fn attr_key(attributes: &[(&str, &str)]) -> String {
    let mut pairs: Vec<_> = attributes.iter().map(|(k, v)| format!("{k}={v}")).collect();
    pairs.sort();
    pairs.join(",")
}

impl SecretBackend for FakeBackend {
    fn env_var(&self, name: &str) -> Option<String> {
        self.env_vars.get(name).cloned()
    }

    fn secret_tool_lookup(&self, attributes: &[(&str, &str)]) -> Option<String> {
        let key = attr_key(attributes);
        self.secret_tool_values.get(&key).cloned()
    }
}

fn env_secret(env: &str, from_env: &str) -> ValidatedSecret {
    ValidatedSecret {
        env: env.to_owned(),
        source: SecretSource::Env {
            from_env: from_env.to_owned(),
        },
        origin: "test".to_owned(),
        tool: None,
    }
}

fn secret_tool_secret(env: &str, attrs: &[(&str, &str)]) -> ValidatedSecret {
    let attributes: BTreeMap<String, String> = attrs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect();
    ValidatedSecret {
        env: env.to_owned(),
        source: SecretSource::SecretTool { attributes },
        origin: "test".to_owned(),
        tool: None,
    }
}

#[test]
fn env_source_resolves() {
    let backend = FakeBackend::new().with_env("MY_TOKEN", "tok123");
    let secrets = vec![env_secret("API_KEY", "MY_TOKEN")];

    let result = resolve_secrets(&secrets, &backend);
    assert_eq!(result.get("API_KEY").unwrap(), "tok123");
}

#[test]
fn secret_tool_source_resolves() {
    let backend =
        FakeBackend::new().with_secret_tool(&[("service", "github"), ("user", "bot")], "gh-tok");
    let secrets = vec![secret_tool_secret(
        "GITHUB_TOKEN",
        &[("service", "github"), ("user", "bot")],
    )];

    let result = resolve_secrets(&secrets, &backend);
    assert_eq!(result.get("GITHUB_TOKEN").unwrap(), "gh-tok");
}

#[test]
fn first_source_wins() {
    let backend = FakeBackend::new()
        .with_env("FROM_ENV", "env-value")
        .with_secret_tool(&[("svc", "x")], "keyring-value");

    // env source listed first — should win
    let secrets = vec![
        env_secret("MY_SECRET", "FROM_ENV"),
        secret_tool_secret("MY_SECRET", &[("svc", "x")]),
    ];

    let result = resolve_secrets(&secrets, &backend);
    assert_eq!(result.get("MY_SECRET").unwrap(), "env-value");
}

#[test]
fn fallback_to_second_source() {
    let backend = FakeBackend::new().with_secret_tool(&[("svc", "x")], "keyring-value");

    // env source first (missing) → falls through to secret-tool
    let secrets = vec![
        env_secret("MY_SECRET", "NONEXISTENT_VAR"),
        secret_tool_secret("MY_SECRET", &[("svc", "x")]),
    ];

    let result = resolve_secrets(&secrets, &backend);
    assert_eq!(result.get("MY_SECRET").unwrap(), "keyring-value");
}

#[test]
fn unresolvable_secret_omitted() {
    let backend = FakeBackend::new();
    let secrets = vec![env_secret("MISSING", "NOPE")];

    let result = resolve_secrets(&secrets, &backend);
    assert!(result.is_empty());
}

#[test]
fn multiple_env_vars_resolved_independently() {
    let backend = FakeBackend::new()
        .with_env("A_SRC", "aaa")
        .with_env("B_SRC", "bbb");

    let secrets = vec![
        env_secret("SECRET_A", "A_SRC"),
        env_secret("SECRET_B", "B_SRC"),
    ];

    let result = resolve_secrets(&secrets, &backend);
    assert_eq!(result.len(), 2);
    assert_eq!(result["SECRET_A"], "aaa");
    assert_eq!(result["SECRET_B"], "bbb");
}

#[test]
fn already_resolved_env_skips_later_entries() {
    let backend = FakeBackend::new()
        .with_env("SRC1", "first")
        .with_env("SRC2", "second");

    // Two entries for same env var — first one resolves, second should be skipped
    let secrets = vec![env_secret("TOKEN", "SRC1"), env_secret("TOKEN", "SRC2")];

    let result = resolve_secrets(&secrets, &backend);
    assert_eq!(result["TOKEN"], "first");
}

#[test]
fn empty_secrets_list_returns_empty_map() {
    let backend = FakeBackend::new();
    let result = resolve_secrets(&[], &backend);
    assert!(result.is_empty());
}
