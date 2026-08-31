mod tests {
    use std::path::Path;

    use super::{
        PreviousImageCleanup, build_podman_build_args, build_podman_container_image_refs_args,
        build_podman_image_exists_args, build_podman_image_inspect_args,
        build_podman_image_rm_args, is_image_reference_conflict, parse_container_image_refs,
        retain_referenced_image, short_image_id,
    };

    #[test]
    fn build_args_include_selected_components_and_pull_flag() {
        let args = build_podman_build_args(
            "localhost/agent-sandbox:latest",
            Path::new("/tmp/Containerfile"),
            Path::new("/tmp"),
            &["ansible-lint".to_owned(), "shellcheck".to_owned()],
            &[],
            true,
        );

        assert!(args.contains(&"--pull".to_owned()));
        assert!(!args.iter().any(|arg| arg.starts_with("BR_VERSION=")));
        assert!(!args.iter().any(|arg| arg.starts_with("BV_VERSION=")));
        assert!(!args.iter().any(|arg| arg.starts_with("DCG_VERSION=")));
        assert!(args.contains(&"EXTRA_DNF_PACKAGES=ansible-lint shellcheck".to_owned()));
        assert!(args.contains(&"EXTRA_TOOL_DOWNLOADS_B64=W10=".to_owned()));
        assert_eq!(args.last().unwrap(), "/tmp");
    }

    #[test]
    fn build_args_override_containerfile_default_for_empty_package_list() {
        let args = build_podman_build_args(
            "localhost/agent-sandbox:latest",
            Path::new("/tmp/Containerfile"),
            Path::new("/tmp"),
            &[],
            &[],
            false,
        );

        assert!(args.contains(&"EXTRA_DNF_PACKAGES=".to_owned()));
    }

    #[test]
    fn image_cleanup_args_target_previous_image_id() {
        assert_eq!(
            build_podman_image_exists_args("localhost/agent-sandbox:latest"),
            vec!["image", "exists", "localhost/agent-sandbox:latest"]
        );
        assert_eq!(
            build_podman_image_inspect_args("localhost/agent-sandbox:latest"),
            vec![
                "image",
                "inspect",
                "--format",
                "{{.Id}}",
                "localhost/agent-sandbox:latest"
            ]
        );
        assert_eq!(
            build_podman_image_rm_args("sha256:old"),
            vec!["image", "rm", "sha256:old"]
        );
        assert_eq!(
            build_podman_container_image_refs_args(),
            vec![
                "ps",
                "--all",
                "--external",
                "--no-trunc",
                "--format",
                "{{.ID}}\t{{.ImageID}}"
            ]
        );
    }

    #[test]
    fn cleanup_retains_previous_image_used_by_containers() {
        let cleanup = retain_referenced_image(
            "sha256:old",
            vec!["3d1419eaf9fd".to_owned(), "another".to_owned()],
        );

        assert_eq!(
            cleanup,
            Some(PreviousImageCleanup::Retained {
                image_id: "sha256:old".to_owned(),
                container_ids: vec!["3d1419eaf9fd".to_owned(), "another".to_owned()]
            })
        );
        assert_eq!(retain_referenced_image("sha256:old", Vec::new()), None);
    }

    #[test]
    fn cleanup_matches_only_exact_image_references() {
        let output = "direct\tsha256:abc123\nchild\tsha256:def456\nexternal\tabc123\nimageless\t\n";

        assert_eq!(
            parse_container_image_refs(output, "sha256:abc123").unwrap(),
            vec!["direct", "external"]
        );
    }

    #[test]
    fn cleanup_only_downgrades_podman_reference_conflicts() {
        assert!(is_image_reference_conflict(Some(2)));
        assert!(!is_image_reference_conflict(Some(125)));
        assert!(!is_image_reference_conflict(None));
    }

    #[test]
    fn short_image_id_strips_prefix_and_truncates() {
        assert_eq!(short_image_id("sha256:1234567890abcdef"), "1234567890ab");
        assert_eq!(short_image_id("abc"), "abc");
    }
}
