use std::path::Path;

use ags::config::{PnpmVersion, parse_and_validate_with_overlay, parse_toml_str};

const CONFIG: &str = r#"
[sandbox]
image = "localhost/agent-sandbox:latest"
containerfile = "/tmp/Containerfile"
cache_dir = "/tmp/cache"
gitconfig_path = "/tmp/gitconfig"
auth_key = "/tmp/auth"
sign_key = "/tmp/sign"
"#;

#[test]
fn omitted_or_explicit_latest_preserves_automatic_updates() {
    for setting in ["", "pnpm_version = 'latest'"] {
        let config =
            parse_toml_str(&format!("{CONFIG}\n{setting}"), Path::new("/config.toml")).unwrap();
        assert_eq!(config.sandbox.pnpm_version, PnpmVersion::default());
    }
}

#[test]
fn config_editor_exposes_the_optional_pnpm_selection() {
    let field = ags::cmd::config_editor::schema::scalar_field("sandbox", "pnpm_version").unwrap();
    assert!(!field.required);
    assert_eq!(field.default_input, PnpmVersion::default().as_str());
    assert_eq!(
        field.kind,
        ags::cmd::config_editor::schema::ScalarFieldKind::Text
    );
}

#[test]
fn accepts_exact_stable_versions() {
    for version in ["12.9.1", "11.27.1", "0.0.0"] {
        let config = parse_toml_str(
            &format!("{CONFIG}\npnpm_version = '{version}'"),
            Path::new("/config.toml"),
        )
        .unwrap();
        assert_eq!(config.sandbox.pnpm_version.as_str(), version);
    }
}

#[test]
fn rejects_ranges_tags_prereleases_and_noncanonical_versions() {
    for version in [
        "",
        " ",
        " latest",
        "12.9.1 ",
        "^12.9.1",
        "~12.9.1",
        "12",
        "12.9",
        "12.x",
        "next",
        "v12.9.1",
        "12.9.1-beta.1",
        "12.9.1+build",
        "01.2.3",
        "1.02.3",
        "1.2.03",
        "1.2.3/other",
        "18446744073709551616.0.0",
    ] {
        let error = parse_toml_str(
            &format!("{CONFIG}\npnpm_version = '{version}'"),
            Path::new("/config.toml"),
        )
        .unwrap_err()
        .to_string();
        assert!(
            error.contains("[sandbox].pnpm_version"),
            "{version}: {error}"
        );
    }
}

#[test]
fn repository_overlay_can_pin_or_restore_latest() {
    let root = tempfile::tempdir().unwrap();
    let base = root.path().join("base.toml");
    let overlay = root.path().join("overlay.toml");
    std::fs::write(&base, format!("{CONFIG}\npnpm_version = '11.27.1'")).unwrap();
    for version in ["12.9.1", "latest"] {
        std::fs::write(&overlay, format!("[sandbox]\npnpm_version = '{version}'")).unwrap();
        let config = parse_and_validate_with_overlay(&base, Some(&overlay)).unwrap();
        assert_eq!(config.sandbox.pnpm_version.as_str(), version);
    }
    std::fs::write(&overlay, "[sandbox]\npnpm_version = '^12'").unwrap();
    assert!(parse_and_validate_with_overlay(&base, Some(&overlay)).is_err());
}
