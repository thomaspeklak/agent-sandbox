use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

/// Persisted SSH agent state read from / written to the env file.
#[derive(Debug, Clone)]
pub struct AgentState {
    pub auth_sock: PathBuf,
    pub pid: u32,
}

/// Result of ensuring the SSH agent is running and keys are loaded.
#[derive(Debug)]
pub struct SshAgentReady {
    pub auth_sock: PathBuf,
    pub warnings: Vec<String>,
}

#[derive(Debug)]
pub enum SshError {
    AgentStart(String),
    EnvFileParse(String),
}

impl fmt::Display for SshError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AgentStart(msg) => write!(f, "failed to start ssh-agent: {msg}"),
            Self::EnvFileParse(msg) => write!(f, "failed to parse agent env file: {msg}"),
        }
    }
}

impl std::error::Error for SshError {}

/// Abstraction over SSH process operations for testability.
pub trait SshRunner {
    /// Check whether a process with the given PID is alive.
    fn is_pid_alive(&self, pid: u32) -> bool;

    /// Check whether a Unix socket exists at the given path.
    fn socket_exists(&self, path: &Path) -> bool;

    /// Start a new ssh-agent bound to the given socket path.
    /// Returns the parsed `AgentState` on success.
    fn start_agent(&self, sock_path: &Path) -> Result<AgentState, SshError>;

    /// List public keys currently loaded in the agent at the given socket.
    /// Returns the full output of `ssh-add -L`.
    fn list_loaded_keys(&self, auth_sock: &Path) -> Option<String>;

    /// Read the public key file content (the `.pub` companion).
    fn read_pub_key(&self, key_path: &Path) -> Option<String>;

    /// Add a private key to the agent.
    /// Returns `Ok(())` on success, `Err(message)` on failure.
    fn add_key(&self, auth_sock: &Path, key_path: &Path) -> Result<(), String>;

    /// Remove a socket file.
    fn remove_socket(&self, path: &Path);

    /// Kill any process bound to the given socket path.
    fn kill_socket_owner(&self, path: &Path);
}

/// Real implementation that shells out to ssh-agent / ssh-add.
pub struct OsSshRunner;

impl SshRunner for OsSshRunner {
    fn is_pid_alive(&self, pid: u32) -> bool {
        // kill -0 checks process existence without sending a signal
        unsafe { libc::kill(pid as i32, 0) == 0 }
    }

    fn socket_exists(&self, path: &Path) -> bool {
        use std::os::unix::fs::FileTypeExt;
        path.symlink_metadata()
            .map(|m| m.file_type().is_socket())
            .unwrap_or(false)
    }

    fn start_agent(&self, sock_path: &Path) -> Result<AgentState, SshError> {
        let output = std::process::Command::new("ssh-agent")
            .args(["-s", "-a"])
            .arg(sock_path)
            .output()
            .map_err(|e| SshError::AgentStart(e.to_string()))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(SshError::AgentStart(stderr.into_owned()));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        parse_agent_output(&stdout, sock_path)
    }

    fn list_loaded_keys(&self, auth_sock: &Path) -> Option<String> {
        let output = std::process::Command::new("ssh-add")
            .arg("-L")
            .env("SSH_AUTH_SOCK", auth_sock)
            .output()
            .ok()?;

        if output.status.success() {
            Some(String::from_utf8_lossy(&output.stdout).into_owned())
        } else {
            None
        }
    }

    fn read_pub_key(&self, key_path: &Path) -> Option<String> {
        let pub_path = key_path.with_extension(format!(
            "{}.pub",
            key_path
                .extension()
                .map(|e| e.to_string_lossy())
                .unwrap_or_default()
        ));
        // Try <key>.pub first, then <key_path>.pub (for paths without extension)
        let pub_path = if pub_path.exists() {
            pub_path
        } else {
            let mut p = key_path.as_os_str().to_owned();
            p.push(".pub");
            PathBuf::from(p)
        };
        fs::read_to_string(&pub_path)
            .ok()
            .map(|s| s.trim().to_owned())
    }

    fn add_key(&self, auth_sock: &Path, key_path: &Path) -> Result<(), String> {
        // Inherit stdio so ssh-add can prompt for passphrases interactively
        let status = std::process::Command::new("ssh-add")
            .arg(key_path)
            .env("SSH_AUTH_SOCK", auth_sock)
            .stdin(std::process::Stdio::inherit())
            .stdout(std::process::Stdio::inherit())
            .stderr(std::process::Stdio::inherit())
            .status()
            .map_err(|e| e.to_string())?;

        if status.success() {
            Ok(())
        } else {
            Err(format!("ssh-add exited with {status}"))
        }
    }

    fn remove_socket(&self, path: &Path) {
        let _ = fs::remove_file(path);
    }

    fn kill_socket_owner(&self, path: &Path) {
        // fuser -k sends SIGKILL to any process holding the socket
        let _ = std::process::Command::new("fuser")
            .arg("-k")
            .arg(path)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status();
    }
}

/// Parse `ssh-agent -s` output to extract PID.
/// Output looks like:
/// ```text
/// SSH_AUTH_SOCK=/path/to/sock; export SSH_AUTH_SOCK;
/// SSH_AGENT_PID=12345; export SSH_AGENT_PID;
/// echo Agent pid 12345;
/// ```
fn parse_agent_output(stdout: &str, sock_path: &Path) -> Result<AgentState, SshError> {
    let mut pid: Option<u32> = None;

    for line in stdout.lines() {
        if let Some(rest) = line.strip_prefix("SSH_AGENT_PID=") {
            // `ssh-agent -s` prints: `SSH_AGENT_PID=12345; export SSH_AGENT_PID;`
            // Keep only the assignment value before the first `;`.
            let val = rest.split(';').next().unwrap_or(rest).trim();
            pid = val.parse().ok();
        }
    }

    match pid {
        Some(p) => Ok(AgentState {
            auth_sock: sock_path.to_owned(),
            pid: p,
        }),
        None => Err(SshError::AgentStart(
            "could not parse SSH_AGENT_PID from ssh-agent output".into(),
        )),
    }
}

/// Read cached agent state from the env file.
fn read_agent_env(env_path: &Path) -> Option<AgentState> {
    let content = fs::read_to_string(env_path).ok()?;
    let mut sock: Option<PathBuf> = None;
    let mut pid: Option<u32> = None;

    for line in content.lines() {
        let line = line.trim();
        if let Some(val) = line.strip_prefix("SSH_AUTH_SOCK=") {
            sock = Some(PathBuf::from(val));
        } else if let Some(val) = line.strip_prefix("SSH_AGENT_PID=") {
            pid = val.parse().ok();
        }
    }

    match (sock, pid) {
        (Some(s), Some(p)) => Some(AgentState {
            auth_sock: s,
            pid: p,
        }),
        _ => None,
    }
}

/// Write agent state to the env file.
fn write_agent_env(env_path: &Path, state: &AgentState) -> std::io::Result<()> {
    let content = format!(
        "SSH_AUTH_SOCK={}\nSSH_AGENT_PID={}\n",
        state.auth_sock.display(),
        state.pid
    );
    if let Some(parent) = env_path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(env_path, content)
}

/// Key to load into the agent.
pub struct SshKey {
    pub private_path: PathBuf,
    pub label: String,
}

/// Ensure a dedicated SSH agent is running and the given keys are loaded.
///
/// - `cache_dir`: directory for agent socket and env file
/// - `keys`: private key paths to load (each with a label for warnings)
///
/// Returns the socket path to mount into the container.
pub fn ensure_agent(
    cache_dir: &Path,
    keys: &[SshKey],
    runner: &dyn SshRunner,
) -> Result<SshAgentReady, SshError> {
    let env_path = cache_dir.join("ssh-agent.env");
    let sock_path = cache_dir.join("ssh-agent.sock");
    let mut warnings = Vec::new();

    // Try to reuse existing agent
    let state = match read_agent_env(&env_path) {
        Some(cached)
            if runner.is_pid_alive(cached.pid) && runner.socket_exists(&cached.auth_sock) =>
        {
            cached
        }
        _ => {
            // Kill any orphaned agent still bound to the socket, then start fresh
            runner.kill_socket_owner(&sock_path);
            runner.remove_socket(&sock_path);
            let new_state = runner.start_agent(&sock_path)?;
            if let Err(e) = write_agent_env(&env_path, &new_state) {
                warnings.push(format!("could not persist agent env: {e}"));
            }
            new_state
        }
    };

    // Load keys
    for key in keys {
        load_key_if_needed(&state, key, runner, &mut warnings);
    }

    Ok(SshAgentReady {
        auth_sock: state.auth_sock,
        warnings,
    })
}

fn load_key_if_needed(
    state: &AgentState,
    key: &SshKey,
    runner: &dyn SshRunner,
    warnings: &mut Vec<String>,
) {
    if !key.private_path.exists() {
        return;
    }

    // Check file is non-empty
    let meta = match fs::metadata(&key.private_path) {
        Ok(m) => m,
        Err(_) => return,
    };
    if meta.len() == 0 {
        warnings.push(format!(
            "{} key file is empty: {}",
            key.label,
            key.private_path.display()
        ));
        return;
    }

    // Check if already loaded by comparing public key
    if let Some(pub_key) = runner.read_pub_key(&key.private_path)
        && let Some(loaded) = runner.list_loaded_keys(&state.auth_sock)
        && loaded.lines().any(|line| line.trim() == pub_key)
    {
        return; // Already loaded
    }

    // Add key
    if let Err(e) = runner.add_key(&state.auth_sock, &key.private_path) {
        warnings.push(format!(
            "failed to add {} key {}: {}",
            key.label,
            key.private_path.display(),
            e.trim()
        ));
    }
}
