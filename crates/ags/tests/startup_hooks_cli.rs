use sha2::{Digest, Sha256};
use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::{Command, Stdio};

fn executable(path: &Path, body: &str) {
    fs::write(path, body).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
}
struct Fixture {
    root: tempfile::TempDir,
}
impl Fixture {
    fn new(body: &str) -> Self {
        let root = tempfile::tempdir().unwrap();
        for dir in ["home", "work", "runtime", "bin"] {
            fs::create_dir(root.path().join(dir)).unwrap();
        }
        let hook = root.path().join("fixture-hook");
        executable(&hook, body);
        let config = format!(
            r#"[[prepare_hook]]
name="fixture"
executable="{hook}"
args=[]
timeout_seconds=2
[sandbox]
image="fixture-image"
containerfile="{root}/Containerfile"
cache_dir="{root}/cache"
gitconfig_path="{root}/gitconfig"
auth_key="{root}/auth"
sign_key="{root}/sign"
enabled_agents=[]
[host_ui]
enabled=false
[clipboard]
enabled=false
"#,
            root = root.path().display(),
            hook = hook.display()
        );
        fs::write(root.path().join("config.toml"), config).unwrap();
        Self { root }
    }
    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_ags"));
        command
            .current_dir(self.root.path().join("work"))
            .env("HOME", self.root.path().join("home"))
            .env("XDG_CONFIG_HOME", self.root.path().join("home/config"))
            .env("XDG_RUNTIME_DIR", self.root.path().join("runtime"))
            .env(
                "PATH",
                format!("{}:/usr/bin:/bin", self.root.path().join("bin").display()),
            )
            .env("HOOK_HOST_TOKEN_SENTINEL", "must-not-inherit");
        command
    }
    fn approve_fixture(&self) {
        self.approve_config(&self.root.path().join("config.toml"));
    }
    fn approve_config(&self, path: &Path) {
        let config = ags::config::parse_and_validate(path).unwrap();
        let hook = &config.prepare_hooks[0];
        let declaration = serde_json::to_vec(hook).unwrap();
        let scope = serde_json::to_vec(&hook.project).unwrap();
        let bytes = fs::read(&hook.executable).unwrap();
        let mut hash = Sha256::new();
        hash.update(b"ags-prepare-hook-trust-v1\0");
        hash.update((declaration.len() as u64).to_le_bytes());
        hash.update(declaration);
        hash.update((scope.len() as u64).to_le_bytes());
        hash.update(scope);
        hash.update((bytes.len() as u64).to_le_bytes());
        hash.update(bytes);
        let hash = format!("{:x}", hash.finalize());
        let dir = self.root.path().join("home/.local/state/ags-hook-trust");
        fs::create_dir_all(&dir).unwrap();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
        let token = dir.join(&hash);
        fs::write(&token, hash).unwrap();
        fs::set_permissions(token, fs::Permissions::from_mode(0o600)).unwrap();
    }
    fn config_arg(&self) -> String {
        self.root.path().join("config.toml").display().to_string()
    }
}

#[test]
fn describe_schema_and_validator_are_discoverable_config_free_and_redacted() {
    let dir = tempfile::tempdir().unwrap();
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_ags"))
            .current_dir(dir.path())
            .env("HOME", dir.path())
            .args(args)
            .output()
            .unwrap()
    };
    let output = run(&["hooks", "describe", "prepare"]);
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("HOST USER"));
    for kind in ["input", "output"] {
        let output = run(&["hooks", "schema", "prepare", kind]);
        assert!(output.status.success());
        let schema: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        let published = fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join(format!("../../docs/schemas/prepare-{kind}.schema.json")),
        )
        .unwrap();
        assert_eq!(
            schema,
            serde_json::from_slice::<serde_json::Value>(&published).unwrap()
        );
        assert_eq!(schema["additionalProperties"], false);
    }
    let path = dir.path().join("response.json");
    fs::write(&path,r#"{"version":1,"env":{"API_TOKEN":{"op":"op://vault/item/secret"},"LITERAL":"private-fixture"},"files":[{"destination":"/tmp/fixture","content":"private-content"}]}"#).unwrap();
    let output = run(&["hooks", "validate", path.to_str().unwrap()]);
    assert!(output.status.success());
    let summary = String::from_utf8_lossy(&output.stdout);
    assert!(summary.contains("API_TOKEN"));
    assert!(!summary.contains("private-fixture"));
    assert!(!summary.contains("private-content"));
    assert!(!summary.contains("op://"));
    assert!(!dir.path().join(".local").exists());
    let mut child = Command::new(env!("CARGO_BIN_EXE_ags"))
        .args(["hooks", "validate", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"{\"version\":1}")
        .unwrap();
    assert!(child.wait_with_output().unwrap().status.success());
    fs::write(&path, r#"{"version":1,"env":{"AGS_GUARD_YOLO":"1"}}"#).unwrap();
    assert!(
        !run(&["hooks", "validate", path.to_str().unwrap()])
            .status
            .success()
    );
}

#[test]
fn validator_rejects_configuration_redirectors_even_with_generated_settings() {
    let fixture = Fixture::new("#!/bin/sh\nexit 1\n");
    for key in ["XDG_CONFIG_HOME", "OPENCODE_CONFIG_DIR"] {
        let response = serde_json::json!({
            "version": 1,
            "env": {key: "/tmp/hook-config"},
            "files": [{"destination": "/tmp/hook-config/opencode/opencode.json", "content": "{}"}],
        });
        let mut child = fixture
            .command()
            .args(["hooks", "validate", "-"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(&serde_json::to_vec(&response).unwrap())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains("protected"));
    }
}

#[test]
fn explicit_project_config_spellings_share_scope_executable_and_approval() {
    let fixture = Fixture::new(
        "#!/bin/sh\ncat >/dev/null\nprintf 'executed\\n' >> executed\nprintf '{\"version\":1}'\n",
    );
    let work = fixture.root.path().join("work");
    fs::create_dir(work.join(".ags")).unwrap();
    let path = work.join(".ags/config.toml");
    fs::copy(
        fixture.root.path().join("fixture-hook"),
        work.join(".ags/fixture-hook"),
    )
    .unwrap();
    let config = fs::read_to_string(fixture.root.path().join("config.toml"))
        .unwrap()
        .replace(
            &fixture
                .root
                .path()
                .join("fixture-hook")
                .display()
                .to_string(),
            "fixture-hook",
        );
    fs::write(&path, config).unwrap();
    let loaded = ags::config::parse_and_validate(&path).unwrap();
    assert_eq!(
        loaded.prepare_hooks[0].project,
        Some(work.canonicalize().unwrap())
    );
    assert_eq!(
        loaded.prepare_hooks[0].executable,
        work.join(".ags/fixture-hook")
    );
    // One token made from the absolute spelling must approve all three spellings.
    fixture.approve_config(&path);
    executable(
        &fixture.root.path().join("bin/podman"),
        "#!/bin/sh\nexit 0\n",
    );
    for spelling in [
        ".ags/config.toml",
        "./.ags/config.toml",
        path.to_str().unwrap(),
    ] {
        for args in [
            vec!["hooks", "test", "fixture", "--config", spelling],
            vec!["--agent", "shell", "--lockdown", "--config", spelling],
        ] {
            let output = fixture.command().args(args).output().unwrap();
            assert!(
                output.status.success(),
                "{spelling}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
    assert_eq!(
        fs::read_to_string(work.join("executed"))
            .unwrap()
            .lines()
            .count(),
        6
    );
}

#[test]
fn named_test_executes_only_approved_fixture_code_validates_context_and_does_not_resolve_op() {
    let body = r#"#!/usr/bin/python3
import json, os, sys
context=json.load(sys.stdin)
assert context['event']=='prepare' and context['version']==1 and context['agent']=='shell'
assert 'HOOK_HOST_TOKEN_SENTINEL' not in os.environ
open(os.path.join(context['workdir'],'executed'),'w').write(context['launch_id'])
print(json.dumps({'version':1,'env':{'FIXTURE_TOKEN':{'op':'op://vault/item/field'}}}))
"#;
    let fixture = Fixture::new(body);
    let args = [
        "hooks",
        "test",
        "fixture",
        "--config",
        &fixture.config_arg(),
    ];
    let output = fixture.command().args(args).output().unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("interactive terminal"));
    assert!(!fixture.root.path().join("work/executed").exists());
    fixture.approve_fixture();
    let output = fixture.command().args(args).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(fixture.root.path().join("work/executed").exists());
    assert!(String::from_utf8_lossy(&output.stderr).contains("NOT a side-effect-free dry run"));
    assert!(!String::from_utf8_lossy(&output.stdout).contains("op://"));
    fs::write(
        fixture.root.path().join("fixture-hook"),
        body.replace("FIXTURE_TOKEN", "CHANGED_TOKEN"),
    )
    .unwrap();
    assert!(
        !fixture
            .command()
            .args(args)
            .output()
            .unwrap()
            .status
            .success()
    );
}

#[test]
fn fifo_entrypoint_is_rejected_promptly_before_approval_or_execution() {
    use std::os::unix::ffi::OsStrExt;
    use std::time::{Duration, Instant};
    let fixture = Fixture::new("#!/bin/sh\ntouch executed\n");
    let hook = fixture.root.path().join("fixture-hook");
    fs::remove_file(&hook).unwrap();
    let name = std::ffi::CString::new(hook.as_os_str().as_bytes()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o700) }, 0);
    // There is deliberately no writer. Bound the subprocess externally because
    // initial fingerprinting precedes the hook runner's deadline/signal guard.
    let config_arg = fixture.config_arg();
    for args in [
        vec!["hooks", "test", "fixture", "--config", &config_arg],
        vec!["--agent", "shell", "--config", &config_arg],
    ] {
        let mut child = fixture
            .command()
            .args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        while child.try_wait().unwrap().is_none() {
            if Instant::now() >= deadline {
                child.kill().unwrap();
                let output = child.wait_with_output().unwrap();
                panic!(
                    "FIFO fingerprint blocked: {}",
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        let output = child.wait_with_output().unwrap();
        assert!(!output.status.success());
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(error.contains("executable regular file"), "{error}");
        assert!(!error.contains("new or changed"), "{error}");
        assert!(!fixture.root.path().join("work/executed").exists());
        assert!(
            !fixture
                .root
                .path()
                .join("home/.local/state/ags-hook-trust")
                .exists()
        );
        assert!(!fixture.root.path().join("Containerfile").exists());
    }
}

#[test]
fn hook_bind_sources_cannot_expose_repository_overlay_trust_storage() {
    use std::os::unix::fs::symlink;
    let fixture = Fixture::new("#!/bin/sh\ncat >/dev/null\nprintf '%s' \"$1\"\n");
    let config_path = fixture.root.path().join("config.toml");
    let config = fs::read_to_string(&config_path).unwrap();
    let config_dir = fixture.root.path().join("home/config");
    let trust = config_dir.join("ags/trusted-repo-overlays.txt");
    fs::create_dir_all(trust.parent().unwrap()).unwrap();
    fs::write(&trust, "").unwrap();
    let file_alias = fixture.root.path().join("trust-alias");
    let dir_alias = fixture.root.path().join("config-alias");
    symlink(&trust, &file_alias).unwrap();
    symlink(&config_dir, &dir_alias).unwrap();
    let check = |source: &Path, config_home: &Path| {
        let response = serde_json::json!({"version": 1, "mounts": [{
            "source": source, "destination": "/tmp/data", "mode": "rw"
        }]})
        .to_string();
        let argv = toml::Value::Array(vec![toml::Value::String(response)]).to_string();
        fs::write(
            &config_path,
            config.replace("args=[]", &format!("args={argv}")),
        )
        .unwrap();
        fixture.approve_fixture();
        let output = fixture
            .command()
            .env("XDG_CONFIG_HOME", config_home)
            .args([
                "hooks",
                "test",
                "fixture",
                "--config",
                &fixture.config_arg(),
            ])
            .output()
            .unwrap();
        assert!(
            !output.status.success(),
            "source {} was accepted",
            source.display()
        );
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(error.contains("protected AGS host storage"), "{error}");
    };
    for source in [
        &trust,
        trust.parent().unwrap(),
        &config_dir,
        &file_alias,
        &dir_alias,
    ] {
        check(source, &config_dir);
    }
    // A symlink in the configured storage path must not obscure its canonical
    // ancestor, including before the trust file or its directory exists.
    check(&config_dir, &dir_alias);
    fs::remove_file(&trust).unwrap();
    check(&config_dir, &dir_alias);
    fs::remove_dir(trust.parent().unwrap()).unwrap();
    check(&config_dir, &dir_alias);
    // The trust file itself can also alias storage outside the config tree.
    fs::create_dir(trust.parent().unwrap()).unwrap();
    let external = fixture.root.path().join("external-trust");
    fs::create_dir(&external).unwrap();
    let target = external.join("trusted.txt");
    fs::write(&target, "").unwrap();
    symlink(&target, &trust).unwrap();
    check(&target, &config_dir);
    check(&external, &config_dir);
    fs::remove_file(&target).unwrap();
    check(&external, &config_dir); // Dangling trust aliases fail closed.
}

#[test]
fn unapproved_launch_refuses_before_assets_or_container_creation() {
    let fixture = Fixture::new("#!/bin/sh\ncat >/dev/null\nprintf '{\"version\":1}'\n");
    let output = fixture
        .command()
        .args(["--agent", "shell", "--config", &fixture.config_arg()])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(!fixture.root.path().join("Containerfile").exists());
    assert!(!fixture.root.path().join("cache").exists());
}

#[test]
fn inert_untrusted_overlay_detection_preserves_no_hook_and_explicit_same_config_modes() {
    let dir = tempfile::tempdir().unwrap();
    let repo = dir.path().join("repo");
    fs::create_dir(&repo).unwrap();
    assert!(
        Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(&repo)
            .status()
            .unwrap()
            .success()
    );
    fs::create_dir(repo.join(".ags")).unwrap();
    let overlay = repo.join(".ags/config.toml");
    let global = dir.path().join("global.toml");
    fs::write(&global, "[sandbox]\nimage='unused'\n").unwrap();
    fs::write(
        &overlay,
        "[[prepare_hook]]\nname='untrusted'\nexecutable='/must/not/execute'\n",
    )
    .unwrap();
    let error = ags::trust::refuse_unloaded_hook_overlay(&repo, &global).unwrap_err();
    assert!(error.contains("overlay trust"));
    assert!(error.contains("HOST USER"));
    assert!(ags::trust::refuse_unloaded_hook_overlay(&repo, &overlay).is_ok());
    fs::write(&overlay, "[sandbox]\nimage='override'\n").unwrap();
    assert!(ags::trust::refuse_unloaded_hook_overlay(&repo, &global).is_ok());
    fs::write(&overlay, "prepare_hook=[]\n").unwrap();
    assert!(ags::trust::refuse_unloaded_hook_overlay(&repo, &global).is_ok());
}

#[test]
fn signal_cancellation_terminates_hook_group_and_returns_without_retry() {
    let fixture = Fixture::new(
        r#"#!/usr/bin/python3
import json, os, subprocess, sys, time
context=json.load(sys.stdin)
child=subprocess.Popen(['/bin/sleep','20'])
open(os.path.join(context['workdir'],'child-pid'),'w').write(str(child.pid))
time.sleep(20)
"#,
    );
    fixture.approve_fixture();
    let mut child = fixture
        .command()
        .args([
            "hooks",
            "test",
            "fixture",
            "--config",
            &fixture.config_arg(),
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let marker = fixture.root.path().join("work/child-pid");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while !marker.exists() {
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    unsafe {
        libc::kill(child.id() as i32, libc::SIGTERM);
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        if child.try_wait().unwrap().is_some() {
            break;
        }
        assert!(std::time::Instant::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let output = child.wait_with_output().unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("cancelled"));
    let pid: u32 = fs::read_to_string(marker).unwrap().trim().parse().unwrap();
    // SIGKILL delivery and direct-child reaping can precede grandchild exit.
    // Poll for observed termination; never pass merely because a signal was sent.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(1);
    loop {
        match fs::read_to_string(format!("/proc/{pid}/stat")) {
            Ok(stat)
                if stat
                    .rsplit_once(')')
                    .is_some_and(|(_, s)| s.starts_with(" Z ")) =>
            {
                break;
            }
            Ok(stat) => assert!(
                std::time::Instant::now() < deadline,
                "descendant still alive: {stat}"
            ),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => break,
            Err(e) => panic!("cannot inspect descendant {pid}: {e}"),
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

#[test]
fn effective_launch_batches_only_surviving_refs_and_cleans_materialized_files_and_transport() {
    for fail in [false, true] {
        let fixture = Fixture::new(
            r#"#!/usr/bin/python3
import json, sys
context=json.load(sys.stdin)
print(json.dumps({'version':1,'env':{
    'APP_REGION':'hook-region','TERM':'hook-term',
    'TOKEN_A':{'op':'op://vault/item/field'},'TOKEN_B':{'op':'op://vault/item/field'},
    'LOSING':{'op':'op://vault/item/never-lookup'}},
    'files':[{'destination':'/tmp/fixture-prepared.txt','content':'generated-fixture-data'}]}))
"#,
        );
        fixture.approve_fixture();
        executable(
            &fixture.root.path().join("bin/ssh-agent"),
            "#!/bin/sh\nexit 1\n",
        );
        executable(
            &fixture.root.path().join("bin/op"),
            r#"#!/usr/bin/python3
import os, sys
root=os.path.dirname(os.path.dirname(__file__))
if '--help' in sys.argv:
    print('--in-file --out-file'); sys.exit(0)
assert sys.argv[1:]==['inject']
input=sys.stdin.buffer.read()
assert input==b'{{ op://vault/item/field }}\0', repr(input)
with open(root+'/op-calls','a') as out: out.write('one-batch\n')
sys.stdout.buffer.write(b'OP_RESOLVED_FIXTURE_VALUE\0')
"#,
        );
        executable(
            &fixture.root.path().join("bin/podman"),
            r#"#!/usr/bin/python3
import json, os, sys
args=sys.argv[1:]
root=os.path.dirname(os.path.dirname(__file__))
if args[:2]==['image','exists'] or '--entrypoint' in args or '--name' not in args:
    sys.exit(0)
assert 'OP_RESOLVED_FIXTURE_VALUE' not in repr(args)
assert 'OP_RESOLVED_FIXTURE_VALUE' not in repr(dict(os.environ))
env_file=args[args.index('--env-file')+1]
values=dict(line.rstrip('\n').split('=',1) for line in open(env_file))
assert values['TOKEN_A']==values['TOKEN_B']=='OP_RESOLVED_FIXTURE_VALUE'
assert 'LOSING' not in values and 'TERM' not in values
assert 'LOSING=cli' in args and 'TERM=hook-term' in args and 'APP_REGION=hook-region' in args
volume=next(arg for arg in args if ':/tmp/fixture-prepared.txt:ro' in arg)
source=volume.split(':')[0]
assert open(source).read()=='generated-fixture-data'
with open(root+'/launch-artifacts','w') as out: json.dump([env_file,source],out)
sys.exit(7 if os.environ.get('FIXTURE_FAIL') else 0)
"#,
        );
        let output = fixture
            .command()
            .env("FIXTURE_FAIL", if fail { "yes" } else { "" })
            .args([
                "--agent",
                "shell",
                "--config",
                &fixture.config_arg(),
                "--env",
                "LOSING=cli",
                "--stop-when-done",
            ])
            .output()
            .unwrap();
        assert_eq!(
            output.status.code(),
            Some(if fail { 7 } else { 0 }),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!String::from_utf8_lossy(&output.stderr).contains("OP_RESOLVED_FIXTURE_VALUE"));
        assert!(!String::from_utf8_lossy(&output.stdout).contains("OP_RESOLVED_FIXTURE_VALUE"));
        assert_eq!(
            fs::read_to_string(fixture.root.path().join("op-calls")).unwrap(),
            "one-batch\n"
        );
        let artifacts: Vec<String> = serde_json::from_slice(
            &fs::read(fixture.root.path().join("launch-artifacts")).unwrap(),
        )
        .unwrap();
        for path in artifacts {
            assert!(!Path::new(&path).exists(), "artifact not cleaned: {path}");
        }
    }
}

#[test]
fn lockdown_launch_executes_hook_with_generated_file_but_never_op_and_rejects_host_mounts() {
    let fixture = Fixture::new(
        "#!/bin/sh\ncat >/dev/null\nprintf '%s' '{\"version\":1,\"env\":{\"LOSING\":{\"op\":\"op://v/i/f\"}},\"files\":[{\"destination\":\"/tmp/lockdown-fixture\",\"content\":\"staged\"}]}'\n",
    );
    fixture.approve_fixture();
    executable(
        &fixture.root.path().join("bin/podman"),
        r#"#!/usr/bin/python3
import sys
args=sys.argv[1:]
if '--name' in args:
    assert '--tmpfs' in args and 'LOSING=cli' in args
    assert any(':/tmp/lockdown-fixture:ro' in v for v in args)
sys.exit(0)
"#,
    );
    let output = fixture
        .command()
        .args([
            "--agent",
            "shell",
            "--lockdown",
            "--config",
            &fixture.config_arg(),
            "--env",
            "LOSING=cli",
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let output = fixture
        .command()
        .args([
            "--agent",
            "shell",
            "--lockdown",
            "--config",
            &fixture.config_arg(),
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("surviving prepare hook op references")
    );
}

#[path = "startup_hooks/review.rs"]
mod review_tests;
