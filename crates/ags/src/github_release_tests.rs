use std::collections::BTreeMap;

use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use crate::config::{
    ArchiveMemberMatch, GitHubReleaseAssetSelector, GitHubReleaseAssetSelectors,
    GitHubReleaseSelection, GitHubReleaseSource, ToolArchiveFormat, ToolDownloadSource,
};

use super::{FetchRequest, GitHubReleaseError, resolve_github_release_source_with};

const REPO: &str = "example/releases";
const NOW: &str = "2026-08-27T13:38:59Z";

fn catalog_asset(name: &str, url: &str, digest: Option<&str>) -> serde_json::Value {
    serde_json::json!({
        "name": name,
        "state": "uploaded",
        "size": 123,
        "browser_download_url": url,
        "digest": digest,
    })
}

fn catalog_release(
    tag: &str,
    published_at: &str,
    assets: Vec<serde_json::Value>,
) -> serde_json::Value {
    serde_json::json!({
        "tag_name": tag,
        "published_at": published_at,
        "draft": false,
        "prerelease": false,
        "assets": assets,
    })
}

fn catalog_source(release: GitHubReleaseSelection) -> GitHubReleaseSource {
    GitHubReleaseSource {
        repository: REPO.to_owned(),
        release,
        archive: ToolArchiveFormat::TarXz,
        member: "tool".to_owned(),
        member_match: ArchiveMemberMatch::UniqueBasename,
        install_as: "tool".to_owned(),
        assets: GitHubReleaseAssetSelectors {
            x86_64: GitHubReleaseAssetSelector {
                archive: r"^tool-{version}-x86_64\.tar\.xz$".to_owned(),
                checksum: None,
            },
            aarch64: GitHubReleaseAssetSelector {
                archive: r"^tool-{version}-aarch64\.tar\.xz$".to_owned(),
                checksum: None,
            },
        },
    }
}

fn digest(byte: char) -> String {
    format!("sha256:{}", byte.to_string().repeat(64))
}

fn resolve_catalog(
    source: &GitHubReleaseSource,
    pages: Vec<serde_json::Value>,
    tagged_release: Option<serde_json::Value>,
    checksums: BTreeMap<String, String>,
) -> Result<ToolDownloadSource, GitHubReleaseError> {
    let now = OffsetDateTime::parse(NOW, &Rfc3339).unwrap();
    resolve_github_release_source_with(source, 1_440, now, |request| match request {
        FetchRequest::ReleasesPage(page) => Ok(pages
            .get(page - 1)
            .cloned()
            .unwrap_or_else(|| serde_json::json!([]))
            .to_string()
            .into_bytes()),
        FetchRequest::ReleaseByTag(_) => Ok(tagged_release
            .clone()
            .expect("tagged release fixture")
            .to_string()
            .into_bytes()),
        FetchRequest::Asset(url) => Ok(checksums
            .get(&url)
            .unwrap_or_else(|| panic!("missing checksum fixture for {url}"))
            .as_bytes()
            .to_vec()),
    })
}

#[test]
fn latest_picks_the_highest_mature_version_regardless_of_api_order() {
    let source = catalog_source(GitHubReleaseSelection::Latest);
    let compatible_assets = |version: &str| {
        vec![
            catalog_asset(
                &format!("tool-{version}-x86_64.tar.xz"),
                &format!("https://objects.example/{version}/x"),
                Some(&digest('a')),
            ),
            catalog_asset(
                &format!("tool-{version}-aarch64.tar.xz"),
                &format!("https://objects.example/{version}/a"),
                Some(&digest('b')),
            ),
        ]
    };
    // GitHub lists by creation time: a backport patch is first, the newest
    // major is immature, and the highest mature version lacks an asset.
    let page = serde_json::json!([
        catalog_release(
            "v1.9.10",
            "2026-08-25T13:30:00Z",
            compatible_assets("1.9.10")
        ),
        catalog_release("v3.0.0", "2026-08-27T13:00:00Z", compatible_assets("3.0.0")),
        catalog_release(
            "v2.1.0",
            "2026-08-20T13:00:00Z",
            vec![catalog_asset(
                "tool-2.1.0-x86_64.tar.xz",
                "https://objects.example/2/x",
                Some(&digest('c')),
            )],
        ),
        catalog_release("v2.0.0", "2026-08-20T13:00:00Z", compatible_assets("2.0.0")),
        catalog_release(
            "v2.0.0-rc1",
            "2026-08-01T13:00:00Z",
            compatible_assets("2.0.0")
        ),
    ]);

    let resolved = resolve_catalog(&source, vec![page], None, BTreeMap::new()).unwrap();

    assert_eq!(resolved.version, "2.0.0");
    assert_eq!(resolved.archive, ToolArchiveFormat::TarXz);
    assert_eq!(resolved.member_match, ArchiveMemberMatch::UniqueBasename);
}

#[test]
fn latest_orders_versions_numerically_not_lexically() {
    let source = catalog_source(GitHubReleaseSelection::Latest);
    let assets = |version: &str| {
        vec![
            catalog_asset(
                &format!("tool-{version}-x86_64.tar.xz"),
                &format!("https://objects.example/{version}/x"),
                Some(&digest('a')),
            ),
            catalog_asset(
                &format!("tool-{version}-aarch64.tar.xz"),
                &format!("https://objects.example/{version}/a"),
                Some(&digest('b')),
            ),
        ]
    };
    let page = serde_json::json!([
        catalog_release("v1.9.0", "2026-08-20T13:00:00Z", assets("1.9.0")),
        catalog_release("v1.10.0", "2026-08-19T13:00:00Z", assets("1.10.0")),
    ]);

    let resolved = resolve_catalog(&source, vec![page], None, BTreeMap::new()).unwrap();

    assert_eq!(resolved.version, "1.10.0");
}

#[test]
fn exact_version_uses_only_the_requested_tag_without_age_or_fallback() {
    let source = catalog_source(GitHubReleaseSelection::Version {
        version: "4.5.6".to_owned(),
        tag_template: "release-{version}".to_owned(),
    });
    let tagged = catalog_release(
        "release-4.5.6",
        "2026-08-27T13:38:58Z",
        vec![catalog_asset(
            "tool-4.5.6-x86_64.tar.xz",
            "https://objects.example/pinned/x",
            Some(&digest('a')),
        )],
    );
    let now = OffsetDateTime::parse(NOW, &Rfc3339).unwrap();
    let mut requests = Vec::new();

    let error = resolve_github_release_source_with(&source, 1_440, now, |request| {
        requests.push(request.clone());
        match request {
            FetchRequest::ReleaseByTag(tag) => {
                assert_eq!(tag, "release-4.5.6");
                Ok(tagged.to_string().into_bytes())
            }
            _ => panic!("version resolution must not list or fall back to other releases"),
        }
    })
    .unwrap_err();

    assert!(matches!(
        error,
        GitHubReleaseError::IncompatibleVersion { .. }
    ));
    assert_eq!(
        requests,
        vec![FetchRequest::ReleaseByTag("release-4.5.6".to_owned())]
    );
}

#[test]
fn substitutes_regex_escaped_version_and_tag_and_uses_api_urls_verbatim() {
    let mut source = catalog_source(GitHubReleaseSelection::Version {
        version: "1.2+3".to_owned(),
        tag_template: "rel-{version}".to_owned(),
    });
    source.assets.x86_64.archive = r"^tool-{version}-{tag}-x86_64\.tar\.xz$".to_owned();
    source.assets.aarch64.archive = r"^tool-{version}-{tag}-aarch64\.tar\.xz$".to_owned();
    let x_url = "https://opaque.example/download?id=x86&token=kept";
    let arm_url = "https://opaque.example/not-derived/aarch64";
    let tagged = catalog_release(
        "rel-1.2+3",
        "2026-08-27T13:38:58Z",
        vec![
            catalog_asset(
                "tool-1.2+3-rel-1.2+3-x86_64.tar.xz",
                x_url,
                Some(&digest('A')),
            ),
            catalog_asset(
                "tool-1.2+3-rel-1.2+3-aarch64.tar.xz",
                arm_url,
                Some(&digest('B')),
            ),
        ],
    );

    let resolved = resolve_catalog(&source, vec![], Some(tagged), BTreeMap::new()).unwrap();

    assert_eq!(resolved.artifacts["x86_64"].url, x_url);
    assert_eq!(resolved.artifacts["aarch64"].url, arm_url);
    assert_eq!(resolved.artifacts["x86_64"].sha256, "a".repeat(64));
    assert_eq!(resolved.artifacts["aarch64"].sha256, "b".repeat(64));
}

#[test]
fn prefers_digest_and_strictly_falls_back_to_the_matching_checksum_entry() {
    let mut source = catalog_source(GitHubReleaseSelection::Version {
        version: "7.8.9".to_owned(),
        tag_template: "v{version}".to_owned(),
    });
    source.assets.x86_64.checksum = Some(r"^tool-7\.8\.9-x86_64\.sha256$".to_owned());
    source.assets.aarch64.checksum = Some(r"^tool-7\.8\.9-aarch64\.sha256$".to_owned());
    let x_checksum_url = "https://objects.example/checksums/x";
    let arm_checksum_url = "https://objects.example/checksums/arm";
    let tagged = catalog_release(
        "v7.8.9",
        "2026-08-20T13:00:00Z",
        vec![
            catalog_asset(
                "tool-7.8.9-x86_64.tar.xz",
                "https://objects.example/archive/x",
                Some(&digest('c')),
            ),
            catalog_asset(
                "tool-7.8.9-aarch64.tar.xz",
                "https://objects.example/archive/arm",
                Some("sha256:not-a-hash"),
            ),
            catalog_asset("tool-7.8.9-x86_64.sha256", x_checksum_url, None),
            catalog_asset("tool-7.8.9-aarch64.sha256", arm_checksum_url, None),
        ],
    );
    let now = OffsetDateTime::parse(NOW, &Rfc3339).unwrap();
    let arm_hash = "d".repeat(64);
    let mut fetched_assets = Vec::new();

    let resolved =
        resolve_github_release_source_with(&source, 1_440, now, |request| match request {
            FetchRequest::ReleaseByTag(_) => Ok(tagged.to_string().into_bytes()),
            FetchRequest::Asset(url) => {
                fetched_assets.push(url.clone());
                assert_eq!(url, arm_checksum_url);
                Ok(format!("{arm_hash}  tool-7.8.9-aarch64.tar.xz\n").into_bytes())
            }
            FetchRequest::ReleasesPage(_) => panic!("unexpected release listing"),
        })
        .unwrap();

    assert_eq!(resolved.artifacts["x86_64"].sha256, "c".repeat(64));
    assert_eq!(resolved.artifacts["aarch64"].sha256, arm_hash);
    assert_eq!(fetched_assets, vec![arm_checksum_url]);
}

#[test]
fn rejects_missing_and_ambiguous_architecture_assets() {
    let source = catalog_source(GitHubReleaseSelection::Version {
        version: "1.0.0".to_owned(),
        tag_template: "v{version}".to_owned(),
    });
    let missing = catalog_release(
        "v1.0.0",
        "2026-08-20T13:00:00Z",
        vec![catalog_asset(
            "tool-1.0.0-x86_64.tar.xz",
            "https://objects.example/x",
            Some(&digest('a')),
        )],
    );
    let error = resolve_catalog(&source, vec![], Some(missing), BTreeMap::new()).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("aarch64 archive regex matched no assets")
    );

    let mut ambiguous_source = source;
    ambiguous_source.assets.x86_64.archive = r"^tool-1\.0\.0-x86_64(?:-alt)?\.tar\.xz$".to_owned();
    let ambiguous = catalog_release(
        "v1.0.0",
        "2026-08-20T13:00:00Z",
        vec![
            catalog_asset(
                "tool-1.0.0-x86_64.tar.xz",
                "https://objects.example/x",
                Some(&digest('a')),
            ),
            catalog_asset(
                "tool-1.0.0-x86_64-alt.tar.xz",
                "https://objects.example/x-alt",
                Some(&digest('b')),
            ),
            catalog_asset(
                "tool-1.0.0-aarch64.tar.xz",
                "https://objects.example/a",
                Some(&digest('c')),
            ),
        ],
    );
    let error =
        resolve_catalog(&ambiguous_source, vec![], Some(ambiguous), BTreeMap::new()).unwrap_err();
    assert!(
        error
            .to_string()
            .contains("x86_64 archive regex matched 2 assets")
    );
}

#[test]
fn validates_tar_xz_member_modes_and_serde_defaults() {
    let old_lock = serde_json::json!({
        "version": "1.0.0",
        "archive": "tar.xz",
        "member": "bin/tool",
        "install_as": "tool",
        "artifacts": {
            "x86_64": {"url": "https://example.com/x", "sha256": "a".repeat(64)},
            "aarch64": {"url": "https://example.com/a", "sha256": "b".repeat(64)}
        }
    });
    let lock: ToolDownloadSource = serde_json::from_value(old_lock).unwrap();
    assert_eq!(lock.archive, ToolArchiveFormat::TarXz);
    assert_eq!(lock.member_match, ArchiveMemberMatch::Exact);
    assert!(
        serde_json::to_value(&lock)
            .unwrap()
            .get("member_match")
            .is_none()
    );

    let mut source = catalog_source(GitHubReleaseSelection::Latest);
    source.member = "bin/tool".to_owned();
    let now = OffsetDateTime::parse(NOW, &Rfc3339).unwrap();
    let error = resolve_github_release_source_with(&source, 0, now, |_| {
        panic!("invalid source must be rejected before fetching")
    })
    .unwrap_err();
    assert!(error.to_string().contains("must be a basename"));

    let json = serde_json::json!({
        "repository": REPO,
        "release": {"mode": "version", "version": "2.3.4"},
        "archive": "tar.xz",
        "member": "tool",
        "install_as": "tool",
        "assets": {
            "x86_64": {"archive": "^x$"},
            "aarch64": {"archive": "^a$"}
        }
    });
    let source: GitHubReleaseSource = serde_json::from_value(json).unwrap();
    assert_eq!(source.member_match, ArchiveMemberMatch::Exact);
    assert!(matches!(
        source.release,
        GitHubReleaseSelection::Version { tag_template, .. } if tag_template == "v{version}"
    ));
}

#[test]
fn rejects_absent_ambiguous_and_malformed_checksum_entries() {
    let hash = "e".repeat(64);
    assert!(super::extract_checksum(b"", "tool.tar.xz").is_err());
    assert!(super::extract_checksum(b"not a checksum\n", "tool.tar.xz").is_err());
    assert!(
        super::extract_checksum(
            format!("{hash}  tool.tar.xz\n{hash}  tool.tar.xz\n").as_bytes(),
            "tool.tar.xz"
        )
        .is_err()
    );
}
