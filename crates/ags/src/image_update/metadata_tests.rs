use super::*;

const TRIPLE: &str = "x86_64-unknown-linux-gnu";

fn channel(rustc: &str, available: bool) -> String {
    let target = |package: &str, version: &str| {
        format!(
            "[pkg.{package}]\nversion = \"{version}\"\n\n[pkg.{package}.target.{TRIPLE}]\navailable = {available}\nurl = \"https://static.rust-lang.org/dist/x.tar.gz\"\nhash = \"00\"\n\n"
        )
    };
    format!(
        "manifest-version = \"2\"\ndate = \"2026-01-01\"\n\n{}{}{}{}{}",
        target("rustc", &format!("{rustc} (abcdef012 2026-01-01)")),
        target("cargo", "0.93.0 (abcdef012 2026-01-01)"),
        target("rust-std", &format!("{rustc} (abcdef012 2026-01-01)")),
        target("rustfmt-preview", "1.8.0-stable (abcdef012 2026-01-01)"),
        target("clippy-preview", "0.1.92 (abcdef012 2026-01-01)"),
    )
}

#[test]
fn parses_the_stable_channel_for_the_host_triple() {
    let release = parse_rust_channel(&channel("1.92.0", true), TRIPLE).unwrap();
    assert_eq!(release.rustc, "1.92.0 (abcdef012 2026-01-01)");
    assert_eq!(release.cargo, "0.93.0 (abcdef012 2026-01-01)");
    assert_eq!(release.rustfmt, "1.8.0-stable (abcdef012 2026-01-01)");
    assert_eq!(release.clippy, "0.1.92 (abcdef012 2026-01-01)");
}

#[test]
fn rejects_unavailable_or_unexpected_rust_metadata() {
    assert!(
        parse_rust_channel(&channel("1.92.0", false), TRIPLE)
            .unwrap_err()
            .contains("not available")
    );
    assert!(parse_rust_channel(&channel("1.92.0", true), "aarch64-unknown-linux-gnu").is_err());
    assert!(parse_rust_channel(&channel("1.93.0-beta.1", true), TRIPLE).is_err());
    assert!(parse_rust_channel(&channel("1.92.0-stable", true), TRIPLE).is_err());
    assert!(parse_rust_channel("not toml [", TRIPLE).is_err());
    let unsupported =
        channel("1.92.0", true).replace("manifest-version = \"2\"", "manifest-version = \"3\"");
    assert!(parse_rust_channel(&unsupported, TRIPLE).is_err());
    let missing_clippy = channel("1.92.0", true).replace("clippy-preview", "other");
    assert!(parse_rust_channel(&missing_clippy, TRIPLE).is_err());
}

#[test]
fn parses_the_rustup_release_manifest() {
    assert_eq!(
        parse_rustup_release("schema-version = '1'\nversion = '1.28.2'\n").unwrap(),
        "1.28.2"
    );
    assert!(parse_rustup_release("schema-version = '2'\nversion = '1.28.2'\n").is_err());
    assert!(parse_rustup_release("schema-version = '1'\nversion = '1.28'\n").is_err());
    assert!(parse_rustup_release("schema-version = '1'\n").is_err());
}

fn pnpm_latest(version: &str, tarball: &str, integrity: &str) -> String {
    serde_json::json!({
        "name": "pnpm",
        "version": version,
        "dist": {"integrity": integrity, "tarball": tarball, "shasum": "ignored"},
        "other": "fields are ignored"
    })
    .to_string()
}

fn integrity() -> String {
    format!(
        "sha512-{}",
        base64::engine::general_purpose::STANDARD.encode([0xab_u8; 64])
    )
}

#[test]
fn parses_pnpm_latest_with_hex_digest() {
    let body = pnpm_latest(
        "10.20.0",
        "https://registry.npmjs.org/pnpm/-/pnpm-10.20.0.tgz",
        &integrity(),
    );
    let release = parse_pnpm_latest(&body).unwrap();
    assert_eq!(release.version, "10.20.0");
    assert_eq!(release.integrity, integrity());
    assert_eq!(release.sha512, "ab".repeat(64));
}

#[test]
fn rejects_prerelease_foreign_or_unverifiable_pnpm_metadata() {
    let url = |version: &str| format!("https://registry.npmjs.org/pnpm/-/pnpm-{version}.tgz");
    for body in [
        pnpm_latest("11.0.0-rc.1", &url("11.0.0-rc.1"), &integrity()),
        pnpm_latest(
            "10.20.0",
            "https://example.com/pnpm-10.20.0.tgz",
            &integrity(),
        ),
        pnpm_latest("10.20.0", &url("10.20.0"), "sha1-AAAA"),
        pnpm_latest("10.20.0", &url("10.20.0"), "sha512-not base64"),
        pnpm_latest("10.20.0", &url("10.20.0"), "sha512-AAAA"),
        pnpm_latest("10.20.0", &url("10.20.0"), &integrity()).replace("\"pnpm\"", "\"npm\""),
        "{".to_owned(),
    ] {
        assert!(parse_pnpm_latest(&body).is_err(), "accepted {body}");
    }
}
