use std::collections::HashMap;
use std::fmt;
use std::io::Read;
use std::process::{Child, ChildStdout, Command, Stdio};
use std::sync::mpsc::{self, TryRecvError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use crate::config::{SecretSource, ValidatedSecret};

pub const COMMAND_SECRET_TIMEOUT: Duration = Duration::from_secs(5);
const COMMAND_POLL_INTERVAL: Duration = Duration::from_millis(10);
const MAX_COMMAND_OUTPUT_BYTES: usize = 64 * 1024;
const COMMAND_ENV_ALLOWLIST: &[&str] = &[
    "PATH",
    "HOME",
    "USER",
    "LOGNAME",
    "DBUS_SESSION_BUS_ADDRESS",
    "XDG_RUNTIME_DIR",
];

/// Abstraction over existing secret backends so tests can mock environment and keyring lookups.
pub trait SecretBackend {
    /// Look up an environment variable by name.
    fn env_var(&self, name: &str) -> Option<String>;

    /// Run `secret-tool lookup` with the given key-value attribute pairs.
    /// Returns `None` if secret-tool is not installed, command fails, or output is empty.
    fn secret_tool_lookup(&self, attributes: &[(&str, &str)]) -> Option<String>;
}

/// Executes a configured command without exposing resolved secrets to its environment.
pub trait HostCommandRunner {
    fn lookup(&self, argv: &[String], timeout: Duration) -> Result<String, CommandSecretError>;
}

/// Structural reasons a command secret source could not resolve a value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommandSecretError {
    EmptyArgv,
    OutputSetup(std::io::ErrorKind),
    Spawn(std::io::ErrorKind),
    Wait(std::io::ErrorKind),
    TimedOut,
    NonZeroExit(Option<i32>),
    OutputRead(std::io::ErrorKind),
    OutputTooLarge,
    EmptyOutput,
    InvalidUtf8,
    NulByte,
    EmbeddedNewline,
}

impl fmt::Display for CommandSecretError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyArgv => f.write_str("empty command argv"),
            Self::OutputSetup(kind) => write!(f, "could not prepare stdout capture ({kind:?})"),
            Self::Spawn(std::io::ErrorKind::NotFound) => f.write_str("executable not found"),
            Self::Spawn(kind) => write!(f, "could not start helper ({kind:?})"),
            Self::Wait(kind) => write!(f, "could not wait for helper ({kind:?})"),
            Self::TimedOut => f.write_str("lookup timed out"),
            Self::NonZeroExit(Some(code)) => write!(f, "helper exited with status {code}"),
            Self::NonZeroExit(None) => f.write_str("helper terminated without an exit status"),
            Self::OutputRead(kind) => write!(f, "could not read helper stdout ({kind:?})"),
            Self::OutputTooLarge => f.write_str("helper stdout exceeded the size limit"),
            Self::EmptyOutput => f.write_str("helper stdout was empty"),
            Self::InvalidUtf8 => f.write_str("helper stdout was not valid UTF-8"),
            Self::NulByte => f.write_str("helper stdout contained a NUL byte"),
            Self::EmbeddedNewline => f.write_str("helper stdout contained an embedded newline"),
        }
    }
}

impl std::error::Error for CommandSecretError {}

/// Real backend for host environment and `secret-tool` lookups.
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

/// Shell-free host command runner with bounded output and process-group containment.
pub struct OsHostCommandRunner;

impl HostCommandRunner for OsHostCommandRunner {
    fn lookup(&self, argv: &[String], timeout: Duration) -> Result<String, CommandSecretError> {
        let executable = argv.first().ok_or(CommandSecretError::EmptyArgv)?;
        let working_dir = dirs::home_dir()
            .filter(|path| path.is_absolute())
            .unwrap_or_else(|| std::path::PathBuf::from("/"));
        let mut command = Command::new(executable);
        command
            .current_dir(working_dir)
            .args(&argv[1..])
            .env_clear()
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        for name in COMMAND_ENV_ALLOWLIST {
            if let Some(value) = std::env::var_os(name) {
                command.env(name, value);
            }
        }
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            command.process_group(0);
        }

        let mut child = command
            .spawn()
            .map_err(|error| CommandSecretError::Spawn(error.kind()))?;
        let process_group = child.id();
        let stdout = child.stdout.take().ok_or(CommandSecretError::OutputSetup(
            std::io::ErrorKind::BrokenPipe,
        ))?;
        let (output_rx, output_reader) = spawn_output_reader(stdout);

        let deadline = Instant::now() + timeout;
        let mut output = None;
        let status = loop {
            match output_rx.try_recv() {
                Ok(Err(CommandSecretError::OutputTooLarge)) => {
                    terminate_and_reap(&mut child, process_group);
                    let _ = output_reader.join();
                    return Err(CommandSecretError::OutputTooLarge);
                }
                Ok(result) => output = Some(result),
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) if output.is_none() => {
                    terminate_and_reap(&mut child, process_group);
                    let _ = output_reader.join();
                    return Err(CommandSecretError::OutputRead(
                        std::io::ErrorKind::BrokenPipe,
                    ));
                }
                Err(TryRecvError::Disconnected) => {}
            }

            match child.try_wait() {
                Ok(Some(status)) => break status,
                Ok(None) if Instant::now() >= deadline => {
                    terminate_and_reap(&mut child, process_group);
                    let _ = output_reader.join();
                    return Err(CommandSecretError::TimedOut);
                }
                Ok(None) => {
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    std::thread::sleep(COMMAND_POLL_INTERVAL.min(remaining));
                }
                Err(error) => {
                    terminate_and_reap(&mut child, process_group);
                    let _ = output_reader.join();
                    return Err(CommandSecretError::Wait(error.kind()));
                }
            }
        };

        kill_process_group(process_group);
        let output = match output {
            Some(result) => result,
            None => output_rx
                .recv()
                .unwrap_or(Err(CommandSecretError::OutputRead(
                    std::io::ErrorKind::BrokenPipe,
                ))),
        };
        let _ = output_reader.join();

        if !status.success() {
            return Err(CommandSecretError::NonZeroExit(status.code()));
        }
        validate_command_output(output?)
    }
}

fn spawn_output_reader(
    stdout: ChildStdout,
) -> (
    mpsc::Receiver<Result<Vec<u8>, CommandSecretError>>,
    JoinHandle<()>,
) {
    let (sender, receiver) = mpsc::sync_channel(1);
    let handle = std::thread::spawn(move || {
        let _ = sender.send(read_bounded_output(stdout));
    });
    (receiver, handle)
}

fn read_bounded_output(stdout: ChildStdout) -> Result<Vec<u8>, CommandSecretError> {
    let mut bytes = Vec::new();
    stdout
        .take((MAX_COMMAND_OUTPUT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|error| CommandSecretError::OutputRead(error.kind()))?;
    if bytes.len() > MAX_COMMAND_OUTPUT_BYTES {
        Err(CommandSecretError::OutputTooLarge)
    } else {
        Ok(bytes)
    }
}

fn terminate_and_reap(child: &mut Child, process_group: u32) {
    kill_process_group(process_group);
    #[cfg(not(unix))]
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(unix)]
fn kill_process_group(process_group: u32) {
    let _ = unsafe { libc::kill(-(process_group as i32), libc::SIGKILL) };
}

#[cfg(not(unix))]
fn kill_process_group(_process_group: u32) {}

fn validate_command_output(mut bytes: Vec<u8>) -> Result<String, CommandSecretError> {
    if bytes.ends_with(b"\r\n") {
        bytes.truncate(bytes.len() - 2);
    } else if bytes.ends_with(b"\n") {
        bytes.pop();
    }
    if bytes.is_empty() {
        return Err(CommandSecretError::EmptyOutput);
    }
    if bytes.contains(&0) {
        return Err(CommandSecretError::NulByte);
    }
    if bytes.iter().any(|byte| matches!(byte, b'\r' | b'\n')) {
        return Err(CommandSecretError::EmbeddedNewline);
    }
    String::from_utf8(bytes).map_err(|_| CommandSecretError::InvalidUtf8)
}

fn try_resolve_one(
    secret: &ValidatedSecret,
    backend: &dyn SecretBackend,
    command_runner: &dyn HostCommandRunner,
) -> Option<String> {
    match &secret.source {
        SecretSource::Env { from_env } => backend.env_var(from_env),
        SecretSource::SecretTool { attributes } => {
            let pairs: Vec<(&str, &str)> = attributes
                .iter()
                .map(|(k, v)| (k.as_str(), v.as_str()))
                .collect();
            backend.secret_tool_lookup(&pairs)
        }
        SecretSource::Command { argv } => {
            match command_runner.lookup(argv, COMMAND_SECRET_TIMEOUT) {
                Ok(value) => Some(value),
                Err(error) => {
                    eprintln!(
                        "warning: command secret lookup for {} failed ({}): {error}",
                        secret.env, secret.origin
                    );
                    None
                }
            }
        }
    }
}

/// Resolve all configured secrets with injected backends.
pub fn resolve_secrets_with_runner(
    secrets: &[ValidatedSecret],
    backend: &dyn SecretBackend,
    command_runner: &dyn HostCommandRunner,
) -> HashMap<String, String> {
    let mut resolved: HashMap<String, String> = HashMap::new();
    for secret in secrets {
        if resolved.contains_key(&secret.env) {
            continue;
        }
        if let Some(value) = try_resolve_one(secret, backend, command_runner) {
            resolved.insert(secret.env.clone(), value);
        }
    }
    resolved
}

/// Resolve configured secrets unless lockdown has disabled all secret sources.
pub fn resolve_secrets_for_run(
    secrets: &[ValidatedSecret],
    backend: &dyn SecretBackend,
    command_runner: &dyn HostCommandRunner,
    lockdown: bool,
) -> HashMap<String, String> {
    if lockdown {
        HashMap::new()
    } else {
        resolve_secrets_with_runner(secrets, backend, command_runner)
    }
}

/// Resolve all configured secrets using the host command runner and existing OS backends.
pub fn resolve_secrets(
    secrets: &[ValidatedSecret],
    backend: &dyn SecretBackend,
) -> HashMap<String, String> {
    resolve_secrets_with_runner(secrets, backend, &OsHostCommandRunner)
}
