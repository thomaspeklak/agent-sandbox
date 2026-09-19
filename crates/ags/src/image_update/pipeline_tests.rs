use super::*;
use crate::image_update::state::tests::sample_state;

#[test]
fn image_names_are_fully_qualified_like_podman_local_builds() {
    for (input, expected) in [
        ("agent-sandbox", "localhost/agent-sandbox:latest"),
        ("agent-sandbox:dev", "localhost/agent-sandbox:dev"),
        ("team/sandbox", "localhost/team/sandbox:latest"),
        (
            "localhost/agent-sandbox:latest",
            "localhost/agent-sandbox:latest",
        ),
        ("localhost/agent-sandbox", "localhost/agent-sandbox:latest"),
        ("quay.io/team/sandbox", "quay.io/team/sandbox:latest"),
        ("registry:5000/sandbox:1", "registry:5000/sandbox:1"),
        ("registry:5000/sandbox", "registry:5000/sandbox:latest"),
    ] {
        assert_eq!(normalize_image_name(input), expected, "{input}");
    }
}

#[test]
fn verification_expects_selected_packages_versions_and_tools() {
    let state = sample_state();
    let expected = expectations(
        &state,
        &["tmux".to_owned(), "git".to_owned(), "git".to_owned()],
    );
    assert_eq!(expected.pnpm, "10.20.0");
    assert_eq!(expected.rustc, "1.92.0 (abcdef012 2026-01-01)");
    assert_eq!(expected.rustup, "1.28.2");
    assert_eq!(expected.commands, ["br"]);
    assert!(expected.rpms.starts_with(&["bash".to_owned()]));
    assert!(
        expected
            .rpms
            .ends_with(&["git".to_owned(), "tmux".to_owned()])
    );
    assert_eq!(
        expected.rpms.len(),
        crate::config::BASE_DNF_PACKAGES.len() + 2
    );
}
