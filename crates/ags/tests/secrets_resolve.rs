use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

use ags::config::{SecretSource, ValidatedSecret};
use ags::secrets::{
    CommandSecretError, HostCommandRunner, OsHostCommandRunner, SecretBackend, resolve_secrets,
    resolve_secrets_for_run, resolve_secrets_with_runner,
};

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

fn command_secret(env: &str, argv: &[&str]) -> ValidatedSecret {
    ValidatedSecret {
        env: env.to_owned(),
        source: SecretSource::Command {
            argv: argv.iter().map(|value| (*value).to_owned()).collect(),
        },
        origin: "test".to_owned(),
        tool: None,
    }
}

struct FakeCommandRunner {
    calls: RefCell<Vec<Vec<String>>>,
    outcomes: HashMap<String, Result<String, CommandSecretError>>,
}

impl FakeCommandRunner {
    fn with_outcome(argv0: &str, outcome: Result<&str, CommandSecretError>) -> Self {
        Self {
            calls: RefCell::new(Vec::new()),
            outcomes: HashMap::from([(argv0.to_owned(), outcome.map(ToOwned::to_owned))]),
        }
    }
}

impl HostCommandRunner for FakeCommandRunner {
    fn lookup(&self, argv: &[String], _timeout: Duration) -> Result<String, CommandSecretError> {
        self.calls.borrow_mut().push(argv.to_vec());
        self.outcomes
            .get(&argv[0])
            .cloned()
            .unwrap_or(Err(CommandSecretError::Spawn(std::io::ErrorKind::NotFound)))
    }
}

#[cfg(unix)]
fn write_helper(dir: &Path, name: &str, body: &str) -> String {
    use std::os::unix::fs::PermissionsExt;

    let path = dir.join(name);
    fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    path.to_string_lossy().into_owned()
}

#[cfg(unix)]
fn process_is_gone(pid: i32) -> bool {
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if unsafe { libc::kill(pid, 0) } != 0 {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
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

#[test]
fn command_source_resolves_and_preserves_argv() {
    let runner = FakeCommandRunner::with_outcome("/helper", Ok("command-value"));
    let secrets = vec![command_secret(
        "TOKEN",
        &["/helper", "literal value", "$HOME", "$(whoami)", ""],
    )];

    let result = resolve_secrets_with_runner(&secrets, &FakeBackend::new(), &runner);
    assert_eq!(result["TOKEN"], "command-value");
    assert_eq!(
        runner.calls.borrow().as_slice(),
        &[vec!["/helper", "literal value", "$HOME", "$(whoami)", ""]]
    );
}

#[test]
fn failed_command_falls_through_to_next_source() {
    let runner = FakeCommandRunner::with_outcome("/helper", Err(CommandSecretError::TimedOut));
    let backend = FakeBackend::new().with_env("HOST_TOKEN", "fallback-value");
    let secrets = vec![
        command_secret("TOKEN", &["/helper"]),
        env_secret("TOKEN", "HOST_TOKEN"),
    ];

    let result = resolve_secrets_with_runner(&secrets, &backend, &runner);
    assert_eq!(result["TOKEN"], "fallback-value");
}

#[test]
fn successful_command_skips_later_sources() {
    let runner = FakeCommandRunner::with_outcome("/helper", Ok("first"));
    let backend = FakeBackend::new().with_env("HOST_TOKEN", "second");
    let secrets = vec![
        command_secret("TOKEN", &["/helper"]),
        env_secret("TOKEN", "HOST_TOKEN"),
    ];

    let result = resolve_secrets_with_runner(&secrets, &backend, &runner);
    assert_eq!(result["TOKEN"], "first");
}

#[test]
fn lockdown_does_not_invoke_command_runner() {
    let runner = FakeCommandRunner::with_outcome("/helper", Ok("must-not-run"));
    let secrets = vec![command_secret("TOKEN", &["/helper"])];

    let result = resolve_secrets_for_run(&secrets, &FakeBackend::new(), &runner, true);
    assert!(result.is_empty());
    assert!(runner.calls.borrow().is_empty());
}

#[cfg(unix)]
#[test]
fn os_runner_passes_arguments_without_shell_interpretation() {
    let temp = tempfile::tempdir().unwrap();
    let helper = write_helper(
        temp.path(),
        "argv-helper",
        r#"[ "$#" -eq 2 ] || exit 9
[ "$2" = 'second; value' ] || exit 8
printf '%s' "$1""#,
    );
    let literal = "literal $HOME $(whoami) * ; value";

    let value = OsHostCommandRunner
        .lookup(
            &[
                "/bin/sh".to_owned(),
                helper,
                literal.to_owned(),
                "second; value".to_owned(),
            ],
            Duration::from_secs(1),
        )
        .unwrap();
    assert_eq!(value, literal);
}

#[cfg(unix)]
#[test]
fn os_runner_uses_only_the_minimal_host_environment() {
    let temp = tempfile::tempdir().unwrap();
    let helper = write_helper(
        temp.path(),
        "env-helper",
        r#"[ -n "$PATH" ] || exit 9
[ -z "${CARGO_MANIFEST_DIR+x}" ] || exit 7
printf 'value'"#,
    );

    let value = OsHostCommandRunner
        .lookup(&["/bin/sh".to_owned(), helper], Duration::from_secs(1))
        .unwrap();
    assert_eq!(value, "value");
}

#[cfg(unix)]
#[test]
fn os_runner_does_not_inherit_the_repository_working_directory() {
    let temp = tempfile::tempdir().unwrap();
    let helper = write_helper(temp.path(), "pwd-helper", "printf '%s' \"$PWD\"");
    let expected = dirs::home_dir()
        .filter(|path| path.is_absolute())
        .unwrap_or_else(|| std::path::PathBuf::from("/"));

    let value = OsHostCommandRunner
        .lookup(&["/bin/sh".to_owned(), helper], Duration::from_secs(1))
        .unwrap();
    assert_eq!(Path::new(&value), expected);
}

#[cfg(unix)]
#[test]
fn os_runner_accepts_one_trailing_lf_or_crlf() {
    let temp = tempfile::tempdir().unwrap();
    let lf = write_helper(temp.path(), "lf-helper", "printf 'value\\n'");
    let crlf = write_helper(temp.path(), "crlf-helper", "printf 'value\\r\\n'");

    assert_eq!(
        OsHostCommandRunner
            .lookup(&["/bin/sh".to_owned(), lf], Duration::from_secs(1))
            .unwrap(),
        "value"
    );
    assert_eq!(
        OsHostCommandRunner
            .lookup(&["/bin/sh".to_owned(), crlf], Duration::from_secs(1))
            .unwrap(),
        "value"
    );
}

#[cfg(unix)]
#[test]
fn os_runner_rejects_malformed_output_without_exposing_it() {
    let temp = tempfile::tempdir().unwrap();
    let cases = [
        ("empty", "exit 0", CommandSecretError::EmptyOutput),
        (
            "multiline",
            "printf 'sensitive\\nsecond'",
            CommandSecretError::EmbeddedNewline,
        ),
        (
            "nul",
            "printf 'sensitive\\000value'",
            CommandSecretError::NulByte,
        ),
        (
            "invalid-utf8",
            "printf '\\377'",
            CommandSecretError::InvalidUtf8,
        ),
    ];

    for (name, body, expected) in cases {
        let helper = write_helper(temp.path(), name, body);
        let error = OsHostCommandRunner
            .lookup(&["/bin/sh".to_owned(), helper], Duration::from_secs(1))
            .unwrap_err();
        assert_eq!(error, expected);
        assert!(!error.to_string().contains("sensitive"));
    }
}

#[cfg(unix)]
#[test]
fn os_runner_reports_missing_and_nonzero_without_helper_output() {
    let missing = OsHostCommandRunner
        .lookup(
            &["/definitely/missing/ags-secret-helper".to_owned()],
            Duration::from_secs(1),
        )
        .unwrap_err();
    assert_eq!(
        missing,
        CommandSecretError::Spawn(std::io::ErrorKind::NotFound)
    );

    let temp = tempfile::tempdir().unwrap();
    let helper = write_helper(
        temp.path(),
        "failing-helper",
        "printf 'sensitive-output'; printf 'sensitive-error' >&2; exit 23",
    );
    let error = OsHostCommandRunner
        .lookup(&["/bin/sh".to_owned(), helper], Duration::from_secs(1))
        .unwrap_err();
    assert_eq!(error, CommandSecretError::NonZeroExit(Some(23)));
    let diagnostic = error.to_string();
    assert!(!diagnostic.contains("sensitive-output"));
    assert!(!diagnostic.contains("sensitive-error"));
}

#[cfg(unix)]
#[test]
fn os_runner_kills_and_reaps_timed_out_helper() {
    let temp = tempfile::tempdir().unwrap();
    let pid_file = temp.path().join("pid");
    let helper = write_helper(
        temp.path(),
        "timeout-helper",
        "sleep 10 &\nchild=$!\nprintf '%s %s' \"$$\" \"$child\" > \"$1\"\nwait",
    );
    let error = OsHostCommandRunner
        .lookup(
            &[
                "/bin/sh".to_owned(),
                helper,
                pid_file.to_string_lossy().into_owned(),
            ],
            Duration::from_millis(500),
        )
        .unwrap_err();
    assert_eq!(error, CommandSecretError::TimedOut);

    let pids: Vec<i32> = fs::read_to_string(pid_file)
        .expect("helper should record its process group before timeout")
        .split_whitespace()
        .map(|pid| pid.parse().unwrap())
        .collect();
    assert_eq!(pids.len(), 2);
    for pid in pids {
        assert!(process_is_gone(pid), "timed-out process {pid} still exists");
    }
}

#[cfg(unix)]
#[test]
fn os_runner_bounds_continuous_output_and_kills_descendants() {
    let temp = tempfile::tempdir().unwrap();
    let pid_file = temp.path().join("output-pids");
    let helper = write_helper(
        temp.path(),
        "output-helper",
        "(sleep 0.05; while :; do printf '0123456789abcdef'; done) &\nwriter=$!\nprintf '%s %s' \"$$\" \"$writer\" > \"$1\"\nwait",
    );
    let started = Instant::now();
    let error = OsHostCommandRunner
        .lookup(
            &[
                "/bin/sh".to_owned(),
                helper,
                pid_file.to_string_lossy().into_owned(),
            ],
            Duration::from_secs(2),
        )
        .unwrap_err();
    assert_eq!(error, CommandSecretError::OutputTooLarge);
    assert!(started.elapsed() < Duration::from_secs(2));

    let pids: Vec<i32> = fs::read_to_string(pid_file)
        .expect("helper should record its process group before writing output")
        .split_whitespace()
        .map(|pid| pid.parse().unwrap())
        .collect();
    for pid in pids {
        assert!(process_is_gone(pid), "output process {pid} still exists");
    }
}
