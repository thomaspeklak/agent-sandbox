use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Instant;

use ags::cli::{self, CliError, SubCommand};
use sha2::{Digest, Sha256};

fn parse(args: &[&str]) -> Result<cli::Command, CliError> {
    cli::parse_args(
        std::iter::once("ags")
            .chain(args.iter().copied())
            .map(str::to_owned),
    )
}

#[test]
fn cli_defaults_overrides_and_invalid_budgets() {
    let cli::Command::Sub(SubCommand::PruneWorkspaceCaches(defaults)) =
        parse(&["prune-workspace-caches"]).unwrap()
    else {
        panic!()
    };
    assert_eq!(defaults.collection_options().max_deletions, 1000);
    assert_eq!(defaults.collection_options().max_caches, 2);
    assert_eq!(defaults.collection_options().grace.as_secs(), 7 * 86400);
    let cli::Command::Sub(SubCommand::PruneWorkspaceCaches(opts)) = parse(&[
        "prune-workspace-caches",
        "--config=host.toml",
        "--dry-run",
        "--grace-days",
        "0",
        "--max-caches=1",
        "--max-deletions",
        "3",
        "--quiet",
    ])
    .unwrap() else {
        panic!()
    };
    assert_eq!(opts.config_path, Some("host.toml".into()));
    assert!(opts.dry_run && opts.quiet);
    assert_eq!(opts.collection_options().grace.as_secs(), 0);
    for args in [
        vec!["--grace-days", "18446744073709551615"],
        vec!["--grace-days", "-1"],
        vec!["--grace-days"],
        vec!["--max-caches", "0"],
        vec!["--max-deletions=2"],
        vec!["--max-deletions=garbage"],
        vec!["--unknown"],
        vec!["positional"],
    ] {
        let mut command = vec!["prune-workspace-caches"];
        command.extend(args);
        assert!(parse(&command).is_err());
    }
}

fn executable(path: &Path, content: &str) {
    fs::write(path, content).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

struct Fixture {
    dir: tempfile::TempDir,
    config: PathBuf,
    cache: PathBuf,
    orphan: PathBuf,
    log: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path().join("cache");
        let worktree = dir.path().join("gone");
        let mut hash = Sha256::new();
        hash.update(worktree.to_str().unwrap().as_bytes());
        hash.update([0]);
        hash.update(0_u64.to_le_bytes());
        hash.update(0_u64.to_le_bytes());
        let orphan = cache
            .join("workspace-caches")
            .join(format!("{:x}", hash.finalize()));
        fs::create_dir_all(orphan.join("pnpm-store")).unwrap();
        fs::write(orphan.join("pnpm-store/package"), b"data").unwrap();
        fs::write(
            orphan.join("identity.json"),
            serde_json::to_vec(&serde_json::json!({
                "worktree": worktree, "anchor_device": 0, "anchor_inode": 0,
                "checkout_incarnation": null
            }))
            .unwrap(),
        )
        .unwrap();
        let config = dir.path().join("config.toml");
        fs::write(
            &config,
            format!(
                r#"[sandbox]
image = "localhost/agent-sandbox:latest"
containerfile = "{root}/Containerfile"
cache_dir = "{cache}"
gitconfig_path = "{root}/gitconfig"
auth_key = "{root}/auth"
sign_key = "{root}/sign"
"#,
                root = dir.path().display(),
                cache = cache.display()
            ),
        )
        .unwrap();
        let bin = dir.path().join("bin");
        fs::create_dir(&bin).unwrap();
        executable(
            &bin.join("podman"),
            r#"#!/bin/sh
printf '%s\n' "$*" >> "$EVENTS"
if [ "$FAIL_INSPECT" = 1 ]; then exit 1; fi
if [ "$SLOW_INSPECT" = 1 ]; then sleep 30; fi
case "$1" in
  ps) printf 'container-id\n' ;;
  container) printf '%s\n' "$INSPECTION" ;;
  *) exit 2 ;;
esac
"#,
        );
        for program in ["git", "curl", "op", "ssh-agent"] {
            executable(
                &bin.join(program),
                "#!/bin/sh\nprintf 'unexpected command\n' >> \"$EVENTS\"\nexit 99\n",
            );
        }
        let log = dir.path().join("events");
        Self {
            dir,
            config,
            cache,
            orphan,
            log,
        }
    }

    fn command(&self) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_ags"));
        let path = std::env::join_paths(std::iter::once(self.dir.path().join("bin")).chain(
            std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
        ))
        .unwrap();
        command
            .args(["prune-workspace-caches", "--config"])
            .arg(&self.config)
            .env("PATH", path)
            .env("EVENTS", &self.log)
            .env("INSPECTION", r#"[{"Mounts":[]}]"#)
            .env("XDG_CACHE_HOME", self.dir.path().join("cache-home"))
            .env("XDG_CONFIG_HOME", self.dir.path().join("config-home"))
            .current_dir(self.dir.path())
            .stdin(std::process::Stdio::null());
        command
    }
}

#[test]
fn scheduled_noop_never_inspects_or_launches_other_commands() {
    let fixture = Fixture::new();
    // A repository overlay must not be read or prompt from cron's cwd.
    fs::create_dir(fixture.dir.path().join(".git")).unwrap();
    fs::write(fixture.dir.path().join(".ags.toml"), "not valid TOML").unwrap();
    let output = fixture.command().arg("--quiet").output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
    assert!(!fixture.log.exists());
    assert!(!fixture.dir.path().join("cache-home").exists());
    assert!(fixture.orphan.join(".orphaned").exists());
}

#[test]
fn actual_inventory_protects_stopped_mount_and_inspection_failures() {
    let fixture = Fixture::new();
    let inspection = serde_json::json!([{"State": {"Status": "exited"}, "Mounts": [{"Source": fixture.orphan.join("pnpm-store")}]}]);
    let output = fixture
        .command()
        .args(["--grace-days", "0"])
        .env("INSPECTION", inspection.to_string())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(fixture.orphan.join("pnpm-store/package").exists());
    for inspection in ["broken", "[]", "[{}]", "[{\"Mounts\":[{}]}]"] {
        let output = fixture
            .command()
            .args(["--grace-days", "0"])
            .env("INSPECTION", inspection)
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert!(fixture.orphan.exists());
    }
    let output = fixture
        .command()
        .args(["--grace-days", "0"])
        .env("FAIL_INSPECT", "1")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(fixture.orphan.exists());
}

#[test]
fn dry_run_then_delete_only_orphan_with_one_batched_inventory_each() {
    let fixture = Fixture::new();
    fs::create_dir_all(fixture.cache.join("agent-downloads")).unwrap();
    fs::write(fixture.cache.join("agent-downloads/keep"), b"keep").unwrap();
    let output = fixture
        .command()
        .args(["--dry-run", "--grace-days=0"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("would prune"));
    assert!(fixture.orphan.exists());
    assert!(!fixture.orphan.join(".orphaned").exists());
    let output = fixture
        .command()
        .args(["--grace-days=0", "--quiet"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!fixture.orphan.exists());
    assert!(fixture.cache.join("agent-downloads/keep").exists());
    assert_eq!(fs::read_to_string(&fixture.log).unwrap().lines().count(), 4);
}

#[test]
fn missing_config_fails_without_bootstrapping_it() {
    let fixture = Fixture::new();
    fs::remove_file(&fixture.config).unwrap();
    let output = fixture.command().output().unwrap();
    assert!(!output.status.success());
    assert!(!fixture.config.exists());
    assert!(!fixture.log.exists());
}

#[test]
fn slow_podman_inventory_times_out_without_deleting() {
    let fixture = Fixture::new();
    let start = Instant::now();
    let output = fixture
        .command()
        .args(["--grace-days=0"])
        .env("SLOW_INSPECT", "1")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("five seconds"));
    assert!(start.elapsed().as_secs() < 10);
    assert!(fixture.orphan.exists());
}
