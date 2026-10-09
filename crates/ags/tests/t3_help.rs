use ags::cli::{CliError, T3Action, parse_args};
use std::process::Command;

fn parse(args: &[&str]) -> CliError {
    parse_args(
        std::iter::once("ags")
            .chain(args.iter().copied())
            .map(str::to_owned),
    )
    .unwrap_err()
}

fn help(args: &[&str]) -> String {
    let root = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ags"))
        .args(args)
        .current_dir(root.path())
        .env_clear()
        .env("HOME", root.path())
        .env("XDG_CONFIG_HOME", root.path().join("config"))
        .env("XDG_CACHE_HOME", root.path().join("cache"))
        .env("PATH", root.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    assert_eq!(
        root.path().read_dir().unwrap().count(),
        0,
        "help must not create configuration or caches"
    );
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn parser_preserves_t3_help_scope() {
    for flag in ["-h", "--help"] {
        assert_eq!(parse(&["t3", flag]), CliError::T3HelpRequested(None));
        for (name, action) in [
            ("status", T3Action::Status),
            ("stop", T3Action::Stop),
            ("upgrade", T3Action::Upgrade),
        ] {
            assert_eq!(
                parse(&["t3", name, flag]),
                CliError::T3HelpRequested(Some(action))
            );
        }
    }
}

#[test]
fn t3_help_is_scoped_and_available_without_repository_config_or_external_tools() {
    for flag in ["-h", "--help"] {
        let text = help(&["t3", flag]);
        assert!(text.starts_with("Usage: ags t3 <status|stop|upgrade>"));
        assert!(text.contains("ags --agent t3"));
        assert!(text.contains("--repository <checkout>"));
        assert!(text.contains("default: current directory"));
        assert!(text.contains("Examples:"));
        assert!(!text.contains("Run flags:") && !text.contains("Update-image flags:"));
        assert!(!text.contains("update-agents"));
        assert!(!text.contains("_owner") && !text.contains("_proxy"));
    }
}

#[test]
fn t3_action_help_describes_each_operation_without_loading_registration() {
    for (action, description) in [
        ("status", "as JSON"),
        ("stop", "preserving mounted data"),
        ("upgrade", "Stop active jobs"),
    ] {
        for flag in ["-h", "--help"] {
            let text = help(&["t3", action, "--repository", "/nonexistent/worktree", flag]);
            assert!(text.starts_with(&format!("Usage: ags t3 {action} [--repository <checkout>]")));
            assert!(text.contains(description));
            assert!(text.contains("--repository <checkout>"));
            assert!(!text.contains("Run flags:"));
        }
    }
}

#[test]
fn top_level_help_remains_the_global_command_reference() {
    assert!(help(&["--help"]).contains("Run flags:"));
    assert_eq!(parse(&["--help"]), CliError::HelpRequested);
}
