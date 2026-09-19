use super::*;
use crate::config::{ArchiveMemberMatch, ToolArchiveFormat};

fn amd64() -> Platform {
    Platform::from_podman("linux", "amd64").unwrap()
}

fn arm64() -> Platform {
    Platform::from_podman("linux", "arm64").unwrap()
}

fn rust(rustc: &str) -> RustRelease {
    RustRelease {
        rustc: format!("{rustc} (abcdef012 2026-01-01)"),
        cargo: "0.93.0 (abcdef012 2026-01-01)".to_owned(),
        rustfmt: "1.8.0-stable (abcdef012 2026-01-01)".to_owned(),
        clippy: "0.1.92 (abcdef012 2026-01-01)".to_owned(),
    }
}

fn pnpm(version: &str) -> PnpmRelease {
    PnpmRelease {
        version: version.to_owned(),
        integrity: format!("sha512-{version}"),
        sha512: "0".repeat(128),
    }
}

fn vendor<'a>(sha: &str, install_as: &'a str) -> VendorInputs<'a> {
    VendorInputs {
        arch: "x86_64",
        sha256: sha.repeat(64),
        archive: ToolArchiveFormat::TarGz,
        member: "tool",
        member_match: ArchiveMemberMatch::Exact,
        install_as,
    }
}

fn final_inputs() -> FinalInputs {
    FinalInputs {
        os_checkpoint: "a".repeat(64),
        rust: "b".repeat(64),
        pnpm: "c".repeat(64),
        glimpse: "d".repeat(64),
        vendor: vec![("br".to_owned(), "e".repeat(64))],
    }
}

fn strings(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_owned()).collect()
}

#[test]
fn keys_are_stable_and_prefixed_by_kind() {
    let key = rust_key(&amd64(), &rust("1.92.0"), "1.28.2");
    assert_eq!(key, rust_key(&amd64(), &rust("1.92.0"), "1.28.2"));
    assert!(key.starts_with("rust-"));
    assert_eq!(key.len(), "rust-".len() + 64);
}

#[test]
fn every_key_includes_the_platform() {
    assert_ne!(
        rust_key(&amd64(), &rust("1.92.0"), "1.28.2"),
        rust_key(&arm64(), &rust("1.92.0"), "1.28.2")
    );
    assert_ne!(
        pnpm_key(&amd64(), &pnpm("10.0.0")),
        pnpm_key(&arm64(), &pnpm("10.0.0"))
    );
    assert_ne!(
        foundation_key(&amd64(), "base", 0),
        foundation_key(&arm64(), "base", 0)
    );
    assert_ne!(
        vendor_key(&amd64(), &vendor("a", "br")),
        vendor_key(&arm64(), &vendor("a", "br"))
    );
}

#[test]
fn equivalent_package_selections_share_one_baseline() {
    let platform = amd64();
    let canonical = baseline_key(&platform, "base", &strings(&["git", "tmux"]));
    assert_eq!(
        canonical,
        baseline_key(&platform, "base", &strings(&["tmux", " git", "git", ""]))
    );
    assert_ne!(
        canonical,
        baseline_key(&platform, "base", &strings(&["git", "tmux", "jq"]))
    );
    assert_ne!(
        canonical,
        baseline_key(&platform, "other-base", &strings(&["git", "tmux"]))
    );
    assert_eq!(
        canonical_packages(&strings(&["b", "a", "b", " "])),
        strings(&["a", "b"])
    );
}

#[test]
fn pnpm_change_only_invalidates_pnpm() {
    let platform = amd64();
    let foundation = foundation_key(&platform, "base", 0);
    assert_ne!(
        pnpm_key(&platform, &pnpm("10.0.0")),
        pnpm_key(&platform, &pnpm("10.0.1"))
    );
    // Rust and Glimpse keys have no pnpm input at all.
    assert_eq!(
        glimpse_key(&platform, &rust("1.92.0"), &foundation),
        glimpse_key(&platform, &rust("1.92.0"), &foundation)
    );
}

#[test]
fn rust_release_changes_rust_and_glimpse() {
    let platform = amd64();
    let foundation = foundation_key(&platform, "base", 0);
    assert_ne!(
        rust_key(&platform, &rust("1.92.0"), "1.28.2"),
        rust_key(&platform, &rust("1.93.0"), "1.28.2")
    );
    assert_ne!(
        glimpse_key(&platform, &rust("1.92.0"), &foundation),
        glimpse_key(&platform, &rust("1.93.0"), &foundation)
    );
}

#[test]
fn rustup_only_change_rebuilds_rust_but_not_glimpse() {
    let platform = amd64();
    assert_ne!(
        rust_key(&platform, &rust("1.92.0"), "1.28.2"),
        rust_key(&platform, &rust("1.92.0"), "1.29.0")
    );
    // Glimpse is keyed by the compiler identity, which rustup does not change.
    let foundation = foundation_key(&platform, "base", 0);
    let glimpse = glimpse_key(&platform, &rust("1.92.0"), &foundation);
    assert_eq!(
        glimpse,
        glimpse_key(&platform, &rust("1.92.0"), &foundation)
    );
}

#[test]
fn rebase_epoch_refreshes_foundation_and_glimpse() {
    let platform = amd64();
    let before = foundation_key(&platform, "base", 0);
    let after = foundation_key(&platform, "base", 1);
    assert_ne!(before, after);
    assert_ne!(
        glimpse_key(&platform, &rust("1.92.0"), &before),
        glimpse_key(&platform, &rust("1.92.0"), &after)
    );
}

#[test]
fn vendor_key_follows_payload_and_extraction_semantics() {
    let platform = amd64();
    let base = vendor_key(&platform, &vendor("a", "br"));
    assert_ne!(base, vendor_key(&platform, &vendor("b", "br")));
    assert_ne!(base, vendor_key(&platform, &vendor("a", "bv")));
    let mut other_member = vendor("a", "br");
    other_member.member = "bin/tool";
    assert_ne!(base, vendor_key(&platform, &other_member));
    let mut other_match = vendor("a", "br");
    other_match.member_match = ArchiveMemberMatch::UniqueBasename;
    assert_ne!(base, vendor_key(&platform, &other_match));
    let mut other_archive = vendor("a", "br");
    other_archive.archive = ToolArchiveFormat::Zip;
    assert_ne!(base, vendor_key(&platform, &other_archive));
}

#[test]
fn final_key_follows_component_ids_and_vendor_selection() {
    let base = final_key(&final_inputs());
    assert_eq!(base, final_key(&final_inputs()));
    for change in [
        |inputs: &mut FinalInputs| inputs.os_checkpoint = "f".repeat(64),
        |inputs: &mut FinalInputs| inputs.rust = "f".repeat(64),
        |inputs: &mut FinalInputs| inputs.pnpm = "f".repeat(64),
        |inputs: &mut FinalInputs| inputs.glimpse = "f".repeat(64),
        |inputs: &mut FinalInputs| inputs.vendor.clear(),
        |inputs: &mut FinalInputs| inputs.vendor[0].0 = "bv".to_owned(),
    ] {
        let mut inputs = final_inputs();
        change(&mut inputs);
        assert_ne!(base, final_key(&inputs));
    }
}
