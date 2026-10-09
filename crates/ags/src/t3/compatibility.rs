//! Recognize upstream bootstrap as data, then dispatch typed container operations.
//! Incoming scripts, including their downloaders and Node probes, never execute.
use regex::Regex;
use std::io;
use std::sync::LazyLock;

pub const MAX_SCRIPT_BYTES: usize = 128 * 1024;
pub const RUNNER: &str = include_str!("fixtures/v0.0.45/runner.sh");
pub const LAUNCH: &str = include_str!("fixtures/v0.0.45/launch.sh");
pub const PAIR: &str = include_str!("fixtures/v0.0.45/pair.sh");
pub const STOP: &str = include_str!("fixtures/v0.0.45/stop.sh");
pub const LOGS: &str = include_str!("fixtures/v0.0.45/logs.sh");

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Operation {
    Launch(String),
    Pair(String),
    Disconnect,
    Logs,
}

fn canonical(text: &str) -> String {
    text.lines()
        .filter(|line| !line.trim().is_empty() && !line.trim_start().starts_with('#'))
        .collect::<Vec<_>>()
        .join("\n")
}

fn pattern(template: &str, substitutions: &[(&str, &str)]) -> Regex {
    let mut expression = regex::escape(&canonical(template));
    for (key, value) in substitutions {
        expression = expression.replace(&format!("@@{key}@@"), value);
    }
    Regex::new(&format!("\\A{expression}\\z")).expect("checked-in T3 compatibility expression")
}

static RUNNER_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    pattern(
        RUNNER,
        &[
            ("T3_NODE_ENV_SCRIPT", "(?s:.*?)"),
            ("T3_NODE_SCRIPT_PATH", "''"),
            (
                "T3_ARCHIVE_VERSION",
                "'(?P<version>[0-9]+\\.[0-9]+\\.[0-9]+(?:-[A-Za-z0-9.-]+)?)'",
            ),
            ("T3_RELEASE_BASE_URL", "'[^'\\n]*'"),
        ],
    )
});
static LAUNCH_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    pattern(
        LAUNCH,
        &[
            ("T3_NODE_ENV_SCRIPT", "(?s:.*?)"),
            ("T3_RUNNER_SCRIPT", "(?P<runner>(?s:.*?))"),
            ("T3_PICK_PORT_SCRIPT", "(?s:.*?)"),
            ("T3_WAIT_READY_SCRIPT", "(?s:.*?)"),
            ("T3_RUNTIME_PORT_SCRIPT", "(?s:.*?)"),
        ],
    )
});
static PAIR_PATTERN: LazyLock<Regex> = LazyLock::new(|| {
    pattern(
        PAIR,
        &[
            ("T3_STATE_KEY", "[a-f0-9]{16}"),
            ("T3_RUNNER_SCRIPT", "(?P<runner>(?s:.*?))"),
        ],
    )
});
static STOP_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| pattern(STOP, &[("T3_STATE_KEY", "[a-f0-9]{16}")]));
static LOG_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| pattern(LOGS, &[("T3_STATE_KEY", "[a-f0-9]{16}")]));

pub fn classify(command: &str, script: &[u8]) -> io::Result<Operation> {
    let unknown = || {
        io::Error::other(
            "unsupported T3 SSH bootstrap; AGS supports the v0.0.45 release contract; align desktop/runtime and update AGS (no runtime was downloaded)",
        )
    };
    if script.len() > MAX_SCRIPT_BYTES {
        return Err(unknown());
    }
    let script = canonical(std::str::from_utf8(script).map_err(|_| unknown())?);
    let launch_key = command.strip_prefix("sh -l -s -- ");
    let is_launch = launch_key.is_some_and(|key| {
        key.len() == 16
            && key
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    });
    let form = if is_launch {
        &*LAUNCH_PATTERN
    } else if command == "sh -s" {
        if STOP_PATTERN.is_match(&script) {
            return Ok(Operation::Disconnect);
        }
        if LOG_PATTERN.is_match(&script) {
            return Ok(Operation::Logs);
        }
        &*PAIR_PATTERN
    } else {
        return Err(unknown());
    };
    let captures = form.captures(&script).ok_or_else(unknown)?;
    let runner = captures.name("runner").ok_or_else(unknown)?.as_str();
    let runner = RUNNER_PATTERN.captures(runner).ok_or_else(unknown)?;
    let version = runner.name("version").ok_or_else(unknown)?.as_str();
    if !valid_version(version) {
        return Err(unknown());
    }
    Ok(if is_launch {
        Operation::Launch(version.into())
    } else {
        Operation::Pair(version.into())
    })
}

pub fn valid_version(version: &str) -> bool {
    static VERSION: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(-[A-Za-z0-9-]+(\.[A-Za-z0-9-]+)*)?$")
            .unwrap()
    });
    VERSION.is_match(version)
}

pub fn require_version(requested: &str, mounted: &str, running: &str) -> io::Result<()> {
    if requested == mounted && mounted == running {
        return Ok(());
    }
    Err(io::Error::other(format!(
        "T3 desktop requests {requested}, environment runtime is {mounted} and server is {running}; align versions with `ags update-agents` and explicit `ags t3 upgrade`, or use the matching desktop. No runtime was downloaded."
    )))
}
