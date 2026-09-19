use super::*;

#[test]
fn normalizes_podman_architecture_names() {
    for (arch, expected, triple, vendor) in [
        ("amd64", "amd64", "x86_64-unknown-linux-gnu", "x86_64"),
        ("x86_64", "amd64", "x86_64-unknown-linux-gnu", "x86_64"),
        ("arm64", "arm64", "aarch64-unknown-linux-gnu", "aarch64"),
        ("aarch64", "arm64", "aarch64-unknown-linux-gnu", "aarch64"),
    ] {
        let platform = Platform::from_podman("linux", arch).unwrap();
        assert_eq!(platform.arch, expected);
        assert_eq!(platform.rust_triple(), triple);
        assert_eq!(platform.vendor_arch(), vendor);
        assert_eq!(platform.to_string(), format!("linux/{expected}"));
    }
}

#[test]
fn rejects_unsupported_platforms() {
    assert!(Platform::from_podman("windows", "amd64").is_err());
    assert!(Platform::from_podman("linux", "s390x").is_err());
    assert!(Platform::from_podman("linux", "arm").is_err());
}

#[test]
fn image_platform_must_match_the_host() {
    let host = Platform::from_podman("linux", "amd64").unwrap();
    assert!(host.matches_image("linux", "amd64"));
    assert!(host.matches_image("linux", "x86_64"));
    assert!(!host.matches_image("linux", "arm64"));
    assert!(!host.matches_image("", ""));
}
