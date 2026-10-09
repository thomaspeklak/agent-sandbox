use super::protocol::EnvValue;
use super::trust::fingerprint;
use super::*;
use crate::cli::{Agent, Command, RunOptions};
use std::fs;
use std::path::Path;
use std::time::{Duration, Instant};

fn script(root: &Path, name: &str, body: &str) -> Hook {
    use std::os::unix::fs::PermissionsExt;
    let executable = root.join(name);
    fs::write(&executable, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
    Hook {
        name: name.into(),
        executable,
        args: Vec::new(),
        timeout_seconds: 1,
        project: None,
    }
}
fn approve_fixture(store: &TrustStore, hook: &Hook) {
    store.record(&fingerprint(hook).unwrap().0).unwrap();
}
fn context(root: &Path) -> Context {
    Context::new(root, Agent::Shell).unwrap()
}
fn limits() -> Limits {
    Limits {
        phase_timeout: Duration::from_secs(2),
        ..Limits::default()
    }
}
fn store(root: &Path) -> TrustStore {
    TrustStore {
        path: root.join("trust"),
    }
}
fn config(root: &Path) -> crate::config::ValidatedConfig {
    crate::config::parse_toml_str(
        &format!(
            r#"
[sandbox]
image = "test"
containerfile = "{0}/Containerfile"
cache_dir = "{0}/cache"
gitconfig_path = "{0}/gitconfig"
auth_key = "{0}/auth"
sign_key = "{0}/sign"
"#,
            root.display()
        ),
        &root.join("config.toml"),
    )
    .unwrap()
}
fn options(extra: &[&str]) -> RunOptions {
    let mut args = vec!["ags".to_owned(), "--agent".into(), "shell".into()];
    args.extend(extra.iter().map(|s| s.to_string()));
    match crate::cli::parse_args(args).unwrap() {
        Command::Run(opts) => opts,
        _ => unreachable!(),
    }
}
fn response(json: &str) -> Response {
    validate_response(json.as_bytes()).unwrap()
}
fn assert_descendant_terminated(pid: u32) {
    // Group SIGKILL is asynchronous; direct-child wait does not wait for a
    // grandchild's exit. Require observed exit, not merely signal delivery.
    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        match fs::read_to_string(format!("/proc/{pid}/stat")) {
            Ok(stat)
                if stat
                    .rsplit_once(')')
                    .is_some_and(|(_, s)| s.starts_with(" Z ")) =>
            {
                return;
            }
            Ok(stat) => assert!(Instant::now() < deadline, "descendant still alive: {stat}"),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
            Err(e) => panic!("cannot inspect descendant {pid}: {e}"),
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn real_scripts_run_in_parallel_with_identical_context_and_declaration_order() {
    let root = tempfile::tempdir().unwrap();
    let store = store(root.path());
    let program = "cat > \"$1\"; touch \"$2\"; while [ ! -f \"$3\" ]; do sleep 0.01; done; sleep \"$4\"; printf '%s' \"$5\"";
    let mut first = script(root.path(), "first", program);
    let mut second = script(root.path(), "second", program);
    first.args = vec![
        root.path().join("input1").display().to_string(),
        root.path().join("start1").display().to_string(),
        root.path().join("start2").display().to_string(),
        "0.15".into(),
        r#"{"version":1,"env":{"EXAMPLE":"global"}}"#.into(),
    ];
    second.args = vec![
        root.path().join("input2").display().to_string(),
        root.path().join("start2").display().to_string(),
        root.path().join("start1").display().to_string(),
        "0".into(),
        r#"{"version":1,"env":{"EXAMPLE":"project"}}"#.into(),
    ];
    second.project = Some(root.path().to_path_buf());
    approve_fixture(&store, &first);
    approve_fixture(&store, &second);
    let ctx = context(root.path());
    let responses = run(&[first, second], &ctx, &store, limits()).unwrap();
    assert_eq!(
        fs::read(root.path().join("input1")).unwrap(),
        serde_json::to_vec(&ctx).unwrap()
    );
    assert_eq!(
        fs::read(root.path().join("input1")).unwrap(),
        fs::read(root.path().join("input2")).unwrap()
    );
    let merged = Contributions::merge(responses).unwrap();
    let mut cfg = config(root.path());
    let materialized = merged.materialize(&mut cfg, &options(&[])).unwrap();
    assert_eq!(materialized.env, [("EXAMPLE".into(), "project".into())]);
}

#[test]
fn native_entrypoint_with_arguments_is_supported_and_bound_to_snapshot() {
    let root = tempfile::tempdir().unwrap();
    let mut native = script(root.path(), "native", "");
    fs::copy("/bin/sh", &native.executable).unwrap();
    native.args = vec![
        "-c".into(),
        "cat >/dev/null; printf '{\"version\":1}'".into(),
    ];
    let store = store(root.path());
    approve_fixture(&store, &native);
    assert_eq!(
        run(&[native], &context(root.path()), &store, limits()).unwrap()[0].version,
        1
    );
    let original = script(
        root.path(),
        "snapshot",
        "cat >/dev/null; printf '{\"version\":1}'",
    );
    approve_fixture(&store, &original);
    let dir = tempfile::tempdir().unwrap();
    let (snapshot, expected_hash) = store.snapshot(&original, dir.path()).unwrap();
    fs::write(&original.executable, "changed").unwrap();
    assert!(fs::read_to_string(snapshot).unwrap().contains("printf"));
    assert!(store.snapshot(&original, root.path()).is_err());
    approve_fixture(&store, &original); // Even an already-approved different revision cannot replace this snapshot.
    assert!(store.check_snapshot(&original, &expected_hash).is_err());
}

#[test]
fn symlink_to_regular_entrypoint_runs_and_remains_snapshot_bound() {
    let root = tempfile::tempdir().unwrap();
    let target = script(
        root.path(),
        "target",
        "cat >/dev/null; printf '{\"version\":1}'",
    );
    let mut hook = target.clone();
    hook.executable = root.path().join("alias");
    std::os::unix::fs::symlink(&target.executable, &hook.executable).unwrap();
    let store = store(root.path());
    approve_fixture(&store, &hook);
    assert_eq!(
        run(&[hook.clone()], &context(root.path()), &store, limits()).unwrap()[0].version,
        1
    );
    let snapshot_dir = tempfile::tempdir().unwrap();
    let (snapshot, expected_hash) = store.snapshot(&hook, snapshot_dir.path()).unwrap();
    assert_eq!(
        fs::read(&snapshot).unwrap(),
        fs::read(&target.executable).unwrap()
    );
    fs::write(&target.executable, "#!/bin/sh\nexit 2\n").unwrap();
    assert!(store.check_snapshot(&hook, &expected_hash).is_err());
    assert!(store.snapshot(&hook, root.path()).is_err());
    assert!(fs::read_to_string(snapshot).unwrap().contains("printf"));
}

#[test]
fn trust_changes_and_noninteractive_refusal_before_any_execution() {
    let root = tempfile::tempdir().unwrap();
    let first = script(
        root.path(),
        "first",
        "cat >/dev/null; touch \"$1\"; printf '{\"version\":1}'",
    );
    let mut first = first;
    first
        .args
        .push(root.path().join("executed").display().to_string());
    let second = script(root.path(), "second", "exit 1");
    let store = store(root.path());
    approve_fixture(&store, &first);
    assert!(
        store
            .ensure(&second, false)
            .unwrap_err()
            .contains("ags hooks test")
    );
    assert!(
        run(
            &[first.clone(), second],
            &context(root.path()),
            &store,
            limits()
        )
        .is_err()
    );
    assert!(!root.path().join("executed").exists());
    let mut changed = first.clone();
    changed.args.push("changed".into());
    assert!(store.ensure(&changed, false).is_err());
    changed = first.clone();
    changed.project = Some(root.path().to_path_buf());
    assert!(store.ensure(&changed, false).is_err());
    fs::write(&first.executable, "#!/bin/sh\nexit 2\n").unwrap();
    assert!(store.ensure(&first, false).is_err());
}

#[test]
fn trust_storage_rejects_symlinks_and_public_permissions() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let root = tempfile::tempdir().unwrap();
    let hook = script(root.path(), "fixture", "exit 0");
    let store = store(root.path());
    approve_fixture(&store, &hook);
    fs::set_permissions(&store.path, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(store.ensure(&hook, false).is_err());
    fs::set_permissions(&store.path, fs::Permissions::from_mode(0o700)).unwrap();
    let hash = fingerprint(&hook).unwrap().0;
    fs::remove_file(store.path.join(&hash)).unwrap();
    let target = root.path().join("token");
    fs::write(&target, &hash).unwrap();
    symlink(&target, store.path.join(&hash)).unwrap();
    assert!(store.ensure(&hook, false).is_err());
}

#[test]
fn invalid_schema_output_failures_and_bounds_are_not_retried() {
    let root = tempfile::tempdir().unwrap();
    let store = store(root.path());
    for (i, body) in [
        "cat >/dev/null; exit 17",
        "cat >/dev/null; echo not-json",
        "cat >/dev/null; printf '{\"version\":2}'",
        "cat >/dev/null; printf '{\"version\":1,\"settings\":{}}'",
        "cat >/dev/null; printf '{\"version\":1,\"env\":{\"AGS_GUARD_YOLO\":\"1\"}}'",
        "cat >/dev/null; head -c 1025 /dev/zero",
        "cat >/dev/null; head -c 1025 /dev/zero >&2; printf '{\"version\":1}'",
    ]
    .iter()
    .enumerate()
    {
        let body = format!("echo attempt >> \"$1\"; {body}");
        let mut hook = script(root.path(), &format!("bad{i}"), &body);
        let attempts = root.path().join(format!("attempts{i}"));
        hook.args.push(attempts.display().to_string());
        approve_fixture(&store, &hook);
        let lim = Limits {
            stdout_bytes: 1024,
            stderr_bytes: 1024,
            ..limits()
        };
        assert!(
            run(&[hook], &context(root.path()), &store, lim).is_err(),
            "case {i}"
        );
        assert_eq!(fs::read_to_string(attempts).unwrap(), "attempt\n");
    }
}

#[test]
fn phase_deadline_failure_cancellation_and_descendant_termination() {
    let root = tempfile::tempdir().unwrap();
    let store = store(root.path());
    let mut slow = script(
        root.path(),
        "slow",
        "cat >/dev/null; sleep 20 & echo $! > \"$1\"; wait",
    );
    let pid_path = root.path().join("child-pid");
    slow.args.push(pid_path.display().to_string());
    approve_fixture(&store, &slow);
    let start = Instant::now();
    let error = run(
        &[slow.clone()],
        &context(root.path()),
        &store,
        Limits {
            phase_timeout: Duration::from_millis(100),
            ..limits()
        },
    )
    .unwrap_err();
    assert!(error.contains("timed out"), "{error}");
    assert!(start.elapsed() < Duration::from_secs(2));
    let pid: u32 = fs::read_to_string(&pid_path)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert_descendant_terminated(pid);
    let fail = script(root.path(), "fail", "cat >/dev/null; sleep 0.05; exit 8");
    approve_fixture(&store, &fail);
    let start = Instant::now();
    let error = run(&[slow, fail], &context(root.path()), &store, limits()).unwrap_err();
    assert!(error.contains("status"), "{error}");
    assert!(start.elapsed() < Duration::from_millis(800));
    let pid = fs::read_to_string(pid_path)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert_descendant_terminated(pid);
}

#[test]
fn per_hook_timeout_is_distinct_from_overall_phase_timeout() {
    let root = tempfile::tempdir().unwrap();
    let store = store(root.path());
    let hook = script(root.path(), "slow", "cat >/dev/null; sleep 20");
    approve_fixture(&store, &hook);
    let start = Instant::now();
    assert!(
        run(
            &[hook],
            &context(root.path()),
            &store,
            Limits {
                phase_timeout: Duration::from_secs(4),
                ..limits()
            }
        )
        .unwrap_err()
        .contains("timed out")
    );
    assert!(start.elapsed() >= Duration::from_millis(900));
    assert!(start.elapsed() < Duration::from_secs(3));
}

#[test]
fn strict_json_rejects_unknown_fields_duplicate_keys_trailing_data_and_protection_bypasses() {
    for json in [
        r#"{"version":1,"env":{"X":"a","X":"b"}}"#,
        r#"{"version":1,"version":1}"#,
        r#"{"version":1} garbage"#,
        r#"{"version":1,"env":{"X":{"op":"op://v/i/f","extra":1}}}"#,
        r#"{"version":1,"env":{"X":{"op":"op://v/i/field\n"}}}"#,
        r#"{"version":1,"env":{"OP_SERVICE_ACCOUNT_TOKEN":"bad"}}"#,
        r#"{"version":1,"env":{"OPENCODE_CONFIG_CONTENT":"bad"}}"#,
        r#"{"version":1,"files":[{"destination":"/tmp/../run/ags/file","content":"bad"}]}"#,
        r#"{"version":1,"files":[{"destination":"/home/dev/.pi/agent/settings.json","content":"bad"}]}"#,
        r#"{"version":1,"files":[{"destination":"/home/dev/.claude.json","content":"bad"}]}"#,
        r#"{"version":1,"files":[{"destination":"/run/ags/onepassword-bootstrap","content":"bad"}]}"#,
        r#"{"version":1,"files":[{"destination":"/tmp/ags-run-in-tmux.sh","content":"bad"}]}"#,
        r#"{"version":1,"files":[{"destination":"/home/dev/.zshrc","content":"bad"}]}"#,
        r#"{"version":1,"mounts":[{"source":"relative","destination":"/tmp/data","mode":"rw"}]}"#,
    ] {
        assert!(validate_response(json.as_bytes()).is_err(), "{json}");
    }
}

#[test]
fn agent_configuration_redirectors_cannot_reinterpret_generated_data_as_settings() {
    for key in [
        "HOME",
        "XDG_CONFIG_HOME",
        "OPENCODE_CONFIG_DIR",
        "OPENCODE_CONFIG",
        "OPENCODE_CONFIG_CONTENT",
        "OPENCODE_TUI_CONFIG",
        "CLAUDE_CONFIG_DIR",
        "CODEX_HOME",
        "GEMINI_CLI_HOME",
        "GEMINI_CLI_SYSTEM_SETTINGS_PATH",
        "GEMINI_CLI_SYSTEM_DEFAULTS_PATH",
        "GEMINI_CLI_TRUSTED_FOLDERS_PATH",
        "PI_CODING_AGENT_DIR",
    ] {
        for value in [
            serde_json::json!("/tmp/hook-config"),
            serde_json::json!({"op": "op://vault/item/field"}),
        ] {
            let output = serde_json::json!({
                "version": 1,
                "env": {key: value},
                "files": [{"destination": "/tmp/hook-config/opencode/opencode.json", "content": "{}"}],
            });
            assert!(
                validate_response(&serde_json::to_vec(&output).unwrap()).is_err(),
                "{key}"
            );
        }
    }
    for key in [
        "ANTHROPIC_API_KEY",
        "OPENAI_API_KEY",
        "GEMINI_API_KEY",
        "OPENCODE_API_KEY",
        "OPENROUTER_API_KEY",
        "AI_GATEWAY_API_KEY",
    ] {
        let output = serde_json::json!({"version": 1, "env": {key: "fixture"}});
        assert!(validate_response(&serde_json::to_vec(&output).unwrap()).is_ok());
    }
}

#[test]
fn env_literal_and_secret_precedence_and_cli_eliminate_losing_references() {
    let root = tempfile::tempdir().unwrap();
    let mut cfg = config(root.path());
    let first = response(
        r#"{"version":1,"env":{"X":{"op":"op://v/i/loser"},"Y":"default","Z":{"op":"op://v/i/loser"}}}"#,
    );
    let second = response(r#"{"version":1,"env":{"X":"literal","Y":{"op":"op://v/i/winner"}}}"#);
    let merged = Contributions::merge(vec![first, second]).unwrap();
    let summary = merged.redact_summary().to_string();
    assert!(!summary.contains("winner"));
    assert!(!summary.contains("literal"));
    let prepared = merged
        .materialize(&mut cfg, &options(&["--env", "Z=cli"]))
        .unwrap();
    assert_eq!(
        prepared.references,
        [("Y".into(), "op://v/i/winner".into())].into()
    );
    assert!(prepared.env.contains(&("Z".into(), "cli".into())));
    assert!(prepared.env.contains(&("X".into(), "literal".into())));
    let mut defaults = [("X".into(), "old".into()), ("Z".into(), "old".into())].into();
    prepared.filter_default_secrets(&mut defaults);
    assert!(defaults.is_empty());
}

#[test]
fn exact_files_and_mounts_replace_by_precedence_and_cleanup_is_owned() {
    let root = tempfile::tempdir().unwrap();
    let mut cfg = config(root.path());
    let first =
        response(r#"{"version":1,"files":[{"destination":"/tmp/hook-file","content":"loser"}]}"#);
    let second =
        response(r#"{"version":1,"files":[{"destination":"/tmp/hook-file","content":"winner"}]}"#);
    let prepared = Contributions::merge(vec![first, second])
        .unwrap()
        .materialize(&mut cfg, &options(&[]))
        .unwrap();
    assert_eq!(prepared.mounts.len(), 1);
    let path = prepared.mounts[0].host.clone();
    assert_eq!(fs::read_to_string(&path).unwrap(), "winner");
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    drop(prepared);
    assert!(!path.exists());
    let source = root.path().join("external-data");
    fs::create_dir(&source).unwrap();
    let json = format!(
        r#"{{"version":1,"mounts":[{{"source":{},"destination":"/tmp/hook-file","mode":"ro"}}]}}"#,
        serde_json::to_string(&source).unwrap()
    );
    let prepared = Contributions::merge(vec![
        response(r#"{"version":1,"files":[{"destination":"/tmp/hook-file","content":"loser"}]}"#),
        response(&json),
    ])
    .unwrap()
    .materialize(&mut cfg, &options(&[]))
    .unwrap();
    assert_eq!(prepared.mounts[0].host, source);
}

#[test]
fn overlapping_destinations_are_rejected_and_cli_exact_destinations_win() {
    let first = response(r#"{"version":1,"files":[{"destination":"/tmp/hook/a","content":"a"}]}"#);
    let second = response(r#"{"version":1,"files":[{"destination":"/tmp/hook","content":"b"}]}"#);
    assert!(Contributions::merge(vec![first, second]).is_err());
    let root = tempfile::tempdir().unwrap();
    let mut cfg = config(root.path());
    let destination = root.path().join("cli-dir");
    fs::create_dir(&destination).unwrap();
    let json = format!(
        r#"{{"version":1,"files":[{{"destination":{},"content":"loser"}}]}}"#,
        serde_json::to_string(&destination).unwrap()
    );
    let opts = options(&["--add-dir", destination.to_str().unwrap()]);
    let prepared = Contributions::merge(vec![response(&json)])
        .unwrap()
        .materialize(&mut cfg, &opts)
        .unwrap();
    assert!(prepared.mounts.is_empty());
}

#[test]
fn lockdown_allows_literals_files_and_rejects_only_surviving_references_and_host_mounts() {
    let root = tempfile::tempdir().unwrap();
    let mut cfg = config(root.path());
    let opts = options(&["--lockdown"]);
    let good = response(
        r#"{"version":1,"env":{"EXAMPLE":"yes"},"files":[{"destination":"/tmp/fixture","content":"data"}]}"#,
    );
    assert!(
        Contributions::merge(vec![good])
            .unwrap()
            .materialize(&mut cfg, &opts)
            .is_ok()
    );
    let secret = r#"{"version":1,"env":{"EXAMPLE":{"op":"op://v/i/f"}}}"#;
    assert!(
        Contributions::merge(vec![response(secret)])
            .unwrap()
            .materialize(&mut cfg, &opts)
            .is_err()
    );
    assert!(
        Contributions::merge(vec![response(secret)])
            .unwrap()
            .materialize(&mut cfg, &options(&["--lockdown", "--env", "EXAMPLE=cli"]))
            .is_ok()
    );
    let mount = response(
        r#"{"version":1,"mounts":[{"source":"/tmp","destination":"/tmp/data","mode":"ro"}]}"#,
    );
    assert!(
        Contributions::merge(vec![mount])
            .unwrap()
            .materialize(&mut cfg, &opts)
            .is_err()
    );
}

#[test]
fn config_accumulates_global_project_hooks_and_binds_scope_paths_and_arguments() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join(".ags")).unwrap();
    let global = root.path().join("global.toml");
    let project = root.path().join(".ags/config.toml");
    let config = config(root.path());
    let toml = format!(
        r#"[[prepare_hook]]
name="global"
executable="global.sh"
args=["hello"]
[sandbox]
image="test"
containerfile="{0}"
cache_dir="{1}"
gitconfig_path="{2}"
auth_key="{3}"
sign_key="{4}"
"#,
        config.sandbox.containerfile.display(),
        config.sandbox.cache_dir.display(),
        config.sandbox.gitconfig_path.display(),
        config.sandbox.auth_key.display(),
        config.sandbox.sign_key.display()
    );
    fs::write(&global, toml).unwrap();
    fs::write(
        &project,
        "[[prepare_hook]]\nname='project'\nexecutable='project.sh'\nargs=['world']\n",
    )
    .unwrap();
    let loaded = crate::config::parse_and_validate_with_overlay(&global, Some(&project)).unwrap();
    assert_eq!(loaded.prepare_hooks.len(), 2);
    assert_eq!(loaded.prepare_hooks[0].name, "global");
    assert_eq!(
        loaded.prepare_hooks[0].executable,
        root.path().join("global.sh")
    );
    assert_eq!(
        loaded.prepare_hooks[1].executable,
        root.path().join(".ags/project.sh")
    );
    assert_eq!(
        loaded.prepare_hooks[1].project.as_deref(),
        Some(root.path())
    );
    assert_eq!(loaded.prepare_hooks[1].args, ["world"]);
}

#[test]
fn all_prohibited_env_schema_keys_and_prefixes_are_enforced() {
    // The published schema is also the source of the documented reserved keyspace.
    let schema: serde_json::Value = serde_json::from_str(super::protocol::OUTPUT_SCHEMA).unwrap();
    for key in schema["properties"]["env"]["propertyNames"]["allOf"][1]["not"]["anyOf"][0]["enum"]
        .as_array()
        .unwrap()
    {
        assert!(super::protocol::validate_env_name(key.as_str().unwrap()).is_err());
    }
    for key in [
        "AGS_X",
        "OP_X",
        "PNPM_X",
        "MISE_X",
        "LD_X",
        "DCG_X",
        "PI_GUARD_X",
        "CLAUDE_X",
        "GIT_CONFIG_X",
    ] {
        assert!(super::protocol::validate_env_name(key).is_err());
    }
    assert!(super::protocol::validate_env_name("EXAMPLE_TOKEN").is_ok());
    assert!(matches!(
        response(r#"{"version":1,"env":{"X":"yes"}}"#).env["X"],
        EnvValue::Literal(_)
    ));
}

#[test]
fn concurrency_is_bounded_and_expired_phase_does_not_execute_queued_code() {
    let root = tempfile::tempdir().unwrap();
    let store = store(root.path());
    let state = root.path().join("state.json");
    fs::write(&state, "[0,0]").unwrap();
    let code = format!(
        r#"#!/usr/bin/python3
import fcntl, json, sys, time
json.load(sys.stdin)
with open({state:?},'r+') as f:
    fcntl.flock(f,fcntl.LOCK_EX)
    active,maximum=json.load(f); active+=1; maximum=max(active,maximum)
    f.seek(0);json.dump([active,maximum],f);f.truncate();f.flush()
    fcntl.flock(f,fcntl.LOCK_UN)
time.sleep(0.08)
with open({state:?},'r+') as f:
    fcntl.flock(f,fcntl.LOCK_EX)
    active,maximum=json.load(f);f.seek(0);json.dump([active-1,maximum],f);f.truncate()
print('{{"version":1}}')
"#,
        state = state.to_str().unwrap()
    );
    let mut hooks = Vec::new();
    for i in 0..6 {
        let hook = script(root.path(), &format!("bounded{i}"), "");
        fs::write(&hook.executable, &code).unwrap();
        approve_fixture(&store, &hook);
        hooks.push(hook);
    }
    run(
        &hooks,
        &context(root.path()),
        &store,
        Limits {
            concurrency: 2,
            ..limits()
        },
    )
    .unwrap();
    let state_value: Vec<u32> = serde_json::from_slice(&fs::read(&state).unwrap()).unwrap();
    assert_eq!(state_value, [0, 2]);
    let prior = fs::read(&state).unwrap();
    assert!(
        run(
            &hooks,
            &context(root.path()),
            &store,
            Limits {
                phase_timeout: Duration::ZERO,
                ..limits()
            }
        )
        .is_err()
    );
    assert_eq!(fs::read(state).unwrap(), prior);
}

fn build_plan_for(
    materialized: &Materialized,
    cfg: &crate::config::ValidatedConfig,
    work: &Path,
) -> crate::plan::LaunchPlan {
    use crate::config::ClipboardMode;
    use crate::plan::BuildLaunchPlanOptions;
    let secrets = std::collections::HashMap::new();
    crate::plan::build_launch_plan(
        cfg,
        work,
        Agent::Shell,
        BuildLaunchPlanOptions {
            browser_mode: false,
            tmux_mode: false,
            guard_enabled: true,
            lockdown: false,
            ssh_auth_sock: None,
            resolved_secrets: &secrets,
            auth_proxy_runtime_dir: None,
            clipboard_runtime_dir: None,
            clipboard_mode: ClipboardMode::Off,
            host_ui_runtime_dir: None,
            host_ui_session_id: None,
            webview_relay_runtime_dir: None,
            psp_socket: None,
            psp_session_id: None,
            extra_mounts: &materialized.mounts,
            extra_mount_dirs: &[],
            env: &materialized.env,
            stop_when_done: true,
            root_mode: false,
            wayland_passthrough: false,
            payload_fd_count: 0,
            bootstrap_path: None,
            bootstrap_host_path: None,
        },
    )
    .unwrap()
}

#[test]
fn final_plan_rejects_workdir_overlaps_preserves_guards_and_filters_inherited_env() {
    let root = tempfile::tempdir().unwrap();
    let work = root.path().join("work");
    fs::create_dir(&work).unwrap();
    let mut cfg = config(root.path());
    let output = format!(
        r#"{{"version":1,"env":{{"TERM":"hook-term"}},"files":[{{"destination":{},"content":"data"}}]}}"#,
        serde_json::to_string(&work.join("overlap")).unwrap()
    );
    let materialized = Contributions::merge(vec![response(&output)])
        .unwrap()
        .materialize(&mut cfg, &options(&[]))
        .unwrap();
    let mut plan = build_plan_for(&materialized, &cfg, &work);
    assert!(
        materialized
            .validate_plan(&plan)
            .unwrap_err()
            .contains("overlap")
    );
    materialized.finish_plan_env(&mut plan);
    assert!(!plan.env.passthrough_names.iter().any(|s| s == "TERM"));
    assert!(
        plan.env
            .inline
            .iter()
            .any(|(k, v)| k == "TERM" && v == "hook-term")
    );
    assert!(
        plan.security
            .security_opts
            .iter()
            .any(|s| s == "no-new-privileges")
    );
    assert!(
        plan.env
            .inline
            .iter()
            .any(|(k, v)| k == "AGS_SANDBOX" && v == "1")
    );
}

#[test]
fn source_symlink_to_protected_cache_is_rejected_and_exact_default_mount_is_replaced() {
    use crate::config::{MountKind, MountMode, MountWhen, ValidatedMount};
    let root = tempfile::tempdir().unwrap();
    let mut cfg = config(root.path());
    fs::create_dir(&cfg.sandbox.cache_dir).unwrap();
    let alias = root.path().join("cache-alias");
    std::os::unix::fs::symlink(&cfg.sandbox.cache_dir, &alias).unwrap();
    let output = format!(
        r#"{{"version":1,"mounts":[{{"source":{},"destination":"/tmp/source-fixture","mode":"ro"}}]}}"#,
        serde_json::to_string(&alias).unwrap()
    );
    assert!(
        Contributions::merge(vec![response(&output)])
            .unwrap()
            .materialize(&mut cfg, &options(&[]))
            .is_err()
    );
    cfg.mounts.push(ValidatedMount {
        host: root.path().join("old"),
        container: "/tmp/replaced".into(),
        mode: MountMode::Ro,
        kind: MountKind::File,
        when: MountWhen::Always,
        create: false,
        optional: false,
        source: "default".into(),
    });
    let output =
        response(r#"{"version":1,"files":[{"destination":"/tmp/replaced","content":"hook"}]}"#);
    let prepared = Contributions::merge(vec![output])
        .unwrap()
        .materialize(&mut cfg, &options(&[]))
        .unwrap();
    assert!(cfg.mounts.is_empty());
    assert_eq!(
        fs::read_to_string(&prepared.mounts[0].host).unwrap(),
        "hook"
    );
}

#[path = "hooks_review_tests.rs"]
mod review_tests;
