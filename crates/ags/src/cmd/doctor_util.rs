use std::fs;
use std::path::Path;
use std::process::Command;

/// Accumulator for OK / WARN / FAIL health-check results with color output.
pub struct Checker {
    pub ok_count: u32,
    pub warn_count: u32,
    pub fail_count: u32,
    use_color: bool,
}

impl Checker {
    pub fn new() -> Self {
        Self {
            ok_count: 0,
            warn_count: 0,
            fail_count: 0,
            use_color: atty_stdout(),
        }
    }

    pub fn section(&self, title: &str) {
        if self.use_color {
            println!("\n\x1b[36m== {title} ==\x1b[0m");
        } else {
            println!("\n== {title} ==");
        }
    }

    pub fn ok(&mut self, msg: &str) {
        self.ok_count += 1;
        if self.use_color {
            println!("\x1b[32m[OK]\x1b[0m {msg}");
        } else {
            println!("[OK] {msg}");
        }
    }

    pub fn warn(&mut self, msg: &str) {
        self.warn_count += 1;
        if self.use_color {
            println!("\x1b[33m[WARN]\x1b[0m {msg}");
        } else {
            println!("[WARN] {msg}");
        }
    }

    pub fn fail(&mut self, msg: &str) {
        self.fail_count += 1;
        if self.use_color {
            println!("\x1b[31m[FAIL]\x1b[0m {msg}");
        } else {
            println!("[FAIL] {msg}");
        }
    }

    pub fn print_summary(&self) {
        if self.use_color {
            println!(
                "\n\x1b[36mSummary:\x1b[0m \
                 \x1b[32m{} ok\x1b[0m, \
                 \x1b[33m{} warnings\x1b[0m, \
                 \x1b[31m{} failures\x1b[0m",
                self.ok_count, self.warn_count, self.fail_count
            );
        } else {
            println!(
                "\nSummary: {} ok, {} warnings, {} failures",
                self.ok_count, self.warn_count, self.fail_count
            );
        }
    }
}

// ── System probes ────────────────────────────────────────────────────

pub fn has_command(name: &str) -> bool {
    Command::new("which")
        .arg(name)
        .output()
        .is_ok_and(|o| o.status.success())
}

pub fn check_required_cmd(ck: &mut Checker, cmd: &str) {
    if has_command(cmd) {
        ck.ok(&format!("binary available: {cmd}"));
    } else {
        ck.fail(&format!("missing required binary: {cmd}"));
    }
}

pub fn check_optional_cmd(ck: &mut Checker, cmd: &str) {
    if has_command(cmd) {
        ck.ok(&format!("optional binary available: {cmd}"));
    } else {
        ck.warn(&format!("optional binary missing: {cmd}"));
    }
}

pub fn git_config_get(gitconfig: &Path, key: &str) -> Option<String> {
    let output = Command::new("git")
        .args(["config", "-f"])
        .arg(gitconfig)
        .args(["--get", key])
        .output()
        .ok()?;
    if output.status.success() {
        let val = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        if val.is_empty() { None } else { Some(val) }
    } else {
        None
    }
}

pub fn podman_image_exists(image: &str) -> bool {
    Command::new("podman")
        .args(["image", "exists", image])
        .status()
        .is_ok_and(|s| s.success())
}

pub fn file_non_empty(path: &Path) -> bool {
    fs::metadata(path).is_ok_and(|m| m.len() > 0)
}

pub fn pub_key_path(key_path: &Path) -> std::path::PathBuf {
    let mut p = key_path.as_os_str().to_owned();
    p.push(".pub");
    std::path::PathBuf::from(p)
}

pub fn read_agent_env(path: &Path) -> Option<(String, u32)> {
    let content = fs::read_to_string(path).ok()?;
    let mut sock = None;
    let mut pid = None;
    for line in content.lines() {
        let line = line.trim();
        if let Some(val) = line.strip_prefix("SSH_AUTH_SOCK=") {
            sock = Some(val.to_owned());
        } else if let Some(val) = line.strip_prefix("SSH_AGENT_PID=") {
            pid = val.parse().ok();
        }
    }
    match (sock, pid) {
        (Some(s), Some(p)) => Some((s, p)),
        _ => None,
    }
}

pub fn is_pid_alive(pid: u32) -> bool {
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

pub fn socket_exists(path: &Path) -> bool {
    use std::os::unix::fs::FileTypeExt;
    path.symlink_metadata()
        .is_ok_and(|m| m.file_type().is_socket())
}

pub fn list_agent_keys(sock_path: &Path) -> Option<String> {
    let output = Command::new("ssh-add")
        .arg("-L")
        .env("SSH_AUTH_SOCK", sock_path)
        .output()
        .ok()?;
    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        None
    }
}

pub fn secret_tool_has_value(attributes: &std::collections::BTreeMap<String, String>) -> bool {
    if !has_command("secret-tool") || attributes.is_empty() {
        return false;
    }
    let mut args = vec!["lookup".to_owned()];
    for (k, v) in attributes {
        args.push(k.clone());
        args.push(v.clone());
    }
    Command::new("secret-tool")
        .args(&args)
        .output()
        .is_ok_and(|o| o.status.success() && !o.stdout.is_empty())
}

pub fn is_executable(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        path.metadata()
            .is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
    }
    #[cfg(not(unix))]
    {
        path.exists()
    }
}

pub fn is_port_open(port: u16) -> bool {
    use std::net::TcpStream;
    use std::time::Duration;
    TcpStream::connect_timeout(
        &format!("127.0.0.1:{port}").parse().unwrap(),
        Duration::from_secs(1),
    )
    .is_ok()
}

fn atty_stdout() -> bool {
    unsafe { libc::isatty(libc::STDOUT_FILENO) != 0 }
}
