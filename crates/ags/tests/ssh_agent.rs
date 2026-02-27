use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use ags::ssh::{AgentState, SshError, SshKey, SshRunner, ensure_agent};

/// Fake SSH runner that tracks calls and returns pre-configured responses.
struct FakeRunner {
    alive_pids: Vec<u32>,
    existing_sockets: Vec<PathBuf>,
    loaded_keys: HashMap<PathBuf, String>,
    pub_keys: HashMap<PathBuf, String>,
    start_agent_result: Mutex<Option<Result<AgentState, String>>>,
    add_key_calls: Mutex<Vec<PathBuf>>,
    add_key_fail: Mutex<Vec<PathBuf>>,
}

impl FakeRunner {
    fn new() -> Self {
        Self {
            alive_pids: Vec::new(),
            existing_sockets: Vec::new(),
            loaded_keys: HashMap::new(),
            pub_keys: HashMap::new(),
            start_agent_result: Mutex::new(None),
            add_key_calls: Mutex::new(Vec::new()),
            add_key_fail: Mutex::new(Vec::new()),
        }
    }

    fn with_alive_pid(mut self, pid: u32) -> Self {
        self.alive_pids.push(pid);
        self
    }

    fn with_socket(mut self, path: &Path) -> Self {
        self.existing_sockets.push(path.to_owned());
        self
    }

    fn with_start_result(self, state: AgentState) -> Self {
        *self.start_agent_result.lock().unwrap() = Some(Ok(state));
        self
    }

    fn with_loaded_key(mut self, sock: &Path, pub_key_line: &str) -> Self {
        self.loaded_keys
            .insert(sock.to_owned(), pub_key_line.to_owned());
        self
    }

    fn with_pub_key(mut self, priv_path: &Path, pub_content: &str) -> Self {
        self.pub_keys
            .insert(priv_path.to_owned(), pub_content.to_owned());
        self
    }

    fn with_add_key_fail(self, key_path: &Path) -> Self {
        self.add_key_fail.lock().unwrap().push(key_path.to_owned());
        self
    }

    fn added_keys(&self) -> Vec<PathBuf> {
        self.add_key_calls.lock().unwrap().clone()
    }
}

impl SshRunner for FakeRunner {
    fn is_pid_alive(&self, pid: u32) -> bool {
        self.alive_pids.contains(&pid)
    }

    fn socket_exists(&self, path: &Path) -> bool {
        self.existing_sockets.contains(&path.to_owned())
    }

    fn start_agent(&self, sock_path: &Path) -> Result<AgentState, SshError> {
        let result = self.start_agent_result.lock().unwrap().take();
        match result {
            Some(Ok(state)) => Ok(state),
            Some(Err(msg)) => Err(SshError::AgentStart(msg)),
            None => Ok(AgentState {
                auth_sock: sock_path.to_owned(),
                pid: 99999,
            }),
        }
    }

    fn list_loaded_keys(&self, auth_sock: &Path) -> Option<String> {
        self.loaded_keys.get(auth_sock).cloned()
    }

    fn read_pub_key(&self, key_path: &Path) -> Option<String> {
        self.pub_keys.get(key_path).cloned()
    }

    fn add_key(&self, _auth_sock: &Path, key_path: &Path) -> Result<(), String> {
        let fails = self.add_key_fail.lock().unwrap();
        if fails.contains(&key_path.to_owned()) {
            return Err("permission denied".into());
        }
        drop(fails);
        self.add_key_calls.lock().unwrap().push(key_path.to_owned());
        Ok(())
    }

    fn remove_socket(&self, _path: &Path) {
        // no-op in tests
    }

    fn kill_socket_owner(&self, _path: &Path) {
        // no-op in tests
    }
}

#[test]
fn reuses_existing_agent_when_alive() {
    let tmp = tempfile::tempdir().unwrap();
    let cache_dir = tmp.path();
    let sock_path = cache_dir.join("ssh-agent.sock");

    // Pre-write an env file
    std::fs::write(
        cache_dir.join("ssh-agent.env"),
        format!(
            "SSH_AUTH_SOCK={}\nSSH_AGENT_PID=1234\n",
            sock_path.display()
        ),
    )
    .unwrap();

    let runner = FakeRunner::new()
        .with_alive_pid(1234)
        .with_socket(&sock_path);

    let result = ensure_agent(cache_dir, &[], &runner).unwrap();
    assert_eq!(result.auth_sock, sock_path);
    assert!(result.warnings.is_empty());
}

#[test]
fn starts_new_agent_when_no_env_file() {
    let tmp = tempfile::tempdir().unwrap();
    let cache_dir = tmp.path();
    let sock_path = cache_dir.join("ssh-agent.sock");

    let runner = FakeRunner::new().with_start_result(AgentState {
        auth_sock: sock_path.clone(),
        pid: 5678,
    });

    let result = ensure_agent(cache_dir, &[], &runner).unwrap();
    assert_eq!(result.auth_sock, sock_path);

    // Verify env file was written
    let env_content = std::fs::read_to_string(cache_dir.join("ssh-agent.env")).unwrap();
    assert!(env_content.contains("SSH_AGENT_PID=5678"));
}

#[test]
fn starts_new_agent_when_pid_dead() {
    let tmp = tempfile::tempdir().unwrap();
    let cache_dir = tmp.path();
    let sock_path = cache_dir.join("ssh-agent.sock");

    // Stale env file with dead PID
    std::fs::write(
        cache_dir.join("ssh-agent.env"),
        format!(
            "SSH_AUTH_SOCK={}\nSSH_AGENT_PID=9999\n",
            sock_path.display()
        ),
    )
    .unwrap();

    let runner = FakeRunner::new().with_start_result(AgentState {
        auth_sock: sock_path.clone(),
        pid: 7777,
    });

    let result = ensure_agent(cache_dir, &[], &runner).unwrap();
    assert_eq!(result.auth_sock, sock_path);

    let env_content = std::fs::read_to_string(cache_dir.join("ssh-agent.env")).unwrap();
    assert!(env_content.contains("SSH_AGENT_PID=7777"));
}

#[test]
fn loads_key_when_not_present_in_agent() {
    let tmp = tempfile::tempdir().unwrap();
    let cache_dir = tmp.path();
    let sock_path = cache_dir.join("ssh-agent.sock");

    // Create a fake key file (non-empty)
    let key_path = cache_dir.join("test-key");
    std::fs::write(&key_path, "PRIVATE KEY DATA").unwrap();

    let runner = FakeRunner::new()
        .with_start_result(AgentState {
            auth_sock: sock_path.clone(),
            pid: 1111,
        })
        .with_pub_key(&key_path, "ssh-ed25519 AAAAC3... test@host")
        .with_loaded_key(&sock_path, "ssh-rsa AAAAB3... other@host");

    let keys = vec![SshKey {
        private_path: key_path.clone(),
        label: "auth".into(),
    }];

    let result = ensure_agent(cache_dir, &keys, &runner).unwrap();
    assert!(result.warnings.is_empty());
    assert_eq!(runner.added_keys(), vec![key_path]);
}

#[test]
fn skips_key_already_loaded() {
    let tmp = tempfile::tempdir().unwrap();
    let cache_dir = tmp.path();
    let sock_path = cache_dir.join("ssh-agent.sock");

    let key_path = cache_dir.join("test-key");
    std::fs::write(&key_path, "PRIVATE KEY DATA").unwrap();

    let pub_key = "ssh-ed25519 AAAAC3... test@host";
    let runner = FakeRunner::new()
        .with_start_result(AgentState {
            auth_sock: sock_path.clone(),
            pid: 2222,
        })
        .with_pub_key(&key_path, pub_key)
        .with_loaded_key(&sock_path, pub_key);

    let keys = vec![SshKey {
        private_path: key_path,
        label: "auth".into(),
    }];

    let result = ensure_agent(cache_dir, &keys, &runner).unwrap();
    assert!(result.warnings.is_empty());
    assert!(runner.added_keys().is_empty());
}

#[test]
fn warns_on_empty_key_file() {
    let tmp = tempfile::tempdir().unwrap();
    let cache_dir = tmp.path();
    let sock_path = cache_dir.join("ssh-agent.sock");

    // Create empty key file
    let key_path = cache_dir.join("empty-key");
    std::fs::write(&key_path, "").unwrap();

    let runner = FakeRunner::new().with_start_result(AgentState {
        auth_sock: sock_path,
        pid: 3333,
    });

    let keys = vec![SshKey {
        private_path: key_path,
        label: "auth".into(),
    }];

    let result = ensure_agent(cache_dir, &keys, &runner).unwrap();
    assert_eq!(result.warnings.len(), 1);
    assert!(result.warnings[0].contains("empty"));
}

#[test]
fn warns_on_add_key_failure() {
    let tmp = tempfile::tempdir().unwrap();
    let cache_dir = tmp.path();
    let sock_path = cache_dir.join("ssh-agent.sock");

    let key_path = cache_dir.join("bad-key");
    std::fs::write(&key_path, "PRIVATE KEY DATA").unwrap();

    let runner = FakeRunner::new()
        .with_start_result(AgentState {
            auth_sock: sock_path,
            pid: 4444,
        })
        .with_add_key_fail(&key_path);

    let keys = vec![SshKey {
        private_path: key_path,
        label: "auth".into(),
    }];

    let result = ensure_agent(cache_dir, &keys, &runner).unwrap();
    assert_eq!(result.warnings.len(), 1);
    assert!(result.warnings[0].contains("failed to add"));
}
