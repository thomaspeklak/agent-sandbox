use std::fs;
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::process::Command;

use ags::config::HostUiConfig;
use ags::host_ui::{self, HostUiError};

const SERVICE: &str = r#"import os, socket, subprocess, sys
args = dict(zip(sys.argv[1::2], sys.argv[2::2]))
assert args['--renderer'] == 'process'
assert args['--idle-timeout-ms'] == '1000'
assert args['--log-level'] == 'info'
print('[glimpse_host_ui] listening on ' + args['--socket'], file=sys.stderr, flush=True)
server = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
server.bind(args['--socket'])
server.listen(4)
while True:
    conn, _ = server.accept()
    if conn.recv(1024):
        # Emulate native diagnostics from a renderer launched after startup.
        subprocess.run([sys.executable, '-c',
            "import os; os.write(2, b'MESA-INTEL: warning: FINISHME\\nfree(): corrupted unsorted chunks\\n')"], check=True)
        conn.sendall(b'done')
    conn.close()
"#;

fn config(root: &Path, body: &str) -> HostUiConfig {
    let stub = root.join("glimpse-host-ui-stub.py");
    // A subprocess keeps writable executable descriptors out of the test process.
    // Parallel test spawns could otherwise inherit one and cause ETXTBSY.
    let status = Command::new("python3")
        .args([
            "-c",
            "import pathlib, sys; pathlib.Path(sys.argv[1]).write_text(sys.argv[2])",
        ])
        .arg(&stub)
        .arg(format!("#!/usr/bin/env python3\n{body}"))
        .status()
        .unwrap();
    assert!(status.success(), "failed to create host UI test service");
    fs::set_permissions(&stub, fs::Permissions::from_mode(0o755)).unwrap();
    HostUiConfig {
        enabled: true,
        binary: stub.to_string_lossy().into_owned(),
        renderer: "process".to_owned(),
        renderer_bin: Some(root.join("renderer")),
        idle_timeout_ms: 1_000,
        log_level: "info".to_owned(),
    }
}

#[test]
fn service_and_late_renderer_diagnostics_are_private_and_survive_cleanup() {
    let temp = tempfile::tempdir().unwrap();
    let runtime_dir = temp.path().join("host-ui-runtime");
    let config = config(temp.path(), SERVICE);
    let guard = host_ui::start(&runtime_dir, "ags-test-session".to_owned(), &config).unwrap();
    assert!(guard.socket_path.exists());
    assert_eq!(guard.session_id, "ags-test-session");
    assert!(!guard.log_path.starts_with(&runtime_dir));
    assert_eq!(
        fs::metadata(&guard.log_path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(
        fs::metadata(guard.log_path.parent().unwrap())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o700
    );

    // Trigger a child renderer after startup, as a clipboard approval would.
    let mut client = UnixStream::connect(&guard.socket_path).unwrap();
    client.write_all(b"render").unwrap();
    let mut reply = String::new();
    client.read_to_string(&mut reply).unwrap();
    assert_eq!(reply, "done");
    let log_path = guard.log_path.clone();
    let log = fs::read_to_string(&log_path).unwrap();
    assert!(log.contains("[glimpse_host_ui] listening"));
    assert!(log.contains("MESA-INTEL: warning: FINISHME"));
    assert!(log.contains("free(): corrupted unsorted chunks"));

    drop(guard);
    assert!(!runtime_dir.exists());
    assert_eq!(fs::read_to_string(log_path).unwrap(), log);
}

#[test]
fn renderer_diagnostics_do_not_reach_the_terminal() {
    // Run the lifecycle test in a subprocess so both terminal streams are observable.
    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "service_and_late_renderer_diagnostics_are_private_and_survive_cleanup",
            "--nocapture",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "stdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    for stream in [&output.stdout, &output.stderr] {
        let text = String::from_utf8_lossy(stream);
        assert!(!text.contains("MESA-INTEL"), "{text}");
        assert!(!text.contains("corrupted unsorted chunks"), "{text}");
    }
}

#[test]
fn sessions_have_separate_logs() {
    let temp = tempfile::tempdir().unwrap();
    let config = config(temp.path(), SERVICE);
    let first = host_ui::start(&temp.path().join("first"), "first".to_owned(), &config).unwrap();
    let second = host_ui::start(&temp.path().join("second"), "second".to_owned(), &config).unwrap();
    assert_ne!(first.log_path, second.log_path);
    assert!(
        fs::read_to_string(&first.log_path)
            .unwrap()
            .contains("first/host-ui.sock")
    );
    assert!(
        fs::read_to_string(&second.log_path)
            .unwrap()
            .contains("second/host-ui.sock")
    );
}

#[test]
fn startup_failure_keeps_diagnostics_and_reports_their_location() {
    let temp = tempfile::tempdir().unwrap();
    let runtime_dir = temp.path().join("host-ui-runtime");
    let config = config(
        temp.path(),
        "import sys\nprint('renderer startup failed', file=sys.stderr, flush=True)\nsys.exit(42)\n",
    );
    let err = match host_ui::start(&runtime_dir, "failed".to_owned(), &config) {
        Ok(_) => panic!("service should not have started"),
        Err(err) => err,
    };
    let message = err.to_string();
    match err {
        HostUiError::ServiceExited { status, log_path } => {
            assert_eq!(status.code(), Some(42));
            assert!(message.contains(log_path.to_str().unwrap()));
            assert!(
                fs::read_to_string(log_path)
                    .unwrap()
                    .contains("renderer startup failed")
            );
        }
        other => panic!("unexpected error: {other}"),
    }
    assert!(!runtime_dir.exists());
}

#[test]
fn readiness_timeout_stops_the_service_and_keeps_its_log() {
    let temp = tempfile::tempdir().unwrap();
    let runtime_dir = temp.path().join("host-ui-runtime");
    let pid_path = temp.path().join("service.pid");
    let config = config(
        temp.path(),
        &format!(
            "import os, pathlib, sys, time\npathlib.Path({:?}).write_text(str(os.getpid()))\nprint('waiting forever', file=sys.stderr, flush=True)\ntime.sleep(60)\n",
            pid_path.to_str().unwrap()
        ),
    );
    let err = match host_ui::start(&runtime_dir, "timeout".to_owned(), &config) {
        Ok(_) => panic!("service should not have become ready"),
        Err(err) => err,
    };
    let message = err.to_string();
    match err {
        HostUiError::ReadyTimeout { log_path, .. } => {
            assert!(message.contains(log_path.to_str().unwrap()));
            assert!(
                fs::read_to_string(log_path)
                    .unwrap()
                    .contains("waiting forever")
            );
        }
        other => panic!("unexpected error: {other}"),
    }
    assert!(!runtime_dir.exists());
    #[cfg(target_os = "linux")]
    assert!(
        !Path::new("/proc")
            .join(fs::read_to_string(pid_path).unwrap())
            .exists()
    );
}

#[test]
fn log_creation_failure_does_not_fall_back_to_terminal_stderr() {
    let temp = tempfile::tempdir().unwrap();
    let config = config(temp.path(), SERVICE);
    fs::write(temp.path().join("host-ui-logs"), "not a directory").unwrap();
    assert!(matches!(
        host_ui::start(&temp.path().join("session"), "test".to_owned(), &config),
        Err(HostUiError::LogCreate(_))
    ));
}
