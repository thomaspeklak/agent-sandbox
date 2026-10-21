use ags::cli::{CliError, Command, SubCommand, T3Options, parse_args};

fn parse(args: &[&str]) -> Result<T3Options, CliError> {
    let Command::Sub(SubCommand::T3(options)) = parse_args(
        std::iter::once("ags")
            .chain(args.iter().copied())
            .map(str::to_owned),
    )?
    else {
        panic!("expected T3 command")
    };
    Ok(options)
}

#[test]
fn management_defaults_to_global_and_accepts_explicit_environment_or_bulk_selection() {
    for action in ["status", "stop", "upgrade"] {
        let options = parse(&["t3", action]).unwrap();
        assert!(options.environment.is_none() && options.repository.is_none() && !options.all);
        let options = parse(&["t3", action, "ags-t3-0123456789abcdef"]).unwrap();
        assert_eq!(
            options.environment.as_deref(),
            Some("ags-t3-0123456789abcdef")
        );
        let options = parse(&["t3", action, "--repository", "/repo with spaces"]).unwrap();
        assert_eq!(
            options.repository.unwrap().to_str(),
            Some("/repo with spaces")
        );
    }
    for action in ["stop", "upgrade"] {
        assert!(parse(&["t3", action, "--all"]).unwrap().all);
    }
    assert_eq!(
        parse(&["t3", "status", "--all"]),
        Err(CliError::UnexpectedFlag("--all".into()))
    );
}

#[test]
fn conflicting_or_repeated_selectors_fail_before_dispatch() {
    for arguments in [
        vec!["t3", "stop", "env", "--all"],
        vec!["t3", "upgrade", "--all", "--repository", "/repo"],
        vec!["t3", "upgrade", "env", "--repository", "/repo"],
        vec!["t3", "status", "first", "second"],
        vec!["t3", "stop", "--repository", "/one", "--repository", "/two"],
    ] {
        assert_eq!(parse(&arguments), Err(CliError::ConflictingT3Selectors));
    }
    assert_eq!(
        parse(&["t3", "upgrade", "--repository", "--all"]),
        Err(CliError::MissingRepositoryValue)
    );
}

#[test]
fn internal_owner_and_proxy_require_registration_and_reject_public_selectors() {
    for action in ["_owner", "_proxy"] {
        assert_eq!(parse(&["t3", action]), Err(CliError::MissingConfigValue));
        let options =
            parse(&["t3", action, "--registration", "/private/registry/id.json"]).unwrap();
        assert!(options.registration.is_some() && options.environment.is_none() && !options.all);
        for flag in ["--all", "--repository", "environment"] {
            assert!(parse(&["t3", action, flag]).is_err());
        }
    }
}
