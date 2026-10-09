use super::Hook;
use sha2::{Digest, Sha256};
use std::fs::{self, OpenOptions};
use std::io::{IsTerminal, Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::PathBuf;

pub struct TrustStore {
    pub path: PathBuf,
}
impl Default for TrustStore {
    fn default() -> Self {
        Self {
            path: dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("/nonexistent"))
                .join(".local/state/ags-hook-trust"),
        }
    }
}
impl TrustStore {
    fn directory(&self) -> Result<(), String> {
        if !self.path.exists() {
            if let Some(parent) = self.path.parent() {
                fs::create_dir_all(parent).map_err(|e| e.to_string())?;
            }
            match std::fs::DirBuilder::new().mode(0o700).create(&self.path) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(e.to_string()),
            }
        }
        secure_metadata(&self.path, true)
    }
    pub fn ensure(&self, hook: &Hook, interactive: bool) -> Result<(), String> {
        let (hash, _) = fingerprint(hook)?;
        if self.approved(&hash)? {
            return Ok(());
        }
        let instruction = format!(
            "hook {:?} is new or changed; run `ags hooks test {} --agent shell` in an interactive terminal (with the same --config) to review and approve host-user execution",
            hook.name, hook.name
        );
        if !interactive || !std::io::stdin().is_terminal() || !std::io::stderr().is_terminal() {
            return Err(instruction);
        }
        eprintln!(
            "Prepare hook {:?}\nExecutable: {:?}\nArguments: {:?}\nProject scope: {:?}\nSHA-256: {hash}\nApproval grants execution as your HOST USER, with external side effects. Only the entrypoint and declaration/argv are tracked. Interpreters, libraries, imports, scripts passed as arguments, and other resources are NOT content tracked. Declare a directly executable script to track its own changes.\nApprove? [y/N]",
            hook.name, hook.executable, hook.args, hook.project
        );
        std::io::stderr().flush().map_err(|e| e.to_string())?;
        let mut answer = String::new();
        std::io::stdin()
            .read_line(&mut answer)
            .map_err(|e| e.to_string())?;
        if !matches!(answer.trim().to_ascii_lowercase().as_str(), "y" | "yes") {
            return Err(instruction);
        }
        // Do not approve code that changed while the operator reviewed it.
        if fingerprint(hook)?.0 != hash {
            return Err("hook changed during approval; review again".into());
        }
        self.record(&hash)
    }
    pub(crate) fn approved(&self, hash: &str) -> Result<bool, String> {
        self.directory()?;
        let token = self.path.join(hash);
        let mut file = match OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(&token)
        {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(e) => return Err(format!("cannot read hook approval: {e}")),
        };
        secure_file_metadata(&file.metadata().map_err(|e| e.to_string())?, false)?;
        let mut value = String::new();
        Read::by_ref(&mut file)
            .take(65)
            .read_to_string(&mut value)
            .map_err(|e| e.to_string())?;
        Ok(value == hash)
    }
    pub(crate) fn record(&self, hash: &str) -> Result<(), String> {
        self.directory()?;
        let token = self.path.join(hash);
        let mut file = match OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(token)
        {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists && self.approved(hash)? => {
                return Ok(());
            }
            Err(e) => return Err(format!("cannot save hook approval: {e}")),
        };
        file.write_all(hash.as_bytes())
            .and_then(|_| file.sync_all())
            .map_err(|e| e.to_string())
    }
    /// Keep executable copies off system temp mounts (which may be noexec).
    pub(crate) fn execution_directory(&self) -> Result<tempfile::TempDir, String> {
        self.directory()?;
        tempfile::Builder::new()
            .prefix("ags-hook-exec-")
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir_in(&self.path)
            .map_err(|e| format!("cannot create private hook execution directory: {e}"))
    }
    pub(crate) fn snapshot(
        &self,
        hook: &Hook,
        directory: &std::path::Path,
    ) -> Result<(PathBuf, String), String> {
        let (hash, bytes) = fingerprint(hook)?;
        if !self.approved(&hash)? {
            return Err(format!(
                "hook {:?} changed or is unapproved; review with ags hooks test",
                hook.name
            ));
        }
        let path = directory.join(
            hook.executable
                .file_name()
                .ok_or("hook executable has no filename")?,
        );
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o700)
            .open(&path)
            .map_err(|e| e.to_string())?;
        file.write_all(&bytes).map_err(|e| e.to_string())?;
        drop(file);
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).map_err(|e| e.to_string())?;
        Ok((path, hash))
    }
    pub(crate) fn check_snapshot(&self, hook: &Hook, expected_hash: &str) -> Result<(), String> {
        let (current_hash, _) = fingerprint(hook)?;
        if current_hash != expected_hash || !self.approved(&current_hash)? {
            return Err(format!(
                "hook {:?} changed after snapshotting; review with ags hooks test and retry the launch",
                hook.name
            ));
        }
        Ok(())
    }
}
use std::os::unix::fs::DirBuilderExt;
fn secure_metadata(path: &std::path::Path, directory: bool) -> Result<(), String> {
    let meta = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    secure_file_metadata(&meta, directory)
}
fn secure_file_metadata(meta: &fs::Metadata, directory: bool) -> Result<(), String> {
    if meta.uid() != unsafe { libc::geteuid() }
        || meta.mode() & 0o077 != 0
        || (directory && !meta.is_dir())
        || (!directory && !meta.is_file())
    {
        return Err("hook trust storage must be owned by the current user, private (0700 directory/0600 file), and not a symlink".into());
    }
    Ok(())
}
pub(crate) fn fingerprint(hook: &Hook) -> Result<(String, Vec<u8>), String> {
    // Opening a FIFO must not block before descriptor-based validation. Follow
    // symlinks as before; hash and snapshot only bytes from this validated file.
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(&hook.executable)
        .map_err(|e| format!("hook {:?}: cannot read entrypoint: {e}", hook.name))?;
    let meta = file.metadata().map_err(|e| e.to_string())?;
    if !meta.is_file() || meta.mode() & 0o111 == 0 || meta.len() > 64 * 1024 * 1024 {
        return Err("hook entrypoint must be an executable regular file at most 64 MiB".into());
    }
    let mut bytes = Vec::new();
    file.take(64 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > 64 * 1024 * 1024 {
        return Err("hook entrypoint exceeds 64 MiB".into());
    }
    let mut hash = Sha256::new();
    hash.update(b"ags-prepare-hook-trust-v1\0");
    let declaration = serde_json::to_vec(hook).map_err(|e| e.to_string())?;
    hash.update((declaration.len() as u64).to_le_bytes());
    hash.update(declaration);
    let scope = serde_json::to_vec(&hook.project).map_err(|e| e.to_string())?;
    hash.update((scope.len() as u64).to_le_bytes());
    hash.update(scope);
    hash.update((bytes.len() as u64).to_le_bytes());
    hash.update(&bytes);
    Ok((format!("{:x}", hash.finalize()), bytes))
}
