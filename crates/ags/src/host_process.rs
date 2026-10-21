//! Bounded shell-free host execution shared by hooks and op inject (Unix).
use std::io::{Read, Write};
use std::os::fd::AsRawFd;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

pub(crate) static INTERRUPTED: AtomicBool = AtomicBool::new(false);
pub(crate) struct Captured {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub failure: Option<String>,
}
impl Captured {
    pub(crate) fn check_status(&self) -> Result<(), String> {
        self.failure.clone().map_or(Ok(()), Err)
    }
}
struct Process {
    child: Child,
    group: u32,
}
impl Drop for Process {
    fn drop(&mut self) {
        unsafe {
            libc::kill(-(self.group as i32), libc::SIGKILL);
        }
        let _ = self.child.wait();
    }
}
fn nonblocking(pipe: &impl AsRawFd) -> Result<(), String> {
    let fd = pipe.as_raw_fd();
    if unsafe { libc::fcntl(fd, libc::F_SETFL, libc::O_NONBLOCK) } < 0 {
        return Err("cannot set nonblocking host pipe".into());
    }
    Ok(())
}
fn drain(pipe: &mut impl Read, bytes: &mut Vec<u8>, limit: usize) -> Result<(), String> {
    let mut buffer = [0; 8192];
    // Bound each turn even if a continuously writing child never fills the pipe.
    for _ in 0..16 {
        match pipe.read(&mut buffer) {
            Ok(0) => return Ok(()),
            Ok(size) => {
                let keep = size.min(limit.saturating_sub(bytes.len()));
                bytes.extend_from_slice(&buffer[..keep]);
                if keep < size {
                    return Err("host output exceeded limit".into());
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => return Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => return Err("host output read failed".into()),
        }
    }
    Ok(())
}

pub(crate) fn capture(
    command: &mut Command,
    input: Vec<u8>,
    deadline: Instant,
    cancel: &AtomicBool,
    stdout_limit: usize,
    stderr_limit: usize,
) -> Result<Captured, String> {
    use std::os::unix::process::CommandExt;
    if Instant::now() >= deadline {
        return Err("host execution timed out before spawn".into());
    }
    if cancel.load(Ordering::SeqCst) || INTERRUPTED.load(Ordering::SeqCst) {
        return Err("host execution cancelled".into());
    }
    command
        .process_group(0)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let child = command
        .spawn()
        .map_err(|e| format!("could not execute host program: {e}"))?;
    let mut process = Process {
        group: child.id(),
        child,
    };
    let mut stdin = process.child.stdin.take();
    let mut stdout = process.child.stdout.take().ok_or("stdout unavailable")?;
    let mut stderr = process.child.stderr.take().ok_or("stderr unavailable")?;
    nonblocking(stdin.as_ref().ok_or("stdin unavailable")?)?;
    nonblocking(&stdout)?;
    nonblocking(&stderr)?;
    let mut output = Captured {
        stdout: Vec::new(),
        stderr: Vec::new(),
        failure: None,
    };
    let mut written = 0;
    let result: Result<(), String> = (|| loop {
        if cancel.load(Ordering::SeqCst) || INTERRUPTED.load(Ordering::SeqCst) {
            break Err("host execution cancelled".into());
        }
        if Instant::now() >= deadline {
            break Err("host execution timed out".into());
        }
        drain(&mut stdout, &mut output.stdout, stdout_limit)?;
        drain(&mut stderr, &mut output.stderr, stderr_limit)?;
        if written == input.len() {
            stdin.take();
        }
        if let Some(pipe) = stdin.as_mut() {
            match pipe.write(&input[written..]) {
                Ok(n) => written += n,
                Err(e)
                    if matches!(
                        e.kind(),
                        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted
                    ) => {}
                Err(_) => {
                    stdin.take();
                }
            }
        }
        match process.child.try_wait() {
            Ok(Some(status)) => {
                break if !status.success() {
                    Err(format!("host program failed (status {status})"))
                } else if written < input.len() {
                    Err("host program did not consume context".into())
                } else {
                    Ok(())
                };
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(5)),
            Err(_) => break Err("could not wait for host program".into()),
        }
    })();
    drop(process); // Terminate the group even on success; no dependent background work.
    drop(stdin);
    // Drain buffered bytes without waiting for EOF from escaped/daemonized descendants.
    let final_out = drain(&mut stdout, &mut output.stdout, stdout_limit);
    let final_err = drain(&mut stderr, &mut output.stderr, stderr_limit);
    output.failure = result.and(final_out).and(final_err).err();
    Ok(output)
}

static SIGNAL_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
extern "C" fn interrupted(_: libc::c_int) {
    INTERRUPTED.store(true, Ordering::SeqCst);
}
/// Temporary terminal cancellation handling while host children exist.
pub(crate) struct SignalGuard {
    old: Vec<(libc::c_int, libc::sigaction)>,
    _lock: std::sync::MutexGuard<'static, ()>,
}
impl SignalGuard {
    pub(crate) fn install() -> Result<Self, String> {
        let lock = SIGNAL_LOCK.lock().map_err(|_| "signal lock poisoned")?;
        INTERRUPTED.store(false, Ordering::SeqCst);
        let mut guard = Self {
            old: Vec::new(),
            _lock: lock,
        };
        for signal in [libc::SIGINT, libc::SIGTERM, libc::SIGHUP] {
            unsafe {
                let mut action: libc::sigaction = std::mem::zeroed();
                action.sa_sigaction = interrupted as *const () as usize;
                libc::sigemptyset(&mut action.sa_mask);
                let mut old = std::mem::zeroed();
                if libc::sigaction(signal, &action, &mut old) != 0 {
                    return Err("cannot install cancellation handler".into());
                }
                guard.old.push((signal, old));
            }
        }
        Ok(guard)
    }
}
impl Drop for SignalGuard {
    fn drop(&mut self) {
        for (signal, old) in &self.old {
            unsafe {
                libc::sigaction(*signal, old, std::ptr::null_mut());
            }
        }
    }
}
