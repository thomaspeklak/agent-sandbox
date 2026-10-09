//! Regression coverage for PR25 review findings; only synthetic hooks/approvals.
use super::*;
use std::os::unix::fs::{MetadataExt, PermissionsExt};

fn trust_entries(store: &TrustStore) -> Vec<String> {
    let mut entries: Vec<_> = fs::read_dir(&store.path)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    entries.sort();
    entries
}

#[test]
fn execution_directory_is_private_inside_validated_trust_storage() {
    let root = tempfile::tempdir().unwrap();
    let store = store(root.path());
    let dir = store.execution_directory().unwrap();
    assert_eq!(dir.path().parent(), Some(store.path.as_path()));
    assert!(
        dir.path()
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("ags-hook-exec-")
    );
    let meta = fs::symlink_metadata(dir.path()).unwrap();
    assert!(meta.is_dir());
    assert_eq!(meta.uid(), unsafe { libc::geteuid() });
    assert_eq!(meta.mode() & 0o777, 0o700);
    let path = dir.path().to_owned();
    drop(dir);
    assert!(!path.exists());
    fs::set_permissions(&store.path, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(store.execution_directory().unwrap_err().contains("private"));
    fs::remove_dir(&store.path).unwrap();
    std::os::unix::fs::symlink(root.path(), &store.path).unwrap();
    assert!(
        store
            .execution_directory()
            .unwrap_err()
            .contains("not a symlink")
    );
}

#[test]
fn runner_executes_state_snapshot_and_cleans_only_its_owned_directories() {
    let root = tempfile::tempdir().unwrap();
    let store = store(root.path());
    let entrypoint = root.path().join("observed-entrypoint");
    let hook = script(
        root.path(),
        "fixture",
        &format!(
            "printf '%s' \"$0\" > '{}'; cat >/dev/null; printf '{{\"version\":1}}'",
            entrypoint.display()
        ),
    );
    approve_fixture(&store, &hook);
    // A separate live execution directory must survive the runner's cleanup.
    let peer = store.execution_directory().unwrap();
    let before = trust_entries(&store);
    run(&[hook], &context(root.path()), &store, limits()).unwrap();
    assert_eq!(trust_entries(&store), before);
    let executed = std::path::PathBuf::from(fs::read_to_string(entrypoint).unwrap());
    assert!(executed.starts_with(&store.path));
    assert_eq!(executed.file_name().unwrap(), "fixture");
    assert!(!executed.exists());
    assert!(peer.path().exists());
}

#[test]
fn state_snapshots_clean_up_on_failure_timeout_and_spawn_error() {
    let root = tempfile::tempdir().unwrap();
    let store = store(root.path());
    for (name, body, message) in [
        ("failure", "exit 9", "exit"),
        ("timeout", "exec sleep 3", "timed out"),
        ("invalid", "exit 0", "could not execute host program"),
    ] {
        let hook = script(root.path(), name, body);
        if name == "invalid" {
            fs::write(&hook.executable, "#!/must/not/exist\n").unwrap();
        }
        approve_fixture(&store, &hook);
        let before = trust_entries(&store);
        let error = run(&[hook], &context(root.path()), &store, limits()).unwrap_err();
        assert!(error.contains(message), "{error}");
        if name == "invalid" {
            assert!(
                error.contains("state filesystem permits execution"),
                "{error}"
            );
        }
        assert_eq!(trust_entries(&store), before);
    }
}

#[test]
fn hook_limits_are_per_config_and_overlay_paths_keep_their_scope() {
    let root = tempfile::tempdir().unwrap();
    fs::create_dir(root.path().join(".ags")).unwrap();
    let global = root.path().join("global.toml");
    let overlay = root.path().join(".ags/config.toml");
    let declarations = |count: usize, prefix: &str| {
        (0..count)
            .map(|i| format!("\n[[prepare_hook]]\nname='{prefix}-{i}'\nexecutable='{prefix}.sh'\n"))
            .collect::<String>()
    };
    let base = format!(
        "[sandbox]\nimage='test'\ncontainerfile='{0}/Containerfile'\ncache_dir='{0}/cache'\ngitconfig_path='{0}/gitconfig'\nauth_key='{0}/auth'\nsign_key='{0}/sign'\n",
        root.path().display()
    );
    fs::write(&global, format!("{base}{}", declarations(40, "global"))).unwrap();
    fs::write(&overlay, declarations(30, "project")).unwrap();
    let loaded = crate::config::parse_and_validate_with_overlay(&global, Some(&overlay)).unwrap();
    assert_eq!(loaded.prepare_hooks.len(), 70);
    for hook in &loaded.prepare_hooks[..40] {
        assert_eq!(hook.executable, root.path().join("global.sh"));
        assert_eq!(hook.project, None);
    }
    for hook in &loaded.prepare_hooks[40..] {
        assert_eq!(hook.executable, root.path().join(".ags/project.sh"));
        assert_eq!(hook.project.as_deref(), Some(root.path()));
    }
    let standalone =
        crate::config::parse_toml_str(&format!("{base}{}", declarations(1, "standalone")), &global)
            .unwrap();
    assert_eq!(standalone.prepare_hooks.len(), 1);
    fs::write(&overlay, declarations(65, "project")).unwrap();
    assert!(
        crate::config::parse_and_validate_with_overlay(&global, Some(&overlay))
            .unwrap_err()
            .to_string()
            .contains("at most 64")
    );
    assert!(
        crate::config::parse_toml_str(&format!("{base}{}", declarations(65, "global")), &global)
            .unwrap_err()
            .to_string()
            .contains("at most 64")
    );
    fs::write(&overlay, declarations(30, "project")).unwrap();
    fs::write(&global, format!("{base}{}", declarations(65, "global"))).unwrap();
    assert!(
        crate::config::parse_and_validate_with_overlay(&global, Some(&overlay))
            .unwrap_err()
            .to_string()
            .contains("at most 64")
    );
}

#[test]
fn state_snapshots_clean_up_after_peer_cancellation() {
    let root = tempfile::tempdir().unwrap();
    let store = store(root.path());
    let started = root.path().join("started");
    let failure = script(
        root.path(),
        "failure",
        &format!(
            "while [ ! -f '{}' ]; do sleep 0.01; done; exit 9",
            started.display()
        ),
    );
    let peer = script(
        root.path(),
        "peer",
        &format!("touch '{}'; exec sleep 5", started.display()),
    );
    approve_fixture(&store, &failure);
    approve_fixture(&store, &peer);
    let before = trust_entries(&store);
    let error = run(&[failure, peer], &context(root.path()), &store, limits()).unwrap_err();
    assert!(started.exists());
    assert!(!error.contains("cancelled"), "{error}");
    assert_eq!(trust_entries(&store), before);
}
