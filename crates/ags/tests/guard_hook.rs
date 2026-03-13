use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn guard_script_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../agent/hooks/guard.sh")
}

fn pi_guard_extension_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../agent/extensions/guard.ts")
}

fn have_command(name: &str) -> bool {
    Command::new("sh")
        .args(["-lc", &format!("command -v {name} >/dev/null 2>&1")])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn require_shell_tools() -> bool {
    if have_command("bash") && have_command("jq") {
        true
    } else {
        eprintln!("skipping guard hook test: bash and/or jq not available");
        false
    }
}

fn write_executable(path: &Path, content: &str) {
    fs::write(path, content).unwrap();
    let mut perms = fs::metadata(path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms).unwrap();
}

fn run_guard_with_env(
    input: &str,
    extra_env: &[(&str, &str)],
    setup: impl FnOnce(&Path),
) -> (String, String, i32, tempfile::TempDir) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path();
    let bin_dir = root.join("bin");
    fs::create_dir_all(&bin_dir).unwrap();
    setup(root);

    let path_env = format!(
        "{}:{}",
        bin_dir.display(),
        std::env::var("PATH").unwrap_or_default()
    );

    let mut command = Command::new("bash");
    command
        .arg(guard_script_path())
        .current_dir(root)
        .env("PATH", path_env)
        .env("HOME", "/home/dev")
        .env(
            "AGS_GUARD_READ_ROOTS_JSON",
            format!("[\"{}\",\"/tmp\"]", root.display()),
        )
        .env(
            "AGS_GUARD_WRITE_ROOTS_JSON",
            format!("[\"{}\",\"/tmp\"]", root.display()),
        )
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    for (key, value) in extra_env {
        command.env(key, value);
    }

    let mut child = command.spawn().unwrap();

    use std::io::Write;
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();

    (
        String::from_utf8_lossy(&output.stdout).to_string(),
        String::from_utf8_lossy(&output.stderr).to_string(),
        output.status.code().unwrap_or(-1),
        temp,
    )
}

fn run_guard(input: &str, setup: impl FnOnce(&Path)) -> (String, String, i32, tempfile::TempDir) {
    run_guard_with_env(input, &[], setup)
}

#[test]
fn pi_guard_extension_uses_dcg_without_ags_bash_string_heuristics() {
    let content = fs::read_to_string(pi_guard_extension_path()).unwrap();

    assert!(
        content.contains("maybeRunDcg(pi, command)"),
        "Pi guard should still delegate Bash evaluation to dcg"
    );
    assert!(
        !content.contains("shutdown|reboot|poweroff|halt"),
        "Pi guard should not contain the old shutdown regex false positive"
    );
    assert!(
        !content.contains("DANGEROUS_BASH_PATTERNS"),
        "Pi guard should not maintain a broad AGS Bash denylist"
    );
    assert!(
        !content.contains("command.includes(p)"),
        "Pi guard should not inspect raw Bash command strings for sensitive paths"
    );
    assert!(
        !content.contains("Command references sensitive host path"),
        "Pi guard should no longer block Bash purely on a substring path match"
    );
    assert!(
        content.contains("AGS_GUARD_YOLO"),
        "Pi guard should respect the AGS_GUARD_YOLO escape hatch"
    );
}

#[test]
fn guard_hook_allows_commit_message_with_shutdown_word() {
    if !require_shell_tools() {
        return;
    }

    let input = r#"{"tool_name":"Bash","tool_input":{"command":"git commit -m 'docs: mention shutdown behavior'"}}"#;
    let (stdout, stderr, exit_code, _temp) = run_guard(input, |_| {});

    assert_eq!(exit_code, 0, "stderr: {stderr}");
    assert!(stdout.trim().is_empty(), "stdout: {stdout}");
    assert!(stderr.trim().is_empty(), "stderr: {stderr}");
}

#[test]
fn guard_hook_denies_read_outside_allowed_roots() {
    if !require_shell_tools() {
        return;
    }

    let input = r#"{"tool_name":"Read","tool_input":{"file_path":"/etc/passwd"}}"#;
    let (stdout, stderr, exit_code, _temp) = run_guard(input, |_| {});

    assert_eq!(exit_code, 2, "stdout: {stdout}\nstderr: {stderr}");
    assert!(stdout.trim().is_empty(), "stdout: {stdout}");
    assert!(
        stderr.contains("Read outside sandbox roots denied"),
        "stderr: {stderr}"
    );
}

#[test]
fn guard_hook_denies_sensitive_path_reference_in_bash() {
    if !require_shell_tools() {
        return;
    }

    let input =
        r#"{"tool_name":"Bash","tool_input":{"command":"cat /home/dev/.ssh/id_ed25519.pub"}}"#;
    let (stdout, stderr, exit_code, _temp) = run_guard(input, |_| {});

    assert_eq!(exit_code, 2, "stdout: {stdout}\nstderr: {stderr}");
    assert!(stdout.trim().is_empty(), "stdout: {stdout}");
    assert!(
        stderr.contains("Command references sensitive host path"),
        "stderr: {stderr}"
    );
}

#[test]
fn guard_hook_passes_original_payload_to_dcg() {
    if !require_shell_tools() {
        return;
    }

    let input = r#"{"tool_name":"Bash","tool_input":{"command":"git reset --hard"}}"#;
    let (stdout, stderr, exit_code, temp) = run_guard(input, |root| {
        let capture_path = root.join("captured-input.json");
        let dcg_path = root.join("bin/dcg");
        write_executable(
            &dcg_path,
            &format!(
                "#!/usr/bin/env bash\ncat > '{}'\nprintf '%s' '{{\"hookSpecificOutput\":{{\"permissionDecision\":\"deny\",\"permissionDecisionReason\":\"blocked by fake dcg\"}}}}'\n",
                capture_path.display()
            ),
        );
    });

    assert_eq!(exit_code, 0, "stderr: {stderr}");
    assert!(stderr.trim().is_empty(), "stderr: {stderr}");
    assert!(
        stdout.contains("\"permissionDecision\":\"deny\""),
        "stdout: {stdout}"
    );

    let captured = fs::read_to_string(temp.path().join("captured-input.json")).unwrap();
    assert_eq!(captured, input);
}

#[test]
fn guard_hook_fails_open_when_dcg_errors() {
    if !require_shell_tools() {
        return;
    }

    let input = r#"{"tool_name":"Bash","tool_input":{"command":"echo hello"}}"#;
    let (stdout, stderr, exit_code, _temp) = run_guard(input, |root| {
        let dcg_path = root.join("bin/dcg");
        write_executable(
            &dcg_path,
            "#!/usr/bin/env bash\necho 'dcg internal error' >&2\nexit 2\n",
        );
    });

    assert_eq!(exit_code, 0, "stdout: {stdout}\nstderr: {stderr}");
    assert!(stdout.trim().is_empty(), "stdout: {stdout}");
    assert!(stderr.trim().is_empty(), "stderr: {stderr}");
}

#[test]
fn guard_hook_yolo_disables_claude_guard() {
    if !require_shell_tools() {
        return;
    }

    let input =
        r#"{"tool_name":"Bash","tool_input":{"command":"cat /home/dev/.ssh/id_ed25519.pub"}}"#;
    let (stdout, stderr, exit_code, _temp) =
        run_guard_with_env(input, &[("AGS_GUARD_YOLO", "1")], |_| {});

    assert_eq!(exit_code, 0, "stdout: {stdout}\nstderr: {stderr}");
    assert!(stdout.trim().is_empty(), "stdout: {stdout}");
    assert!(stderr.trim().is_empty(), "stderr: {stderr}");
}
