use std::collections::BTreeSet;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use ags::assets::{IMAGE_RECIPES, write_image_build_snapshot};
use ags::podman::{ImageBuild, LayerCache, PullPolicy, build_image_args};

fn build(pull: PullPolicy, cache: LayerCache) -> Vec<String> {
    build_image_args(&ImageBuild {
        containerfile: Path::new("/snap/image/rust.Containerfile"),
        context_dir: Path::new("/snap/image"),
        tag: "localhost/ags-build-0123456789abcdef/rust:candidate",
        iidfile: Path::new("/snap/.iid-rust"),
        pull,
        cache,
        build_args: &[("RUSTC_VERSION", "1.92.0 (abcdef012 2026-01-01)".to_owned())],
        labels: &[("io.ags.image.component", "rust".to_owned())],
    })
}

#[test]
fn image_builds_state_layer_and_pull_policy_explicitly() {
    let args = build(PullPolicy::Never, LayerCache::Reuse);
    assert_eq!(&args[..3], ["build", "--layers=true", "--pull=never"]);
    assert!(!args.contains(&"--no-cache".to_owned()));
    assert!(!args.contains(&"--pull".to_owned()));
    assert!(
        args.windows(2)
            .any(|pair| pair == ["--iidfile", "/snap/.iid-rust"])
    );
    assert!(
        args.windows(2)
            .any(|pair| pair == ["-t", "localhost/ags-build-0123456789abcdef/rust:candidate"])
    );
    assert!(
        args.windows(2)
            .any(|pair| pair == ["--label", "io.ags.image.component=rust"])
    );
    // A value with spaces stays one argument.
    assert!(
        args.windows(2)
            .any(|pair| pair == ["--build-arg", "RUSTC_VERSION=1.92.0 (abcdef012 2026-01-01)"])
    );
    assert_eq!(args.last().unwrap(), "/snap/image");

    assert!(build(PullPolicy::Always, LayerCache::Reuse).contains(&"--pull=always".to_owned()));
    assert!(build(PullPolicy::Missing, LayerCache::Reuse).contains(&"--pull=missing".to_owned()));
    let rebuild = build(PullPolicy::Never, LayerCache::Rebuild);
    assert!(rebuild.contains(&"--no-cache".to_owned()));
    assert!(rebuild.contains(&"--layers=true".to_owned()));
}

fn snapshot() -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("snapshot");
    write_image_build_snapshot(&root).unwrap();
    (dir, root)
}

#[test]
fn snapshot_contains_every_recipe_and_the_glimpse_crate() {
    let (_dir, root) = snapshot();
    for path in [
        "Containerfile",
        "tmux.conf",
        "uv.toml",
        "glimpse-shim/Cargo.toml",
        "glimpse-shim/Cargo.lock",
        "glimpse-shim/src/main.rs",
    ] {
        assert!(root.join(path).is_file(), "{path}");
    }
    for (name, content) in IMAGE_RECIPES {
        assert_eq!(
            std::fs::read_to_string(root.join("image").join(name)).unwrap(),
            *content
        );
    }
    assert_eq!(
        std::fs::read_to_string(root.join("Containerfile")).unwrap(),
        ags::assets::CONTAINERFILE
    );
}

#[test]
fn unchanged_assets_are_not_rewritten() {
    let (_dir, root) = snapshot();
    let recipe = root.join("image/rust.Containerfile");
    let before = std::fs::metadata(&recipe).unwrap();
    write_image_build_snapshot(&root).unwrap();
    let after = std::fs::metadata(&recipe).unwrap();
    // An atomic replacement would produce a new inode.
    assert_eq!(before.ino(), after.ino());
    assert_eq!(before.mtime_nsec(), after.mtime_nsec());

    std::fs::write(&recipe, "stale").unwrap();
    write_image_build_snapshot(&root).unwrap();
    assert_eq!(
        std::fs::read_to_string(&recipe).unwrap(),
        ags::assets::image_recipe("rust.Containerfile")
    );
}

fn lock_packages(lock: &str) -> BTreeSet<(String, String, Option<String>, Option<String>)> {
    let value: toml::Value = toml::from_str(lock).unwrap();
    value["package"]
        .as_array()
        .unwrap()
        .iter()
        .map(|package| {
            let field = |name: &str| {
                package
                    .get(name)
                    .and_then(|v| v.as_str())
                    .map(str::to_owned)
            };
            (
                field("name").unwrap(),
                field("version").unwrap(),
                field("source"),
                field("checksum"),
            )
        })
        .collect()
}

#[test]
fn standalone_glimpse_lock_is_a_subset_of_the_workspace_lock() {
    let standalone = lock_packages(include_str!(
        "../../../config/image/glimpse-shim.Cargo.lock"
    ));
    let workspace = lock_packages(include_str!("../../../Cargo.lock"));
    let foreign: Vec<_> = standalone.difference(&workspace).collect();
    assert!(
        foreign.is_empty(),
        "config/image/glimpse-shim.Cargo.lock drifted from Cargo.lock; regenerate it with scripts/update-glimpse-lock.sh: {foreign:?}"
    );
    assert!(
        standalone
            .iter()
            .any(|(name, _, _, _)| name == "glimpse-shim")
    );
}

#[test]
fn standalone_glimpse_lock_resolves_under_locked() {
    let (_dir, root) = snapshot();
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let output = Command::new(cargo)
        .args([
            "tree",
            "--locked",
            "--offline",
            "-e",
            "normal,build,dev",
            "--prefix",
            "none",
        ])
        .current_dir(root.join("glimpse-shim"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("clap"));
}
