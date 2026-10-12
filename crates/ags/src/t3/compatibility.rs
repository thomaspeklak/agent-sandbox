//! Read the desktop's launch/pairing request as data, then dispatch fixed operations.
//! Installer implementation and serialized JS helpers are not a protocol contract:
//! none of the incoming shell, downloaders, or Node probes ever execute.
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
        .map(str::trim)
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

static STOP_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| pattern(STOP, &[("T3_STATE_KEY", "[a-f0-9]{16}")]));
static LOG_PATTERN: LazyLock<Regex> =
    LazyLock::new(|| pattern(LOGS, &[("T3_STATE_KEY", "[a-f0-9]{16}")]));

fn unknown(reason: &str) -> io::Error {
    io::Error::other(format!(
        "unsupported T3 SSH bootstrap: {reason}; AGS could not recognize the desktop request; no incoming script was executed"
    ))
}

fn unquote(value: &str) -> &str {
    for quote in ['\'', '"'] {
        if let Some(value) = value
            .strip_prefix(quote)
            .and_then(|s| s.strip_suffix(quote))
        {
            return value;
        }
    }
    value
}

fn assignment<'a>(script: &'a str, name: &str) -> io::Result<&'a str> {
    let mut values = script.lines().filter_map(|line| {
        let (key, value) = line.split_once('=')?;
        (key == name).then(|| unquote(value.trim()))
    });
    let value = values
        .next()
        .ok_or_else(|| unknown(&format!("missing {name}")))?;
    if values.next().is_some() {
        return Err(unknown(&format!("ambiguous {name}")));
    }
    Ok(value)
}

fn state_key(key: &str) -> bool {
    key.len() == 16
        && key
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

fn ends_with_command(script: &str, command: &str) -> bool {
    script
        .lines()
        .last()
        .is_some_and(|line| line.split_whitespace().eq(command.split_whitespace()))
}

fn runner_version(runner: &str) -> io::Result<String> {
    if !assignment(runner, "T3_NODE_SCRIPT_PATH")?.is_empty() {
        return Err(unknown("development Node runners are not supported"));
    }
    let version = assignment(runner, "T3_ARCHIVE_VERSION")?;
    if !valid_version(version) {
        return Err(unknown("invalid requested runtime version"));
    }
    if assignment(runner, "T3_RUNTIME_DIR")? != "$HOME/.t3/runtime/versions/$T3_ARCHIVE_VERSION"
        || !ends_with_command(runner, "exec \"$T3_RUNTIME_DIR/t3\" \"$@\"")
    {
        return Err(unknown("unrecognized release runtime layout"));
    }
    Ok(version.into())
}

pub fn classify(command: &str, script: &[u8]) -> io::Result<Operation> {
    if script.len() > MAX_SCRIPT_BYTES {
        return Err(unknown("request exceeds the size limit"));
    }
    let script =
        canonical(std::str::from_utf8(script).map_err(|_| unknown("request is not UTF-8"))?);
    let launch_key = command.strip_prefix("sh -l -s -- ");
    let is_launch = launch_key.is_some_and(|key| state_key(unquote(key)));
    if !is_launch && command == "sh -s" {
        if STOP_PATTERN.is_match(&script) {
            return Ok(Operation::Disconnect);
        }
        if LOG_PATTERN.is_match(&script) {
            return Ok(Operation::Logs);
        }
    }
    if !is_launch && command != "sh -s" {
        return Err(unknown("unrecognized SSH command or state key"));
    }
    let marker = if is_launch {
        "cat >\"$RUNNER_NEXT\" <<'SH'\n"
    } else {
        "cat >\"$RUNNER_FILE\" <<'SH'\n"
    };
    let (before, body) = script
        .split_once(marker)
        .ok_or_else(|| unknown("missing runtime runner"))?;
    let (runner, after) = body
        .split_once("\nSH\n")
        .ok_or_else(|| unknown("unterminated runtime runner"))?;
    // Read fields from the envelope, not from opaque embedded helper code.
    let envelope = format!("{before}{after}");
    if assignment(&envelope, "DEFAULT_SERVER_HOME")? != "$HOME/.t3" {
        return Err(unknown("unrecognized server home"));
    }
    if is_launch {
        if assignment(&envelope, "STATE_KEY")? != "$1"
            || assignment(&envelope, "STATE_DIR")? != "$HOME/.t3/ssh-launch/$STATE_KEY"
            || assignment(&envelope, "T3_ARCHIVE_MODE")? != "1"
            || !ends_with_command(
                &envelope,
                "printf '{\"remotePort\":%s,\"serverKind\":\"%s\"}\\n' \"$REMOTE_PORT\" \"${REMOTE_MANAGED:-managed}\"",
            )
        {
            return Err(unknown("unrecognized launch request"));
        }
    } else {
        let state = assignment(&envelope, "STATE_DIR")?;
        if !state
            .strip_prefix("$HOME/.t3/ssh-launch/")
            .is_some_and(state_key)
            || assignment(&envelope, "PAIRING_BASE_DIR")? != "$DEFAULT_SERVER_HOME"
            || !ends_with_command(
                &envelope,
                "\"$RUNNER_FILE\" auth pairing create --base-dir \"$PAIRING_BASE_DIR\" --json",
            )
        {
            return Err(unknown("unrecognized pairing request"));
        }
    }
    let version = runner_version(runner)?;
    Ok(if is_launch {
        Operation::Launch(version)
    } else {
        Operation::Pair(version)
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
