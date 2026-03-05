use std::path::{Path, PathBuf};

use ags::config::MountMode;
use ags::plan::{LaunchPlan, PlanEnv, PlanMount, SecurityConfig, WorkdirMapping};
use ags::podman::{build_run_args, write_env_file};

fn minimal_plan() -> LaunchPlan {
    LaunchPlan {
        image: "localhost/agent-sandbox:latest".to_owned(),
        containerfile: PathBuf::from("/tmp/Containerfile"),
        container_name: "ags-project-abcd".to_owned(),
        workdir: WorkdirMapping {
            host: PathBuf::from("/home/user/project"),
            container: "/home/user/project".to_owned(),
        },
        mounts: vec![PlanMount {
            host: PathBuf::from("/home/user/project"),
            container: "/home/user/project".to_owned(),
            mode: MountMode::Rw,
        }],
        env: PlanEnv {
            inline: vec![
                ("HOME".to_owned(), "/home/dev".to_owned()),
                ("SSH_AUTH_SOCK".to_owned(), "/ssh-agent".to_owned()),
            ],
            passthrough_names: vec!["TERM".to_owned()],
            env_file_entries: vec![("GH_TOKEN".to_owned(), "ghp_test".to_owned())],
            read_roots_json: r#"["/home/user/project","/tmp"]"#.to_owned(),
            write_roots_json: r#"["/home/user/project","/tmp"]"#.to_owned(),
        },
        security: SecurityConfig::default(),
        network_mode: "slirp4netns:allow_host_loopback=false".to_owned(),
        boot_dirs: vec!["/home/dev/.ssh".to_owned()],
        entrypoint: "exec pi \"$@\"".to_owned(),
    }
}

#[test]
fn args_start_with_run() {
    let plan = minimal_plan();
    let args = build_run_args(&plan, Path::new("/tmp/env"));
    assert_eq!(args[0], "run");
}

#[test]
fn args_include_rm_and_it() {
    let plan = minimal_plan();
    let args = build_run_args(&plan, Path::new("/tmp/env"));
    assert!(args.contains(&"--rm".to_owned()));
    assert!(args.contains(&"-it".to_owned()));
}

#[test]
fn args_include_security_flags() {
    let plan = minimal_plan();
    let args = build_run_args(&plan, Path::new("/tmp/env"));
    assert!(args.contains(&"--userns=keep-id".to_owned()));
    assert!(args.contains(&"--security-opt=no-new-privileges".to_owned()));
    assert!(args.contains(&"--security-opt=label=disable".to_owned()));
    assert!(args.contains(&"--cap-drop=all".to_owned()));
    assert!(args.contains(&"--pids-limit=4096".to_owned()));
}

#[test]
fn args_include_network_mode() {
    let plan = minimal_plan();
    let args = build_run_args(&plan, Path::new("/tmp/env"));
    let net_idx = args.iter().position(|a| a == "--network").unwrap();
    assert_eq!(args[net_idx + 1], "slirp4netns:allow_host_loopback=false");
}

#[test]
fn args_include_container_name() {
    let plan = minimal_plan();
    let args = build_run_args(&plan, Path::new("/tmp/env"));

    let idx = args.iter().position(|a| a == "--name").unwrap();
    assert_eq!(args[idx + 1], "ags-project-abcd");
}

#[test]
fn args_include_inline_env() {
    let plan = minimal_plan();
    let args = build_run_args(&plan, Path::new("/tmp/env"));

    // Find -e HOME=/home/dev
    let has_home = args
        .windows(2)
        .any(|w| w[0] == "-e" && w[1] == "HOME=/home/dev");
    assert!(has_home, "should have HOME env var");
}

#[test]
fn args_include_passthrough_env_names() {
    let plan = minimal_plan();
    let args = build_run_args(&plan, Path::new("/tmp/env"));

    let has_term = args.windows(2).any(|w| w[0] == "-e" && w[1] == "TERM");
    assert!(has_term, "should have TERM passthrough");
}

#[test]
fn args_include_guard_roots() {
    let plan = minimal_plan();
    let args = build_run_args(&plan, Path::new("/tmp/env"));

    let has_read = args
        .windows(2)
        .any(|w| w[0] == "-e" && w[1].starts_with("AGS_GUARD_READ_ROOTS_JSON="));
    let has_write = args
        .windows(2)
        .any(|w| w[0] == "-e" && w[1].starts_with("AGS_GUARD_WRITE_ROOTS_JSON="));
    assert!(has_read, "should have read roots");
    assert!(has_write, "should have write roots");
}

#[test]
fn args_include_env_file() {
    let plan = minimal_plan();
    let args = build_run_args(&plan, Path::new("/tmp/my-env"));

    let ef_idx = args.iter().position(|a| a == "--env-file").unwrap();
    assert_eq!(args[ef_idx + 1], "/tmp/my-env");
}

#[test]
fn args_include_volume_mounts() {
    let plan = minimal_plan();
    let args = build_run_args(&plan, Path::new("/tmp/env"));

    let has_volume = args
        .windows(2)
        .any(|w| w[0] == "-v" && w[1] == "/home/user/project:/home/user/project:rw,z");
    assert!(has_volume, "should have workdir volume mount");
}

#[test]
fn args_include_workdir() {
    let plan = minimal_plan();
    let args = build_run_args(&plan, Path::new("/tmp/env"));

    let w_idx = args.iter().position(|a| a == "-w").unwrap();
    assert_eq!(args[w_idx + 1], "/home/user/project");
}

#[test]
fn args_end_with_image_and_entrypoint() {
    let plan = minimal_plan();
    let args = build_run_args(&plan, Path::new("/tmp/env"));

    // Image should be near the end, followed by bash -lc <entrypoint> _
    let img_idx = args
        .iter()
        .position(|a| a == "localhost/agent-sandbox:latest")
        .unwrap();
    assert_eq!(args[img_idx + 1], "bash");
    assert_eq!(args[img_idx + 2], "-lc");
    assert_eq!(args[img_idx + 3], "exec pi \"$@\"");
    assert_eq!(args[img_idx + 4], "_");
}

#[test]
fn args_with_multiple_mounts() {
    let mut plan = minimal_plan();
    plan.mounts.push(PlanMount {
        host: PathBuf::from("/home/user/.config"),
        container: "/home/dev/.config".to_owned(),
        mode: MountMode::Ro,
    });

    let args = build_run_args(&plan, Path::new("/tmp/env"));

    let volume_count = args.iter().filter(|a| *a == "-v").count();
    assert_eq!(volume_count, 2);
}

#[test]
fn args_pull_never() {
    let plan = minimal_plan();
    let args = build_run_args(&plan, Path::new("/tmp/env"));
    assert!(args.contains(&"--pull=never".to_owned()));
}

#[test]
fn write_env_file_creates_file() {
    let dir = tempfile::tempdir().unwrap();
    let entries = vec![
        ("KEY1".to_owned(), "val1".to_owned()),
        ("KEY2".to_owned(), "val2".to_owned()),
    ];

    let path = write_env_file(&entries, dir.path()).unwrap();

    assert!(path.exists());
    let content = std::fs::read_to_string(&path).unwrap();
    assert!(content.contains("KEY1=val1\n"));
    assert!(content.contains("KEY2=val2\n"));
}

#[test]
fn write_env_file_restricted_permissions() {
    let dir = tempfile::tempdir().unwrap();
    let entries = vec![("SECRET".to_owned(), "value".to_owned())];

    let path = write_env_file(&entries, dir.path()).unwrap();

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "env file should be mode 0600");
    }
}

#[test]
fn write_env_file_empty_entries() {
    let dir = tempfile::tempdir().unwrap();
    let path = write_env_file(&[], dir.path()).unwrap();

    let content = std::fs::read_to_string(&path).unwrap();
    assert!(content.is_empty());
}

#[test]
fn error_display() {
    let err = ags::podman::PodmanError::ImageBuild("failed".into());
    assert!(err.to_string().contains("image build failed"));

    let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied");
    let err = ags::podman::PodmanError::EnvFileCreate(io_err);
    assert!(err.to_string().contains("env file"));
}
