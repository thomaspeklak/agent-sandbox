use std::fs;
use std::path::PathBuf;

use ags::git;

#[test]
fn parse_dot_git_file_standard() {
    let content = "gitdir: /home/user/repos/main/.git/worktrees/feature-branch\n";
    let result = git::parse_dot_git_file(content);
    assert_eq!(
        result,
        Some(PathBuf::from(
            "/home/user/repos/main/.git/worktrees/feature-branch"
        ))
    );
}

#[test]
fn parse_dot_git_file_with_spaces() {
    let content = "gitdir:   /path/with spaces/repo/.git/worktrees/branch  \n";
    let result = git::parse_dot_git_file(content);
    assert_eq!(
        result,
        Some(PathBuf::from(
            "/path/with spaces/repo/.git/worktrees/branch"
        ))
    );
}

#[test]
fn parse_dot_git_file_empty_path() {
    assert_eq!(git::parse_dot_git_file("gitdir:   "), None);
}

#[test]
fn parse_dot_git_file_no_prefix() {
    assert_eq!(git::parse_dot_git_file("/some/random/path"), None);
}

#[test]
fn parse_dot_git_file_empty_input() {
    assert_eq!(git::parse_dot_git_file(""), None);
}

#[test]
fn parse_dot_git_file_relative_path() {
    let content = "gitdir: ../../.git/worktrees/my-branch\n";
    let result = git::parse_dot_git_file(content);
    assert_eq!(
        result,
        Some(PathBuf::from("../../.git/worktrees/my-branch"))
    );
}

#[test]
fn ensure_gitconfig_creates_file_when_missing() {
    let dir = tempdir();
    let gitconfig = dir.join("sandbox-gitconfig");

    let result = git::ensure_gitconfig(&gitconfig, "/home/dev/.ssh/ags-agent-signing.pub");
    assert!(result.is_ok(), "ensure_gitconfig failed: {result:?}");
    assert!(gitconfig.exists());

    let content = fs::read_to_string(&gitconfig).unwrap();
    assert!(content.contains("[user]"));
    assert!(content.contains("[commit]"));
    assert!(content.contains("gpgsign = true"));
    assert!(content.contains("[gpg]"));
    assert!(content.contains("format = ssh"));
    assert!(content.contains("signingkey = /home/dev/.ssh/ags-agent-signing.pub"));
}

#[test]
fn ensure_gitconfig_noop_when_exists() {
    let dir = tempdir();
    let gitconfig = dir.join("sandbox-gitconfig");

    fs::write(&gitconfig, "existing content").unwrap();

    let result = git::ensure_gitconfig(&gitconfig, "/home/dev/.ssh/key.pub");
    assert!(result.is_ok());

    let content = fs::read_to_string(&gitconfig).unwrap();
    assert_eq!(content, "existing content");
}

#[test]
fn ensure_gitconfig_creates_parent_dirs() {
    let dir = tempdir();
    let gitconfig = dir.join("nested").join("dir").join("gitconfig");

    let result = git::ensure_gitconfig(&gitconfig, "/key.pub");
    assert!(result.is_ok());
    assert!(gitconfig.exists());
}

#[test]
fn discover_external_mounts_non_git_dir() {
    let dir = tempdir();
    let result = git::discover_external_git_mounts(&dir);
    assert!(result.paths.is_empty());
}

#[test]
fn discover_external_mounts_normal_repo() {
    let dir = tempdir();

    // Init a normal git repo - git metadata is inside workdir
    let status = std::process::Command::new("git")
        .args(["init", &dir.to_string_lossy()])
        .output();

    if status.is_err() {
        eprintln!("git not available, skipping test");
        return;
    }

    let result = git::discover_external_git_mounts(&dir);
    // Normal repo: .git is inside workdir, so no external mounts
    assert!(
        result.paths.is_empty(),
        "Expected no external mounts for normal repo, got: {:?}",
        result.paths
    );
}

#[test]
fn discover_external_mounts_worktree() {
    let base = tempdir();
    let main_repo = base.join("main");
    let worktree = base.join("worktree");

    // Create main repo with a commit
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .output()
            .ok()
            .filter(|o| o.status.success())
    };

    if git(&["init", &main_repo.to_string_lossy()]).is_none() {
        eprintln!("git not available, skipping");
        return;
    }

    git(&[
        "-C",
        &main_repo.to_string_lossy(),
        "config",
        "user.email",
        "test@test.com",
    ]);
    git(&[
        "-C",
        &main_repo.to_string_lossy(),
        "config",
        "user.name",
        "Test",
    ]);
    git(&[
        "-C",
        &main_repo.to_string_lossy(),
        "commit",
        "--allow-empty",
        "-m",
        "init",
    ]);

    // Create a worktree
    let wt_result = git(&[
        "-C",
        &main_repo.to_string_lossy(),
        "worktree",
        "add",
        &worktree.to_string_lossy(),
        "-b",
        "test-branch",
    ]);

    if wt_result.is_none() {
        eprintln!("git worktree not available, skipping");
        return;
    }

    let result = git::discover_external_git_mounts(&worktree);
    // Worktree metadata lives under main_repo/.git, which is outside the worktree
    assert!(
        !result.paths.is_empty(),
        "Expected external mounts for worktree, got none"
    );

    // The mounted path(s) should be under the main repo
    let main_repo_canonical = main_repo.canonicalize().unwrap();
    for mount_path in &result.paths {
        assert!(
            mount_path.starts_with(&main_repo_canonical),
            "Expected mount path {mount_path:?} to be under main repo {main_repo_canonical:?}"
        );
    }
}

#[test]
fn worktree_parent_repo_dir_none_for_normal_repo() {
    let dir = tempdir();

    let status = std::process::Command::new("git")
        .args(["init", &dir.to_string_lossy()])
        .output();

    if status.is_err() {
        eprintln!("git not available, skipping test");
        return;
    }

    let parent = git::worktree_parent_repo_dir(&dir);
    assert!(parent.is_none());
}

#[test]
fn worktree_parent_repo_dir_for_linked_worktree() {
    let base = tempdir();
    let main_repo = base.join("main");
    let worktree = base.join("worktree");

    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .output()
            .ok()
            .filter(|o| o.status.success())
    };

    if git(&["init", &main_repo.to_string_lossy()]).is_none() {
        eprintln!("git not available, skipping");
        return;
    }

    git(&[
        "-C",
        &main_repo.to_string_lossy(),
        "config",
        "user.email",
        "test@test.com",
    ]);
    git(&[
        "-C",
        &main_repo.to_string_lossy(),
        "config",
        "user.name",
        "Test",
    ]);
    git(&[
        "-C",
        &main_repo.to_string_lossy(),
        "commit",
        "--allow-empty",
        "-m",
        "init",
    ]);

    let wt_result = git(&[
        "-C",
        &main_repo.to_string_lossy(),
        "worktree",
        "add",
        &worktree.to_string_lossy(),
        "-b",
        "test-branch",
    ]);

    if wt_result.is_none() {
        eprintln!("git worktree not available, skipping");
        return;
    }

    let parent = git::worktree_parent_repo_dir(&worktree).expect("expected parent repo dir");
    assert_eq!(
        parent.canonicalize().unwrap(),
        main_repo.canonicalize().unwrap()
    );
}

fn tempdir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ags-git-test-{}", std::process::id()));
    let unique = dir.join(format!("{}", rand_u32()));
    fs::create_dir_all(&unique).expect("failed to create temp dir");
    unique
}

fn rand_u32() -> u32 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    use std::time::SystemTime;
    let mut hasher = DefaultHasher::new();
    SystemTime::now().hash(&mut hasher);
    std::thread::current().id().hash(&mut hasher);
    hasher.finish() as u32
}
