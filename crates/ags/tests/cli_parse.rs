use ags::cli::{
    Agent, AliasMode, CliError, Command, CompletionsOptions, CreateAliasesOptions, InstallOptions,
    Shell, SubCommand, parse_args,
};

fn args(items: &[&str]) -> Vec<String> {
    items.iter().map(|item| (*item).to_owned()).collect()
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
        ("update", SubCommand::Update),
        ("update-agents", SubCommand::UpdateAgents),
        ("uninstall", SubCommand::Uninstall),
    ] {
        let cmd = parse_args(args(&["ags", arg])).unwrap();
        assert_eq!(cmd, Command::Sub(expected));
    }
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
