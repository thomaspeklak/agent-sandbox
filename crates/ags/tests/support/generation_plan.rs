use super::*;
use ags::agent_runtime::{RUNTIME_DIRS, Update};

#[test]
fn launch_pins_one_generation_read_only_but_keeps_user_data_writable() {
    let toml = minimal_config_toml();
    let config = parse_toml_str(&toml, Path::new("/test/config.toml")).unwrap();
    let cache = &config.sandbox.cache_dir;
    let workdir = tempfile::tempdir().unwrap();
    let first = Update::begin(cache).unwrap();
    first.publish().unwrap();
    let old_plan = build_plan_from(&toml, workdir.path());
    let first_path = first.path.clone();
    drop(first);
    let second = Update::begin(cache).unwrap();
    second.publish().unwrap();
    let new_plan = build_plan_from(&toml, workdir.path());
    for (plan, generation) in [(&old_plan, &first_path), (&new_plan, &second.path)] {
        assert_eq!(&plan.runtime_lease.as_ref().unwrap().path, generation);
        let lease_file = fs::File::open(generation.join(".lease")).unwrap();
        assert!(matches!(
            lease_file.try_lock(),
            Err(fs::TryLockError::WouldBlock)
        ));
        assert_eq!(
            find_plan_env(plan, "DISABLE_AUTOUPDATER").as_deref(),
            Some("1")
        );
        assert_eq!(
            find_plan_env(plan, "OPENCODE_DISABLE_AUTOUPDATE").as_deref(),
            Some("true")
        );
        for suffix in RUNTIME_DIRS {
            let mount = plan
                .mounts
                .iter()
                .find(|mount| mount.host == generation.join(suffix))
                .unwrap();
            assert_eq!(mount.mode, MountMode::Ro);
            assert!(!mount.host.to_string_lossy().contains("/current/"));
        }
        for target in ["/var/cache/ags/pnpm/store", "/var/cache/ags/pnpm/cache"] {
            let mount = plan
                .mounts
                .iter()
                .find(|mount| mount.container == target)
                .unwrap();
            assert_eq!(mount.mode, MountMode::Rw);
            assert!(!mount.host.starts_with(generation));
            assert!(!mount.host.to_string_lossy().contains("agent-downloads"));
        }
        assert!(plan.mounts.iter().all(|mount| {
            !mount
                .host
                .to_string_lossy()
                .contains("agent-downloads/pnpm")
        }));
        for target in ["/home/dev/.pi", "/home/dev/.npm-global"] {
            let mount = plan
                .mounts
                .iter()
                .find(|mount| mount.container == target)
                .unwrap();
            assert_eq!(mount.mode, MountMode::Rw);
            assert!(!mount.host.starts_with(generation));
        }
    }
}
