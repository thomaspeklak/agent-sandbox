use ags::cli::{
    Agent, AliasMode, CliError, Command, CompletionsOptions, CreateAliasesOptions, InstallOptions,
    Shell, SubCommand, ToolConfigOptions, UpdateAgentsCliOptions, UpdateImageOptions, help_text,
    parse_args,
};

fn args(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| s.to_string()).collect()
}

#[test]
fn parses_agent_and_passthrough_args() {
    let cmd = parse_args(args(&["ags", "--agent", "pi", "--", "--continue"]))
        .expect("expected valid args");

    match cmd {
        Command::Run(opts) => {
            assert_eq!(opts.agent, Agent::Pi);
            assert_eq!(opts.passthrough_args, vec!["--continue"]);
            assert!(!opts.browser);
            assert!(!opts.tmux);
            assert!(!opts.psp);
            assert!(!opts.yolo);
            assert!(!opts.lockdown);
            assert!(!opts.wayland_compositor_passthrough);
            assert!(opts.config_path.is_none());
        }
        _ => panic!("expected Run command"),
    }
}

#[test]
fn rejects_missing_agent() {
    let error = parse_args(args(&["ags", "--", "--continue"])).expect_err("expected parse error");
    assert_eq!(error, CliError::MissingAgent);
}

#[test]
fn parses_browser_flag() {
    let cmd = parse_args(args(&["ags", "--agent", "pi", "--browser"])).unwrap();
    match cmd {
        Command::Run(opts) => assert!(opts.browser),
        _ => panic!("expected Run command"),
    }
}

#[test]
fn parses_tmux_flag() {
    let cmd = parse_args(args(&["ags", "--agent", "pi", "--tmux"])).unwrap();
    match cmd {
        Command::Run(opts) => assert!(opts.tmux),
        _ => panic!("expected Run command"),
    }
}

#[test]
fn parses_psp_flag() {
    let cmd = parse_args(args(&["ags", "--agent", "pi", "--psp"])).unwrap();
    match cmd {
        Command::Run(opts) => {
            assert!(opts.psp);
            assert!(!opts.psp_keep);
        }
        _ => panic!("expected Run command"),
    }
}

#[test]
fn parses_psp_keep_flag() {
    let cmd = parse_args(args(&["ags", "--agent", "pi", "--psp", "--psp-keep"])).unwrap();
    match cmd {
        Command::Run(opts) => {
            assert!(opts.psp);
            assert!(opts.psp_keep);
        }
        _ => panic!("expected Run command"),
    }
}

#[test]
fn parses_yolo_flag() {
    let cmd = parse_args(args(&["ags", "--agent", "pi", "--yolo"])).unwrap();
    match cmd {
        Command::Run(opts) => assert!(opts.yolo),
        _ => panic!("expected Run command"),
    }
}

#[test]
fn parses_root_flag() {
    let cmd = parse_args(args(&["ags", "--agent", "claude", "--root"])).unwrap();
    match cmd {
        Command::Run(opts) => assert!(opts.root),
        _ => panic!("expected Run command"),
    }
}

#[test]
fn parses_lockdown_flag() {
    let cmd = parse_args(args(&["ags", "--agent", "pi", "--lockdown"])).unwrap();
    match cmd {
        Command::Run(opts) => assert!(opts.lockdown),
        _ => panic!("expected Run command"),
    }
}

#[test]
fn parses_wayland_compositor_passthrough_flag() {
    let cmd = parse_args(args(&[
        "ags",
        "--agent",
        "pi",
        "--wayland-compositor-passthrough",
    ]))
    .unwrap();
    match cmd {
        Command::Run(opts) => assert!(opts.wayland_compositor_passthrough),
        _ => panic!("expected Run command"),
    }
}

#[test]
fn parses_defaults_flag() {
    let cmd = parse_args(args(&[
        "ags",
        "--agent",
        "claude",
        "--defaults",
        "--",
        "--model",
        "opus",
    ]))
    .unwrap();
    match cmd {
        Command::Run(opts) => {
            assert_eq!(opts.agent, Agent::Claude);
            assert_eq!(
                opts.passthrough_args,
                vec![
                    "--strict-mcp-config",
                    "--dangerously-skip-permissions",
                    "--model",
                    "opus"
                ]
            );
        }
        _ => panic!("expected Run command"),
    }
}

#[test]
fn parses_defaults_short_flag() {
    let cmd = parse_args(args(&["ags", "--agent", "gemini", "-D"])).unwrap();
    match cmd {
        Command::Run(opts) => {
            assert_eq!(opts.agent, Agent::Gemini);
            assert_eq!(opts.passthrough_args, vec!["--yolo"]);
        }
        _ => panic!("expected Run command"),
    }
}

#[test]
fn parses_config_flag() {
    let cmd = parse_args(args(&["ags", "--agent", "pi", "--config", "/tmp/c.toml"])).unwrap();
    match cmd {
        Command::Run(opts) => {
            assert_eq!(opts.config_path.unwrap().to_str().unwrap(), "/tmp/c.toml");
        }
        _ => panic!("expected Run command"),
    }
}

#[test]
fn parses_subcommands() {
    for (arg, expected) in [
        ("setup", SubCommand::Setup),
        ("doctor", SubCommand::Doctor),
        (
            "update-image",
            SubCommand::UpdateImage(UpdateImageOptions::default()),
        ),
        (
            "update",
            SubCommand::UpdateDeprecated(UpdateImageOptions::default()),
        ),
        (
            "update-agents",
            SubCommand::UpdateAgents(UpdateAgentsCliOptions::default()),
        ),
        ("uninstall", SubCommand::Uninstall),
    ] {
        let cmd = parse_args(args(&["ags", arg])).unwrap();
        assert_eq!(cmd, Command::Sub(expected));
    }
}

#[test]
fn parses_update_image_keep_existing_flag() {
    let cmd = parse_args(args(&["ags", "update-image", "--keep-existing"])).unwrap();
    assert_eq!(
        cmd,
        Command::Sub(SubCommand::UpdateImage(UpdateImageOptions {
            keep_existing: true,
            ..Default::default()
        }))
    );
}

#[test]
fn parses_deprecated_update_keep_existing_flag() {
    let cmd = parse_args(args(&["ags", "update", "--keep-existing"])).unwrap();
    assert_eq!(
        cmd,
        Command::Sub(SubCommand::UpdateDeprecated(UpdateImageOptions {
            keep_existing: true,
            ..Default::default()
        }))
    );
}

#[test]
fn parses_update_image_config_path() {
    let cmd = parse_args(args(&["ags", "update-image", "--config=/tmp/ags.toml"])).unwrap();
    assert_eq!(
        cmd,
        Command::Sub(SubCommand::UpdateImage(UpdateImageOptions {
            config_path: Some("/tmp/ags.toml".into()),
            ..Default::default()
        }))
    );
}

#[test]
fn parses_update_image_config_path_as_separate_value() {
    let cmd = parse_args(args(&["ags", "update-image", "--config", "/tmp/ags.toml"])).unwrap();
    assert_eq!(
        cmd,
        Command::Sub(SubCommand::UpdateImage(UpdateImageOptions {
            config_path: Some("/tmp/ags.toml".into()),
            ..Default::default()
        }))
    );
}

#[test]
fn parses_update_agents_config_path() {
    for arguments in [
        &["ags", "update-agents", "--config=/tmp/ags.toml"][..],
        &["ags", "update-agents", "--config", "/tmp/ags.toml"][..],
    ] {
        let cmd = parse_args(args(arguments)).unwrap();
        assert_eq!(
            cmd,
            Command::Sub(SubCommand::UpdateAgents(UpdateAgentsCliOptions {
                config_path: Some("/tmp/ags.toml".into()),
            }))
        );
    }
}

#[test]
fn update_agents_rejects_invalid_arguments() {
    assert_eq!(
        parse_args(args(&["ags", "update-agents", "--config"])).unwrap_err(),
        CliError::MissingConfigValue
    );
    assert_eq!(
        parse_args(args(&["ags", "update-agents", "unexpected"])).unwrap_err(),
        CliError::UnexpectedPositional("unexpected".into())
    );
}

#[test]
fn update_image_rejects_missing_or_empty_config_values() {
    for arguments in [
        &["ags", "update-image", "--config"][..],
        &["ags", "update-image", "--config", ""][..],
        &["ags", "update-image", "--config="][..],
    ] {
        let error = parse_args(args(arguments)).expect_err("expected parse error");
        assert_eq!(error, CliError::MissingConfigValue);
    }
}

#[test]
fn help_shows_update_image_but_not_deprecated_update_alias() {
    let help = help_text();
    assert!(help.contains("update-image"));
    assert!(help.contains("\ntools          Choose sandbox tools and agent CLIs\n"));
    assert!(help.contains("--keep-existing Keep the previous image after a successful rebuild"));
    assert!(help.contains("--psp                Enable podman-socket-proxy for Docker/Testcontainers flows (policy-gated)"));
    assert!(help.contains("--psp-keep           Keep PSP-created containers after session exit (debug; requires --psp)"));
    assert!(help.contains("--wayland-compositor-passthrough"));
    assert!(help.contains("--env <NAME=VALUE>"));
    assert!(help.contains(
        "--shell <name> Shell to generate completion script for (fish|zsh|bash)\n\nTools flags:"
    ));
    assert!(!help.contains("\n     \x20 update         Rebuild container image"));
}

#[test]
fn parses_tools_with_positional_packages() {
    let cmd = parse_args(args(&["ags", "tools", "config/tool-packages.example.json"])).unwrap();

    assert_eq!(
        cmd,
        Command::Sub(SubCommand::Tools(ToolConfigOptions {
            packages_path: "config/tool-packages.example.json".into(),
            config_path: None,
        }))
    );
}

#[test]
fn parses_tools_with_flags() {
    let cmd = parse_args(args(&[
        "ags",
        "tools",
        "--packages",
        "packages.json",
        "--config=/tmp/ags.toml",
    ]))
    .unwrap();

    assert_eq!(
        cmd,
        Command::Sub(SubCommand::Tools(ToolConfigOptions {
            packages_path: "packages.json".into(),
            config_path: Some("/tmp/ags.toml".into()),
        }))
    );
}

#[test]
fn tools_requires_packages_path() {
    let error = parse_args(args(&["ags", "tools"])).expect_err("expected parse error");
    assert_eq!(error, CliError::MissingToolPackagesPath);
    assert_eq!(
        error.to_string(),
        "missing tool catalog JSON path (use `ags tools <path>` or `ags tools --packages <path>`)"
    );
}

#[test]
fn tools_rejects_missing_or_empty_packages_values() {
    for arguments in [
        &["ags", "tools", "--packages"][..],
        &["ags", "tools", "--packages", ""][..],
        &["ags", "tools", "--packages="][..],
    ] {
        let error = parse_args(args(arguments)).expect_err("expected parse error");
        assert_eq!(error, CliError::MissingToolPackagesValue);
    }
}

#[test]
fn tools_rejects_missing_or_empty_config_values() {
    for arguments in [
        &["ags", "tools", "packages.json", "--config"][..],
        &["ags", "tools", "packages.json", "--config", ""][..],
        &["ags", "tools", "packages.json", "--config="][..],
    ] {
        let error = parse_args(args(arguments)).expect_err("expected parse error");
        assert_eq!(error, CliError::MissingConfigValue);
    }
}

#[test]
fn tools_rejects_extra_positional_and_unknown_flag() {
    let error =
        parse_args(args(&["ags", "tools", "a.json", "b.json"])).expect_err("expected parse error");
    assert_eq!(error, CliError::UnexpectedPositional("b.json".to_owned()));

    let error = parse_args(args(&["ags", "tools", "--nope"])).expect_err("expected parse error");
    assert_eq!(error, CliError::UnexpectedFlag("--nope".to_owned()));
}

#[test]
fn parses_install_defaults() {
    let cmd = parse_args(args(&["ags", "install"])).unwrap();
    assert_eq!(
        cmd,
        Command::Sub(SubCommand::Install(InstallOptions {
            link_self: false,
            force: false,
            add_agent_mounts: false,
        }))
    );
}

#[test]
fn parses_install_flags() {
    let cmd = parse_args(args(&["ags", "install", "--link-self", "--force"])).unwrap();
    assert_eq!(
        cmd,
        Command::Sub(SubCommand::Install(InstallOptions {
            link_self: true,
            force: true,
            add_agent_mounts: false,
        }))
    );
}

#[test]
fn parses_install_add_agent_mounts_flag() {
    let cmd = parse_args(args(&["ags", "install", "--add-agent-mounts"])).unwrap();
    assert_eq!(
        cmd,
        Command::Sub(SubCommand::Install(InstallOptions {
            link_self: false,
            force: false,
            add_agent_mounts: true,
        }))
    );
}

#[test]
fn parses_run_add_dir_flags() {
    let cmd = parse_args(args(&[
        "ags",
        "--agent",
        "claude",
        "--add-dir",
        "~/code",
        "-d",
        "/data/shared",
    ]))
    .unwrap();

    match cmd {
        Command::Run(opts) => {
            assert_eq!(opts.agent, Agent::Claude);
            assert_eq!(
                opts.add_dirs,
                vec![
                    std::path::PathBuf::from("~/code"),
                    std::path::PathBuf::from("/data/shared")
                ]
            );
        }
        _ => panic!("expected Run command"),
    }
}

#[test]
fn run_add_dir_requires_value() {
    let err = parse_args(args(&["ags", "--agent", "pi", "-d"])).expect_err("expected parse error");
    assert_eq!(err, CliError::MissingMountPathValue);
}

#[test]
fn parses_repeatable_run_env_flags() {
    let cmd = parse_args(args(&[
        "ags",
        "--agent",
        "pi",
        "--env",
        "BROWSER_URL=http://127.0.0.1:9222",
        "--env=EMPTY=",
        "--env",
        "TOKEN=value=with=equals",
    ]))
    .unwrap();

    match cmd {
        Command::Run(opts) => assert_eq!(
            opts.env,
            vec![
                ("BROWSER_URL".to_owned(), "http://127.0.0.1:9222".to_owned()),
                ("EMPTY".to_owned(), String::new()),
                ("TOKEN".to_owned(), "value=with=equals".to_owned()),
            ]
        ),
        _ => panic!("expected Run command"),
    }
}

#[test]
fn run_env_rejects_missing_or_invalid_assignments() {
    for flag in ["--env", "--env="] {
        let err = parse_args(args(&["ags", "--agent", "pi", flag]))
            .expect_err("expected missing env assignment");
        assert_eq!(err, CliError::MissingEnvValue);
    }

    for assignment in ["NO_EQUALS", "=value", "9INVALID=value", "BAD-NAME=value"] {
        let err = parse_args(args(&["ags", "--agent", "pi", "--env", assignment]))
            .expect_err("expected invalid env assignment");
        assert_eq!(err, CliError::InvalidEnvAssignment(assignment.to_owned()));
    }

    let err = parse_args(args(&["ags", "--agent", "pi", "--env", "AGS_LOCKDOWN=0"]))
        .expect_err("expected reserved env name");
    assert_eq!(err, CliError::ReservedEnvName("AGS_LOCKDOWN".to_owned()));
}

#[test]
fn parses_create_aliases_defaults() {
    let cmd = parse_args(args(&["ags", "create-aliases"])).unwrap();
    assert_eq!(
        cmd,
        Command::Sub(SubCommand::CreateAliases(CreateAliasesOptions {
            shell: None,
            mode: AliasMode::Wrappers,
            force: false,
        }))
    );
}

#[test]
fn parses_create_aliases_flags() {
    let cmd = parse_args(args(&[
        "ags",
        "create-aliases",
        "--shell",
        "fish",
        "--mode",
        "both",
        "--force",
    ]))
    .unwrap();

    assert_eq!(
        cmd,
        Command::Sub(SubCommand::CreateAliases(CreateAliasesOptions {
            shell: Some(Shell::Fish),
            mode: AliasMode::Both,
            force: true,
        }))
    );
}

#[test]
fn parses_op_secret_sets_in_cli_order() {
    let cmd = parse_args(args(&[
        "ags",
        "--agent",
        "pi",
        "--op-secret-set",
        "Employee/first item",
        "-1",
        "Employee/second/item",
        "--op-secret-set=IDs/vault-item",
        "--",
        "-1",
    ]))
    .unwrap();

    match cmd {
        Command::Run(opts) => assert_eq!(
            opts.op_secret_sets,
            vec![
                "Employee/first item",
                "Employee/second/item",
                "IDs/vault-item"
            ]
        ),
        _ => panic!("expected Run command"),
    }
}

#[test]
fn op_secret_set_requires_value() {
    for flag in ["--op-secret-set", "-1", "--op-secret-set="] {
        let err = parse_args(args(&["ags", "--agent", "pi", flag]))
            .expect_err("expected missing secret-set value");
        assert_eq!(err, CliError::MissingOpSecretSetValue);
    }
}

#[test]
fn parses_agent_equals_form() {
    let cmd = parse_args(args(&["ags", "--agent=claude"])).unwrap();
    match cmd {
        Command::Run(opts) => assert_eq!(opts.agent, Agent::Claude),
        _ => panic!("expected Run command"),
    }
}

#[test]
fn parses_completions_flags() {
    let cmd = parse_args(args(&["ags", "completions", "--shell", "zsh"])).unwrap();
    assert_eq!(
        cmd,
        Command::Sub(SubCommand::Completions(CompletionsOptions {
            shell: Shell::Zsh,
        }))
    );
}

#[test]
fn completions_requires_shell() {
    let err = parse_args(args(&["ags", "completions"]))
        .expect_err("expected missing shell value for completions");
    assert_eq!(err, CliError::MissingShellValue);
}

#[test]
fn rejects_invalid_alias_mode() {
    let err = parse_args(args(&["ags", "create-aliases", "--mode", "weird"]))
        .expect_err("expected parse failure");
    assert_eq!(err, CliError::InvalidAliasMode("weird".to_owned()));
}
