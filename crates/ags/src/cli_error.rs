use super::CliError;
use std::fmt;

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::HelpRequested => f.write_str("help requested"),
            Self::MissingAgent => f.write_str("missing required argument: --agent <pi|claude|codex|gemini|opencode|t3|shell>"),
            Self::MissingAgentValue => f.write_str("missing value for --agent"),
            Self::MissingConfigValue => f.write_str("missing value for --config"),
            Self::MissingToolPackagesValue => f.write_str("missing value for --packages"),
            Self::MissingToolPackagesPath => f.write_str("missing tool catalog JSON path (use `ags tools <path>` or `ags tools --packages <path>`)"),
            Self::MissingNodeCommand => f.write_str("missing Node runtime command (expected `install <version>` or `list`)"),
            Self::MissingNodeVersion => f.write_str("missing Node version for `node install`"),
            Self::MissingEnvValue => f.write_str("missing value for --env (expected NAME=VALUE)"),
            Self::MissingOpSecretSetValue => f.write_str("missing value for --op-secret-set / -1"),
            Self::MissingShellValue => f.write_str("missing value for --shell"),
            Self::MissingAliasModeValue => f.write_str("missing value for --mode"),
            Self::MissingMountPathValue => f.write_str("missing value for --add-dir / -d"),
            Self::InvalidAgent(agent) => write!(f, "invalid agent '{agent}'"),
            Self::InvalidEnvAssignment(value) => write!(f, "invalid environment assignment '{value}' (expected NAME=VALUE)"),
            Self::ReservedEnvName(name) => write!(f, "environment variable '{name}' uses the reserved AGS_ prefix"),
            Self::InvalidShell(shell) => write!(f, "invalid shell '{shell}' (expected fish|zsh|bash)"),
            Self::InvalidAliasMode(mode) => write!(f, "invalid mode '{mode}' (expected wrappers|aliases|both)"),
            Self::InvalidPruneOption(flag) => write!(f, "invalid or missing numeric value for {flag}"),
            Self::UnexpectedFlag(flag) => write!(f, "unexpected flag '{flag}'"),
            Self::UnexpectedPositional(arg) => write!(f, "unexpected positional argument '{arg}' (use '--' before passthrough args)"),
        }
    }
}
