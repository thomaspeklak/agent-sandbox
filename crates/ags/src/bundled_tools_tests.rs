use super::{BuildArchitecture, br_required_assets, dcg_required_assets};

#[test]
fn recognizes_podman_architecture_names() {
    assert!(matches!(
        BuildArchitecture::from_podman_arch("amd64").unwrap(),
        BuildArchitecture::X86_64
    ));
    assert!(matches!(
        BuildArchitecture::from_podman_arch("arm64").unwrap(),
        BuildArchitecture::Aarch64
    ));
    assert!(BuildArchitecture::from_podman_arch("riscv64").is_err());
}

#[test]
fn defines_release_assets_for_each_bundled_tool() {
    assert_eq!(
        br_required_assets(BuildArchitecture::X86_64),
        [
            "br-{version}-linux_amd64.tar.gz".to_owned(),
            "br-{version}-linux_amd64.tar.gz.sha256".to_owned(),
        ]
    );
    assert_eq!(
        dcg_required_assets(BuildArchitecture::Aarch64),
        [
            "dcg-aarch64-unknown-linux-gnu.tar.xz".to_owned(),
            "dcg-aarch64-unknown-linux-gnu.tar.xz.sha256".to_owned(),
        ]
    );
}
