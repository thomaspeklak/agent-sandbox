#[path = "cli_agent.rs"]
mod agent;
#[path = "cli_help.rs"]
mod help;
#[path = "cli_node.rs"]
mod node;
#[path = "cli_run.rs"]
mod run;
#[path = "cli_subcommands.rs"]
mod subcommands;
#[path = "cli_tools.rs"]
mod tools;
#[path = "cli_update_agents.rs"]
mod update_agents;
#[path = "cli_update_image.rs"]
mod update_image;

use help::HELP_TEXT;
use std::fmt;
use std::path::PathBuf;

pub use agent::Agent;
pub use node::{NodeCommand, NodeOptions};
pub use tools::ToolConfigOptions;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Run(RunOptions),
    Sub(SubCommand),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunOptions {
    pub agent: Agent,
    pub tty: bool,
    pub container_name: Option<String>,
    pub timeout_seconds: Option<u32>,
    pub repo_config: bool,
    pub browser: bool,
    pub tmux: bool,
    pub psp: bool,
    pub psp_keep: bool,
    pub yolo: bool,
    pub root: bool,
    pub lockdown: bool,
    pub wayland_compositor_passthrough: bool,
    pub stop_when_done: bool,
    pub config_path: Option<PathBuf>,
    pub add_dirs: Vec<PathBuf>,
    pub env: Vec<(String, String)>,
    pub op_secret_sets: Vec<String>,
    pub passthrough_args: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AliasMode {
    Wrappers,
    Aliases,
    Both,
}

impl AliasMode {
    fn parse(value: &str) -> Result<Self, CliError> {
        match value {
            "wrappers" => Ok(Self::Wrappers),
            "aliases" => Ok(Self::Aliases),
            "both" => Ok(Self::Both),
            _ => Err(CliError::InvalidAliasMode(value.to_owned())),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shell {
    Fish,
    Zsh,
    Bash,
}

impl Shell {
    fn parse(value: &str) -> Result<Self, CliError> {
        match value {
            "fish" => Ok(Self::Fish),
            "zsh" => Ok(Self::Zsh),
            "bash" => Ok(Self::Bash),
            _ => Err(CliError::InvalidShell(value.to_owned())),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreateAliasesOptions {
    pub shell: Option<Shell>,
    pub mode: AliasMode,
    pub force: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallOptions {
    pub link_self: bool,
    pub force: bool,
    pub add_agent_mounts: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletionsOptions {
    pub shell: Shell,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UpdateImageOptions {
    pub keep_existing: bool,
    pub config_path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct UpdateAgentsCliOptions {
    pub config_path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubCommand {
    Setup,
    Doctor,
    UpdateImage(UpdateImageOptions),
    UpdateDeprecated(UpdateImageOptions),
    UpdateAgents(UpdateAgentsCliOptions),
    Install(InstallOptions),
    Uninstall,
    CreateAliases(CreateAliasesOptions),
    Completions(CompletionsOptions),
    Config,
    Tools(ToolConfigOptions),
    Node(NodeOptions),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliError {
    HelpRequested,
    MissingAgent,
    MissingAgentValue,
    MissingConfigValue,
    MissingToolPackagesValue,
    MissingToolPackagesPath,
    MissingNodeCommand,
    MissingNodeVersion,
    MissingEnvValue,
    MissingOpSecretSetValue,
    MissingShellValue,
    MissingAliasModeValue,
    MissingMountPathValue,
    InvalidAgent(String),
    InvalidEnvAssignment(String),
    ReservedEnvName(String),
    InvalidShell(String),
    InvalidAliasMode(String),
    InvalidRunOption(String),
    UnexpectedFlag(String),
    UnexpectedPositional(String),
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::HelpRequested => f.write_str("help requested"),
            Self::MissingAgent => f.write_str(
                "missing required argument: --agent <pi|claude|codex|gemini|opencode|shell>",
            ),
            Self::MissingAgentValue => f.write_str("missing value for --agent"),
            Self::MissingConfigValue => f.write_str("missing value for --config"),
            Self::MissingToolPackagesValue => f.write_str("missing value for --packages"),
            Self::MissingToolPackagesPath => {
                f.write_str(
                    "missing tool catalog JSON path (use `ags tools <path>` or `ags tools --packages <path>`)",
                )
            }
            Self::MissingNodeCommand => {
                f.write_str("missing Node runtime command (expected `install <version>` or `list`)")
            }
            Self::MissingNodeVersion => f.write_str("missing Node version for `node install`"),
            Self::MissingEnvValue => f.write_str("missing value for --env (expected NAME=VALUE)"),
            Self::MissingOpSecretSetValue => f.write_str("missing value for --op-secret-set / -1"),
            Self::MissingShellValue => f.write_str("missing value for --shell"),
            Self::MissingAliasModeValue => f.write_str("missing value for --mode"),
            Self::MissingMountPathValue => f.write_str("missing value for --add-dir / -d"),
            Self::InvalidAgent(agent) => write!(f, "invalid agent '{agent}'"),
            Self::InvalidEnvAssignment(value) => write!(
                f,
                "invalid environment assignment '{value}' (expected NAME=VALUE)"
            ),
            Self::ReservedEnvName(name) => {
                write!(
                    f,
                    "environment variable '{name}' uses the reserved AGS_ prefix"
                )
            }
            Self::InvalidShell(shell) => {
                write!(f, "invalid shell '{shell}' (expected fish|zsh|bash)")
            }
            Self::InvalidAliasMode(mode) => {
                write!(f, "invalid mode '{mode}' (expected wrappers|aliases|both)")
            }
            Self::InvalidRunOption(message) => f.write_str(message),
            Self::UnexpectedFlag(flag) => write!(f, "unexpected flag '{flag}'"),
            Self::UnexpectedPositional(arg) => write!(
                f,
                "unexpected positional argument '{arg}' (use '--' before passthrough args)"
            ),
        }
    }
}

fn required_value<T: AsRef<str>>(value: Option<T>, error: CliError) -> Result<T, CliError> {
    value
        .filter(|value| !value.as_ref().is_empty())
        .ok_or(error)
}

pub fn parse_args<I>(args: I) -> Result<Command, CliError>
where
    I: IntoIterator<Item = String>,
{
    let mut iter = args.into_iter();
    let _program = iter.next();

    let first = match iter.next() {
        None => return Err(CliError::MissingAgent),
        Some(arg) => arg,
    };

    match first.as_str() {
        "-h" | "--help" => return Err(CliError::HelpRequested),
        "setup" => return Ok(Command::Sub(SubCommand::Setup)),
        "doctor" => return Ok(Command::Sub(SubCommand::Doctor)),
        "update-image" => {
            return Ok(Command::Sub(SubCommand::UpdateImage(
                update_image::parse_args(iter)?,
            )));
        }
        "update" => {
            return Ok(Command::Sub(SubCommand::UpdateDeprecated(
                update_image::parse_args(iter)?,
            )));
        }
        "update-agents" => {
            return Ok(Command::Sub(SubCommand::UpdateAgents(
                update_agents::parse_args(iter)?,
            )));
        }
        "install" => {
            let opts = subcommands::parse_install_args(iter)?;
            return Ok(Command::Sub(SubCommand::Install(opts)));
        }
        "uninstall" => return Ok(Command::Sub(SubCommand::Uninstall)),
        "create-aliases" => {
            let opts = subcommands::parse_create_aliases_args(iter)?;
            return Ok(Command::Sub(SubCommand::CreateAliases(opts)));
        }
        "completions" => {
            let opts = subcommands::parse_completions_args(iter)?;
            return Ok(Command::Sub(SubCommand::Completions(opts)));
        }
        "config" => return Ok(Command::Sub(SubCommand::Config)),
        "tools" => {
            let opts = tools::parse_tools_args(iter)?;
            return Ok(Command::Sub(SubCommand::Tools(opts)));
        }
        "node" | "runtime" | "runtimes" => {
            let opts = node::parse_args(iter)?;
            return Ok(Command::Sub(SubCommand::Node(opts)));
        }
        _ => {}
    }

    run::parse_args(first, iter)
}

pub fn help_text() -> &'static str {
    HELP_TEXT
}
