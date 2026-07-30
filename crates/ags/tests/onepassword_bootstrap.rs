use std::fs;
use std::os::fd::AsRawFd;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};

fn bootstrap() -> tempfile::TempPath {
    let file = tempfile::NamedTempFile::new().unwrap();
    fs::write(file.path(), ags::assets::ONEPASSWORD_BOOTSTRAP).unwrap();
    fs::set_permissions(file.path(), fs::Permissions::from_mode(0o755)).unwrap();
    file.into_temp_path()
}

fn run(items: &[&str], command: &[&str], inherited: &[(&str, &str)]) -> std::process::Output {
    let bootstrap = bootstrap();
    let files: Vec<_> = items
        .iter()
        .map(|item| {
            tempfile::tempfile()
                .and_then(|mut file| {
                    std::io::Write::write_all(&mut file, item.as_bytes())?;
                    std::io::Seek::seek(&mut file, std::io::SeekFrom::Start(0))?;
                    Ok(file)
                })
                .unwrap()
        })
        .collect();
    let raw_fds: Vec<_> = files.iter().map(AsRawFd::as_raw_fd).collect();
    let mut process = Command::new("python3");
    process.args([
        "-c",
        "import os,sys; split=sys.argv.index('--'); [os.dup2(int(fd), 3+i) for i,fd in enumerate(sys.argv[1:split])]; os.execv(sys.argv[split+1], sys.argv[split+1:])",
    ]);
    for fd in &raw_fds {
        process.arg(fd.to_string());
    }
    process.arg("--");
    process.arg(&bootstrap);
    process.args(["--fd-count", &items.len().to_string(), "--"]);
    process.args(command);
    process.stdout(Stdio::piped()).stderr(Stdio::piped());
    for (key, value) in inherited {
        process.env(key, value);
    }
    unsafe {
        process.pre_exec(move || {
            for fd in &raw_fds {
                let flags = libc::fcntl(*fd, libc::F_GETFD);
                if flags < 0 || libc::fcntl(*fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
            }
            Ok(())
        });
    }
    process.output().unwrap()
}

#[test]
fn injects_all_fields_with_empty_and_multiline_values_and_overlays_environment() {
    let item = r#"{"category":"SECURE_NOTE","fields":[{"label":"PGHOST","value":"db"},{"label":"PGPORT","value":"5432"},{"label":"PGUSER","value":"postgres"},{"label":"PGPASSWORD","value":""},{"label":"PGDATABASE","value":"line1\nline2"},{"label":"IGNORED"}]}"#;
    let output = run(
        &[item],
        &[
            "python3",
            "-c",
            "import os; print('|'.join(os.environ.get(k, '?') for k in ['PGHOST','PGPORT','PGUSER','PGPASSWORD','PGDATABASE','IGNORED','OVERLAY']))",
        ],
        &[("OVERLAY", "before")],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        "db|5432|postgres||line1\nline2|?|before\n"
    );
}

#[test]
fn missing_value_fields_are_ignored_before_label_validation() {
    let item = r#"{"category":"SECURE_NOTE","fields":[{"label":"BAD-NAME"},{"label":"\u0000"},{"value":"used","label":"GOOD"}]}"#;
    let output = run(
        &[item],
        &["python3", "-c", "import os; print(os.environ['GOOD'])"],
        &[],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "used\n");
}

#[test]
fn closes_payload_descriptors_before_final_exec() {
    let item = r#"{"category":"SECURE_NOTE","fields":[{"label":"GOOD","value":"used"}]}"#;
    let output = run(
        &[item],
        &[
            "sh",
            "-c",
            "for fd in 3 4; do target=$(readlink /proc/self/fd/$fd 2>/dev/null || true); case $target in *ags-op-item*) exit 99;; esac; done",
        ],
        &[],
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn merges_multiple_documents_last_value_wins_and_execs_final_command() {
    let first = r#"{"category":"SECURE_NOTE","fields":[{"label":"DUP","value":"first"},{"label":"OVERLAY","value":"item"}]}"#;
    let second = r#"{"category":"SECURE_NOTE","fields":[{"label":"DUP","value":"last"}]}"#;
    let output = run(
        &[first, second],
        &[
            "python3",
            "-c",
            "import os; print(os.environ['DUP'] + ':' + os.environ['OVERLAY'])",
        ],
        &[("OVERLAY", "host")],
    );
    assert!(output.status.success());
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "last:item\n");
}

#[test]
fn same_document_duplicates_use_the_last_field() {
    let item = r#"{"category":"SECURE_NOTE","fields":[{"label":"DUP","value":"first"},{"label":"DUP","value":"last"}]}"#;
    let output = run(
        &[item],
        &["python3", "-c", "import os; print(os.environ['DUP'])"],
        &[],
    );
    assert!(output.status.success());
    assert_eq!(String::from_utf8(output.stdout).unwrap(), "last\n");
}

#[test]
fn invalid_documents_are_redacted() {
    let sentinel = "DO-NOT-PRINT-THIS-VALUE";
    for item in [
        "not json",
        r#"{"category":"PASSWORD","fields":[]}"#,
        r#"{"category":"SECURE_NOTE","fields":[{"label":"BAD-NAME","value":"DO-NOT-PRINT-THIS-VALUE"}]}"#,
        r#"{"category":"SECURE_NOTE","fields":[{"label":"BAD=NAME","value":"DO-NOT-PRINT-THIS-VALUE"}]}"#,
        r#"{"category":"SECURE_NOTE","fields":[{"label":"BAD\u0000NAME","value":"DO-NOT-PRINT-THIS-VALUE"}]}"#,
        r#"{"category":"SECURE_NOTE","fields":[{"label":"GOOD","value":"BAD\u0000VALUE"}]}"#,
        r#"{"category":"SECURE_NOTE","fields":[{"label":"GOOD","value":7}]}"#,
    ] {
        let output = run(&[item], &["true"], &[]);
        assert!(!output.status.success());
        let error = String::from_utf8(output.stderr).unwrap();
        assert!(error.contains("1Password bootstrap failed"));
        assert!(!error.contains(sentinel), "{error}");
        assert!(!error.contains(item), "{error}");
    }
}
