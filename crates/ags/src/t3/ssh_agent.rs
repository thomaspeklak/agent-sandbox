use crate::ssh::{AgentState, OsSshRunner, SshError, SshKey, SshRunner};
use std::path::{Path, PathBuf};

struct Noninteractive;
impl SshRunner for Noninteractive {
    fn is_pid_alive(&self, pid: u32) -> bool {
        OsSshRunner.is_pid_alive(pid)
    }
    fn socket_exists(&self, path: &Path) -> bool {
        OsSshRunner.socket_exists(path)
    }
    fn start_agent(&self, path: &Path) -> Result<AgentState, SshError> {
        OsSshRunner.start_agent(path)
    }
    fn list_loaded_keys(&self, path: &Path) -> Option<String> {
        OsSshRunner.list_loaded_keys(path)
    }
    fn read_pub_key(&self, path: &Path) -> Option<String> {
        OsSshRunner.read_pub_key(path)
    }
    fn remove_socket(&self, path: &Path) {
        OsSshRunner.remove_socket(path);
    }
    fn kill_socket_owner(&self, path: &Path) {
        OsSshRunner.kill_socket_owner(path);
    }
    fn add_key(&self, socket: &Path, key: &Path) -> Result<(), String> {
        let status = std::process::Command::new("timeout")
            .args(["5", "ssh-add", "-q"])
            .arg(key)
            .env("SSH_AUTH_SOCK", socket)
            .env("SSH_ASKPASS", "/bin/false")
            .env("SSH_ASKPASS_REQUIRE", "never")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map_err(|error| error.to_string())?;
        if status.success() {
            Ok(())
        } else {
            Err("load SSH keys interactively with `ags setup`/ssh-add before reconnecting".into())
        }
    }
}

pub fn prepare(config: &crate::config::ValidatedConfig) -> Option<PathBuf> {
    let keys = [
        SshKey {
            private_path: config.sandbox.auth_key.clone(),
            label: "auth".into(),
        },
        SshKey {
            private_path: config.sandbox.sign_key.clone(),
            label: "signing".into(),
        },
    ];
    match crate::ssh::ensure_agent(&config.sandbox.cache_dir, &keys, &Noninteractive) {
        Ok(ready) => {
            for warning in ready.warnings {
                eprintln!("warning: {warning}");
            }
            Some(ready.auth_sock)
        }
        Err(error) => {
            eprintln!("warning: T3 SSH agent: {error}");
            None
        }
    }
}
