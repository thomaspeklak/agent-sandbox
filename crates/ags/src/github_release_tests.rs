use time::{OffsetDateTime, format_description::well_known::Rfc3339};

use super::{
    GitHubReleaseError, ReleaseSelection, parse_release_page, resolve_latest_mature_release_with,
};

const REPO: &str = "example/releases";
const NOW: &str = "2026-08-27T13:38:59Z";

fn release(
    tag: &str,
    published_at: Option<&str>,
    draft: bool,
    prerelease: bool,
    assets: &[&str],
) -> serde_json::Value {
    serde_json::json!({
        "tag_name": tag,
        "published_at": published_at,
        "draft": draft,
        "prerelease": prerelease,
        "assets": assets
            .iter()
            .map(|name| serde_json::json!({
                "name": name,
                "state": "uploaded",
                "size": 1,
            }))
            .collect::<Vec<_>>(),
    })
}

fn resolve(
    minimum_release_age: u32,
    required_assets: &[&str],
    pages: Vec<serde_json::Value>,
) -> Result<ReleaseSelection, GitHubReleaseError> {
    let now = OffsetDateTime::parse(NOW, &Rfc3339).unwrap();
    let required_assets = required_assets
        .iter()
        .map(|asset| (*asset).to_owned())
        .collect::<Vec<_>>();
    let pages = pages
        .into_iter()
        .map(|page| page.to_string())
        .collect::<Vec<_>>();
    let mut pages = pages.iter();

    resolve_latest_mature_release_with(REPO, minimum_release_age, &required_assets, now, |_| {
        let body = pages.next().map_or("[]", String::as_str);
        parse_release_page(REPO, body.as_bytes())
    })
}

#[test]
fn selects_the_newest_mature_strict_version_tag() {
    let release = resolve(
        1_440,
        &[],
        vec![serde_json::json!([
            release("v1.2.3", Some("2026-08-27T13:00:00Z"), false, false, &[]),
            release("v1.2.2", Some("2026-08-25T13:00:00Z"), false, false, &[]),
        ])],
    )
    .unwrap();

    assert_eq!(release.tag_name, "v1.2.2");
    assert_eq!(release.latest_tag_name, "v1.2.3");
}

#[test]
fn rejects_legacy_and_non_release_tags() {
    let release = resolve(
        1_440,
        &[],
        vec![serde_json::json!([
            release("0.0.47", Some("2026-08-20T13:00:00Z"), false, false, &[]),
            release(
                "pr-38252-videos",
                Some("2026-08-20T13:00:00Z"),
                false,
                false,
                &[]
            ),
            release(
                "v1.2.3-rc.1",
                Some("2026-08-20T13:00:00Z"),
                false,
                false,
                &[]
            ),
            release("v01.2.3", Some("2026-08-20T13:00:00Z"), false, false, &[]),
            release("v1.02.3", Some("2026-08-20T13:00:00Z"), false, false, &[]),
            release("v1.2.03", Some("2026-08-20T13:00:00Z"), false, false, &[]),
            release("v1.2.2", Some("2026-08-20T13:00:00Z"), false, false, &[]),
        ])],
    )
    .unwrap();

    assert_eq!(release.tag_name, "v1.2.2");
    assert_eq!(release.latest_tag_name, "v1.2.2");
}

#[test]
fn rejects_draft_prerelease_and_missing_publication_time() {
    let release = resolve(
        1_440,
        &[],
        vec![serde_json::json!([
            release("v1.2.5", Some("2026-08-20T13:00:00Z"), true, false, &[]),
            release("v1.2.4", Some("2026-08-20T13:00:00Z"), false, true, &[]),
            release("v1.2.3", None, false, false, &[]),
            release("v1.2.2", Some("2026-08-20T13:00:00Z"), false, false, &[]),
        ])],
    )
    .unwrap();

    assert_eq!(release.tag_name, "v1.2.2");
}

#[test]
fn requires_uploaded_nonempty_assets_after_age_filtering() {
    let archive = "tool-x86_64.tar.gz";
    let checksum = "tool-x86_64.tar.gz.sha256";
    let release = resolve(
        1_440,
        &[archive, checksum],
        vec![serde_json::json!([
            release(
                "v1.2.4",
                Some("2026-08-20T13:00:00Z"),
                false,
                false,
                &[archive]
            ),
            release(
                "v1.2.3",
                Some("2026-08-20T13:00:00Z"),
                false,
                false,
                &[archive, checksum]
            ),
        ])],
    )
    .unwrap();

    assert_eq!(release.tag_name, "v1.2.3");
    assert_eq!(release.latest_tag_name, "v1.2.4");
}

#[test]
fn expands_version_in_asset_requirements() {
    let release = resolve(
        1_440,
        &["br-{version}-linux_amd64.tar.gz"],
        vec![serde_json::json!([release(
            "v1.2.3",
            Some("2026-08-20T13:00:00Z"),
            false,
            false,
            &["br-1.2.3-linux_amd64.tar.gz"],
        )])],
    )
    .unwrap();

    assert_eq!(release.tag_name, "v1.2.3");
}

#[test]
fn continues_to_the_next_page_when_the_first_page_is_ineligible() {
    let first_page = (0..100)
        .map(|patch| {
            release(
                &format!("v9.9.{patch}"),
                Some("2026-08-27T13:00:00Z"),
                false,
                false,
                &[],
            )
        })
        .collect::<Vec<_>>();
    let release = resolve(
        1_440,
        &[],
        vec![
            serde_json::Value::Array(first_page),
            serde_json::json!([release(
                "v1.2.3",
                Some("2026-08-20T13:00:00Z"),
                false,
                false,
                &[],
            )]),
        ],
    )
    .unwrap();

    assert_eq!(release.tag_name, "v1.2.3");
    assert_eq!(release.latest_tag_name, "v9.9.0");
}

#[test]
fn reports_when_no_release_meets_the_policy() {
    let error = resolve(
        1_440,
        &["tool.tar.gz"],
        vec![serde_json::json!([release(
            "v1.2.3",
            Some("2026-08-27T13:00:00Z"),
            false,
            false,
            &["tool.tar.gz"],
        )])],
    )
    .unwrap_err();

    assert!(
        error
            .to_string()
            .contains("no stable vMAJOR.MINOR.PATCH release published at least 1440 minutes ago")
    );
}
