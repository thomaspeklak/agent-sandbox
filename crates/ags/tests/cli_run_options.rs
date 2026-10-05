use ags::cli::{CliError, Command, RunOptions, help_text, parse_args};

fn parse(flags: &[&str]) -> Result<RunOptions, CliError> {
    let args = ["ags", "--agent", "shell"]
        .into_iter()
        .chain(flags.iter().copied())
        .map(str::to_owned);
    match parse_args(args)? {
        Command::Run(opts) => Ok(opts),
        _ => panic!("expected a run"),
    }
}

#[test]
fn run_defaults_keep_tty_and_repo_config_enabled() {
    let opts = parse(&[]).unwrap();
    assert!(opts.tty);
    assert!(opts.repo_config);
    assert_eq!(opts.container_name, None);
    assert_eq!(opts.timeout_seconds, None);
}

#[test]
fn parses_separate_and_equals_forms() {
    for flags in [
        vec![
            "--tty",
            "false",
            "--container-name",
            "Job_123-a",
            "--timeout-seconds",
            "60",
        ],
        vec![
            "--tty=false",
            "--container-name=Job_123-a",
            "--timeout-seconds=60",
        ],
    ] {
        let opts = parse(&flags).unwrap();
        assert!(!opts.tty);
        assert_eq!(opts.container_name.as_deref(), Some("Job_123-a"));
        assert_eq!(opts.timeout_seconds, Some(60));
    }
    assert!(!parse(&["--no-repo-config"]).unwrap().repo_config);
    assert!(parse(&["--tty=true", "--tmux"]).unwrap().tty);
}

#[test]
fn accepts_validation_boundaries() {
    for name in ["a".to_owned(), "0".repeat(80)] {
        assert_eq!(
            parse(&["--container-name", &name]).unwrap().container_name,
            Some(name)
        );
    }
    for seconds in ["1", "86400"] {
        assert_eq!(
            parse(&["--timeout-seconds", seconds])
                .unwrap()
                .timeout_seconds,
            Some(seconds.parse().unwrap())
        );
    }
}

#[test]
fn rejects_invalid_values() {
    for name in [
        "".to_owned(),
        "a".repeat(81),
        "-job".to_owned(),
        "_job".to_owned(),
        "job.name".to_owned(),
        "a/b".to_owned(),
        "café".to_owned(),
        "job name".to_owned(),
    ] {
        assert!(
            matches!(
                parse(&["--container-name", &name]),
                Err(CliError::InvalidRunOption(_))
            ),
            "{name:?}"
        );
    }
    for seconds in ["", "0", "86401", "-1", "1.5", "abc", "4294967296"] {
        assert!(
            matches!(
                parse(&["--timeout-seconds", seconds]),
                Err(CliError::InvalidRunOption(_))
            ),
            "{seconds:?}"
        );
    }
    for tty in ["", "yes", "0", "FALSE"] {
        assert!(
            matches!(parse(&["--tty", tty]), Err(CliError::InvalidRunOption(_))),
            "{tty:?}"
        );
    }
}

#[test]
fn rejects_missing_values() {
    for flag in [
        "--tty",
        "--container-name",
        "--timeout-seconds",
        "--tty=",
        "--container-name=",
        "--timeout-seconds=",
    ] {
        assert!(
            matches!(parse(&[flag]), Err(CliError::InvalidRunOption(_))),
            "{flag}"
        );
    }
}

#[test]
fn rejects_tmux_without_a_tty_in_either_order() {
    for flags in [["--tty=false", "--tmux"], ["--tmux", "--tty=false"]] {
        let error = parse(&flags).unwrap_err();
        assert_eq!(
            error.to_string(),
            "--tty=false cannot be combined with --tmux"
        );
    }
}

#[test]
fn later_values_win_and_passthrough_stays_untouched() {
    let opts = parse(&[
        "--tty=false",
        "--tty=true",
        "--container-name=first",
        "--container-name=last",
        "--timeout-seconds=1",
        "--timeout-seconds=2",
        "--",
        "--tty=false",
        "--no-repo-config",
    ])
    .unwrap();
    assert!(opts.tty);
    assert!(opts.repo_config);
    assert_eq!(opts.container_name.as_deref(), Some("last"));
    assert_eq!(opts.timeout_seconds, Some(2));
    assert_eq!(opts.passthrough_args, ["--tty=false", "--no-repo-config"]);
}

#[test]
fn help_documents_run_controls() {
    for flag in [
        "--tty",
        "--container-name",
        "--timeout-seconds",
        "--no-repo-config",
    ] {
        assert!(help_text().contains(flag), "missing {flag}");
    }
}
