use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use ags::trust::{RepoConfigPrompter, resolve_repo_local_overlay};

struct TestPrompter {
    answer: bool,
    calls: AtomicUsize,
}

impl TestPrompter {
    fn new(answer: bool) -> Self {
        Self {
            answer,
            calls: AtomicUsize::new(0),
        }
    }

    fn calls(&self) -> usize {
        self.calls.load(Ordering::Relaxed)
    }
}

impl RepoConfigPrompter for TestPrompter {
    fn confirm_repo_local_config(&self, _repo_root: &Path, _overlay_path: &Path) -> bool {
        self.calls.fetch_add(1, Ordering::Relaxed);
        self.answer
    }
}

#[test]
fn missing_repo_local_overlay_continues_without_prompt() {
    let Some((_temp, repo_root)) = init_git_repo() else {
        return;
    };
    let prompter = TestPrompter::new(true);
    let trust_store = repo_root.join("../trusted.txt");
    let global_config = repo_root.join("../global-config.toml");

    let overlay = resolve_repo_local_overlay(&repo_root, &global_config, &trust_store, &prompter)
        .expect("trust resolution should succeed");

    assert!(overlay.is_none());
    assert_eq!(prompter.calls(), 0);
    assert!(!trust_store.exists());
}

#[test]
fn denied_repo_local_overlay_is_skipped_without_persisting_trust() {
    let Some((_temp, repo_root)) = init_git_repo() else {
        return;
    };
    let overlay_path = write_overlay(&repo_root);
    let prompter = TestPrompter::new(false);
    let trust_store = repo_root.join("../trusted.txt");
    let global_config = repo_root.join("../global-config.toml");

    let overlay = resolve_repo_local_overlay(&repo_root, &global_config, &trust_store, &prompter)
        .expect("trust resolution should succeed");

    assert!(overlay.is_none());
    assert_eq!(prompter.calls(), 1);
    assert!(!trust_store.exists());
    assert!(overlay_path.exists());
}

#[test]
fn accepted_repo_local_overlay_is_persisted_and_reused() {
    let Some((_temp, repo_root)) = init_git_repo() else {
        return;
    };
    let overlay_path = write_overlay(&repo_root);
    let trust_store = repo_root.join("../trusted.txt");
    let global_config = repo_root.join("../global-config.toml");

    let first_prompt = TestPrompter::new(true);
    let overlay =
        resolve_repo_local_overlay(&repo_root, &global_config, &trust_store, &first_prompt)
            .expect("first trust resolution should succeed");

    assert_eq!(overlay.as_deref(), Some(overlay_path.as_path()));
    assert_eq!(first_prompt.calls(), 1);

    let trusted = fs::read_to_string(&trust_store).expect("trust store should be written");
    assert!(
        trusted.contains(repo_root.canonicalize().unwrap().to_string_lossy().as_ref()),
        "trust store should contain canonical repo root: {trusted}"
    );

    let second_prompt = TestPrompter::new(false);
    let overlay =
        resolve_repo_local_overlay(&repo_root, &global_config, &trust_store, &second_prompt)
            .expect("second trust resolution should succeed");

    assert_eq!(overlay.as_deref(), Some(overlay_path.as_path()));
    assert_eq!(
        second_prompt.calls(),
        0,
        "trusted repo should not prompt again"
    );
}

#[test]
fn overlay_is_ignored_when_it_matches_the_global_config_path() {
    let Some((_temp, repo_root)) = init_git_repo() else {
        return;
    };
    let overlay_path = write_overlay(&repo_root);
    let prompter = TestPrompter::new(true);
    let trust_store = repo_root.join("../trusted.txt");

    let overlay = resolve_repo_local_overlay(&repo_root, &overlay_path, &trust_store, &prompter)
        .expect("trust resolution should succeed");

    assert!(overlay.is_none());
    assert_eq!(prompter.calls(), 0);
}

fn init_git_repo() -> Option<(tempfile::TempDir, PathBuf)> {
    let temp = tempfile::tempdir().ok()?;
    let repo_root = temp.path().join("repo");
    fs::create_dir_all(&repo_root).ok()?;

    let output = std::process::Command::new("git")
        .args(["init", &repo_root.to_string_lossy()])
        .output();
    let Ok(output) = output else {
        eprintln!("git not available, skipping trust test");
        return None;
    };
    if !output.status.success() {
        eprintln!("git init failed, skipping trust test");
        return None;
    }

    Some((temp, repo_root))
}

fn write_overlay(repo_root: &Path) -> PathBuf {
    let overlay = repo_root.join(".ags/config.toml");
    fs::create_dir_all(overlay.parent().unwrap()).unwrap();
    fs::write(&overlay, "[host_ui]\nenabled = false\n").unwrap();
    overlay
}
