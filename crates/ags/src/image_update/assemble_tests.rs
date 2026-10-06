use super::*;

fn vendor(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(command, id)| ((*command).to_owned(), (*id).to_owned()))
        .collect()
}

#[test]
fn renders_one_stage_and_copy_per_selected_tool() {
    let rendered = render_final(
        crate::assets::CONTAINERFILE,
        &vendor(&[("br", "1111"), ("dcg", "2222")]),
    )
    .unwrap();
    assert!(rendered.contains("FROM 1111 AS vendor-0\nFROM 2222 AS vendor-1\n"));
    assert!(rendered.contains("COPY --from=vendor-0 /out/br /usr/local/bin/br\n"));
    assert!(rendered.contains("COPY --from=vendor-1 /out/dcg /usr/local/bin/dcg\n"));
    assert!(!rendered.contains("@ags-vendor"));
    // Vendor stages precede the final stage, so they are not the output image.
    let stages = rendered.find("FROM 2222 AS vendor-1").unwrap();
    let output_stage = rendered.find("FROM ${OS_IMAGE}").unwrap();
    assert!(stages < output_stage);
}

#[test]
fn removed_tools_are_absent_from_the_rendered_recipe() {
    let rendered = render_final(crate::assets::CONTAINERFILE, &vendor(&[("br", "1111")])).unwrap();
    assert!(!rendered.contains("dcg"));
    let none = render_final(crate::assets::CONTAINERFILE, &[]).unwrap();
    assert!(!none.contains("COPY --from=vendor-"));
    assert!(
        !none
            .lines()
            .any(|line| line.starts_with("FROM ") && line.contains(" AS vendor-"))
    );
    assert!(none.contains("COPY --from=glimpse /out/glimpse-shim /opt/ags/glimpse-shim"));
}

#[test]
fn rejects_templates_without_exactly_one_marker() {
    assert!(render_final("FROM x\n", &[]).is_err());
    let doubled = format!("{}# @ags-vendor-copies@\n", crate::assets::CONTAINERFILE);
    assert!(render_final(&doubled, &[]).is_err());
}

#[test]
fn verification_is_offline_mount_free_and_pull_free() {
    let expected = Expectations {
        pnpm: "10.20.0".to_owned(),
        rustc: "1.92.0 (abcdef012 2026-01-01)".to_owned(),
        rustup: "1.28.2".to_owned(),
        rpms: vec!["bash".to_owned(), "git".to_owned()],
        commands: vec!["br".to_owned()],
    };
    let args = verify_args("candidate-id", &expected);
    assert_eq!(
        &args[..5],
        ["run", "--rm", "-i", "--pull=never", "--network=none"]
    );
    assert!(
        !args
            .iter()
            .any(|arg| arg == "-v" || arg.starts_with("--volume"))
    );
    assert!(!args.iter().any(|arg| arg.starts_with("--mount")));
    assert!(args.contains(&"EXPECT_RUSTC_VERSION=1.92.0 (abcdef012 2026-01-01)".to_owned()));
    assert!(args.contains(&"EXPECT_RPMS=bash git".to_owned()));
    assert!(args.contains(&"EXPECT_COMMANDS=br".to_owned()));
    assert!(args.ends_with(&[
        "candidate-id".to_owned(),
        "bash".to_owned(),
        "-s".to_owned()
    ]));
}
