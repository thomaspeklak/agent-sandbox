use super::{Context, Hook, Response, TrustStore, validate_response};
use std::process::Command;
use std::sync::{
    Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use std::time::{Duration, Instant};

#[derive(Clone, Copy)]
pub struct Limits {
    pub concurrency: usize,
    pub phase_timeout: Duration,
    pub stdout_bytes: usize,
    pub stderr_bytes: usize,
}
impl Default for Limits {
    fn default() -> Self {
        Self {
            concurrency: 4,
            phase_timeout: Duration::from_secs(60),
            stdout_bytes: 1024 * 1024,
            stderr_bytes: 64 * 1024,
        }
    }
}

/// All workers see identical context bytes; results always return in declaration order.
/// Approval is checked for the ENTIRE set before spawning, then rechecked at execution.
pub fn run(
    hooks: &[Hook],
    context: &Context,
    store: &TrustStore,
    limits: Limits,
) -> Result<Vec<Response>, String> {
    context.validate()?;
    if limits.concurrency == 0 {
        return Err("hook concurrency must be positive".into());
    }
    for hook in hooks {
        store.ensure(hook, false)?;
    }
    let input = serde_json::to_vec(context).map_err(|e| e.to_string())?;
    let deadline = Instant::now() + limits.phase_timeout;
    // Finish all writable snapshot handles before any worker forks. A sibling
    // fork retaining a writable entrypoint handle can cause Linux ETXTBSY.
    let mut snapshots = Vec::new();
    for hook in hooks {
        if Instant::now() >= deadline {
            return Err("prepare phase timed out during snapshotting".into());
        }
        let dir = store.execution_directory()?;
        let (path, hash) = store.snapshot(hook, dir.path())?;
        snapshots.push((dir, path, hash));
    }
    let next = AtomicUsize::new(0);
    let cancel = AtomicBool::new(false);
    let results = Mutex::new(
        (0..hooks.len())
            .map(|_| None)
            .collect::<Vec<Option<Result<Response, String>>>>(),
    );
    std::thread::scope(|scope| {
        for _ in 0..limits.concurrency.min(hooks.len()) {
            let (input, next, cancel, results, snapshots) =
                (&input, &next, &cancel, &results, &snapshots);
            scope.spawn(move || {
                loop {
                    if cancel.load(Ordering::SeqCst) {
                        break;
                    }
                    let index = next.fetch_add(1, Ordering::SeqCst);
                    let Some(hook) = hooks.get(index) else {
                        break;
                    };
                    let result = run_one(
                        (hook, &snapshots[index].1, &snapshots[index].2),
                        input.clone(),
                        context,
                        store,
                        limits,
                        deadline,
                        cancel,
                    );
                    if result.is_err() {
                        cancel.store(true, Ordering::SeqCst);
                    }
                    results.lock().expect("hook result lock")[index] = Some(result);
                }
            });
        }
    });
    let results = results
        .into_inner()
        .map_err(|_| "hook result lock poisoned")?;
    // Prefer the original failure over peers cancelled by it.
    for result in results.iter().flatten() {
        if let Err(error) = result
            && !error.contains("cancelled")
        {
            return Err(error.clone());
        }
    }
    results
        .into_iter()
        .map(|r| r.unwrap_or_else(|| Err("prepare phase cancelled".into())))
        .collect()
}
fn run_one(
    entrypoint: (&Hook, &std::path::Path, &str),
    input: Vec<u8>,
    context: &Context,
    store: &TrustStore,
    limits: Limits,
    phase_deadline: Instant,
    cancel: &AtomicBool,
) -> Result<Response, String> {
    let (hook, snapshot, expected_hash) = entrypoint;
    let execute = || {
        store.check_snapshot(hook, expected_hash)?; // Bind the recheck to these exact snapshot bytes.
        let mut command = Command::new(snapshot);
        command
            .args(&hook.args)
            .current_dir(&context.workdir)
            .env_clear();
        // No inherited host tokens or resolved secrets. Hook-owned auth must be explicit.
        for key in [
            "PATH",
            "HOME",
            "USER",
            "LOGNAME",
            "XDG_RUNTIME_DIR",
            "DBUS_SESSION_BUS_ADDRESS",
        ] {
            if let Some(value) = std::env::var_os(key) {
                command.env(key, value);
            }
        }
        let deadline =
            phase_deadline.min(Instant::now() + Duration::from_secs(hook.timeout_seconds));
        let output = crate::host_process::capture(
            &mut command,
            input,
            deadline,
            cancel,
            limits.stdout_bytes,
            limits.stderr_bytes,
        )
        .map_err(|error| {
            if error.starts_with("could not execute host program:") {
                format!(
                    "{error}; hook snapshots are executed under {:?}; verify that this state filesystem permits execution (not mounted noexec) and the entrypoint interpreter is available",
                    store.path
                )
            } else {
                error
            }
        })?;
        if !output.stderr.is_empty() {
            // Escape control codes so a diagnostic cannot spoof terminal prompts.
            eprintln!(
                "[hook {} stderr] {:?}",
                hook.name,
                String::from_utf8_lossy(&output.stderr)
            );
        }
        output.check_status()?;
        validate_response(&output.stdout)
    };
    execute().map_err(|e| format!("prepare hook {:?}: {e}", hook.name))
}
