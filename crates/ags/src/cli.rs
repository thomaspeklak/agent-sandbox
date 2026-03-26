#[path = "cli_help.rs"]
mod help;

use help::HELP_TEXT;

use std::fmt;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Agent {
    Pi,
    Claude,
    Codex,
    Gemini,
    Opencode,
    Shell,
}

impl Agent {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pi => "pi",
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Gemini => "gemini",
            Self::Opencode => "opencode",
            Self::Shell => "shell",
        }
    }

    fn parse(value: &str) -> Result<Self, CliError> {
        match value {
            "pi" => Ok(Self::Pi),
            "claude" => Ok(Self::Claude),
            "codex" => Ok(Self::Codex),
            "gemini" => Ok(Self::Gemini),
            "opencode" => Ok(Self::Opencode),
            "shell" => Ok(Self::Shell),
            _ => Err(CliError::InvalidAgent(value.to_owned())),
        }
    }
}

impl fmt::Display for Agent {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Top-level command parsed from CLI args.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Run an agent inside the sandbox.
    Run(RunOptions),
    /// Subcommands: setup, doctor, update-image, update-agents, install, uninstall, create-aliases, completions.
    Sub(SubCommand),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunOptions {
    pub agent: Agent,
    pub browser: bool,
    pub tmux: bool,
    pub psp: bool,
    pub psp_keep: bool,
    pub yolo: bool,
    pub root: bool,
    pub stop_when_done: bool,
    pub config_path: Option<PathBuf>,
    pub add_dirs: Vec<PathBuf>,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubCommand {
    Setup,
    Doctor,
    UpdateImage,
    UpdateDeprecated,
    UpdateAgents,
    Install(InstallOptions),
    Uninstall,
    CreateAliases(CreateAliasesOptions),
    Completions(CompletionsOptions),
    Config,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliError {
    HelpRequested,
    MissingAgent,
    MissingAgentValue,
    MissingConfigValue,
    MissingShellValue,
    MissingAliasModeValue,
    MissingMountPathValue,
    InvalidAgent(String),
    InvalidShell(String),
    InvalidAliasMode(String),
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
            Self::MissingShellValue => f.write_str("missing value for --shell"),
            Self::MissingAliasModeValue => f.write_str("missing value for --mode"),
            Self::MissingMountPathValue => f.write_str("missing value for --add-dir / -d"),
            Self::InvalidAgent(agent) => write!(f, "invalid agent '{agent}'"),
            Self::InvalidShell(shell) => {
                write!(f, "invalid shell '{shell}' (expected fish|zsh|bash)")
            }
            Self::InvalidAliasMode(mode) => {
                write!(f, "invalid mode '{mode}' (expected wrappers|aliases|both)")
            }
            Self::UnexpectedFlag(flag) => write!(f, "unexpected flag '{flag}'"),
            Self::UnexpectedPositional(arg) => write!(
                f,
                "unexpected positional argument '{arg}' (use '--' before passthrough args)"
            ),
        }
    }
}

pub fn parse_args<I>(args: I) -> Result<Command, CliError>
where
    I: IntoIterator<Item = String>,
{
    let mut iter = args.into_iter();
    let _program = iter.next();

    // Peek at first arg for subcommands
    let first = match iter.next() {
        None => return Err(CliError::MissingAgent),
        Some(arg) => arg,
    };

    match first.as_str() {
        "-h" | "--help" => return Err(CliError::HelpRequested),
        "setup" => return Ok(Command::Sub(SubCommand::Setup)),
        "doctor" => return Ok(Command::Sub(SubCommand::Doctor)),
        "update-image" => return Ok(Command::Sub(SubCommand::UpdateImage)),
        "update" => return Ok(Command::Sub(SubCommand::UpdateDeprecated)),
        "update-agents" => return Ok(Command::Sub(SubCommand::UpdateAgents)),
        "install" => {
            let opts = parse_install_args(iter)?;
            return Ok(Command::Sub(SubCommand::Install(opts)));
        }
        "uninstall" => return Ok(Command::Sub(SubCommand::Uninstall)),
        "create-aliases" => {
            let opts = parse_create_aliases_args(iter)?;
            return Ok(Command::Sub(SubCommand::CreateAliases(opts)));
        }
        "completions" => {
            let opts = parse_completions_args(iter)?;
            return Ok(Command::Sub(SubCommand::Completions(opts)));
        }
        "config" => return Ok(Command::Sub(SubCommand::Config)),
        _ => {}
    }

    // Parse run command flags
    let mut state = RunParseState::default();
    let mut passthrough_args = Vec::new();

    // Process the first arg we already consumed (handle `--` as passthrough separator)
    if first == "--" {
        passthrough_args.extend(iter);
    } else {
        parse_run_arg(&first, &mut iter, &mut state)?;

        while let Some(arg) = iter.next() {
            if arg == "--" {
                passthrough_args.extend(iter);
                break;
            }
            parse_run_arg(&arg, &mut iter, &mut state)?;
        }
    }

    let agent = state.agent.ok_or(CliError::MissingAgent)?;

    Ok(Command::Run(RunOptions {
        agent,
        browser: state.browser,
        tmux: state.tmux,
        psp: state.psp,
        psp_keep: state.psp_keep,
        yolo: state.yolo,
        root: state.root,
        stop_when_done: state.stop_when_done,
        config_path: state.config_path,
        add_dirs: state.add_dirs,
        passthrough_args,
    }))
}

#[derive(Default)]
struct RunParseState {
    agent: Option<Agent>,
    browser: bool,
    tmux: bool,
    psp: bool,
    psp_keep: bool,
    yolo: bool,
    root: bool,
    stop_when_done: bool,
    config_path: Option<PathBuf>,
    add_dirs: Vec<PathBuf>,
}

fn parse_run_arg<I: Iterator<Item = String>>(
    arg: &str,
    iter: &mut I,
    state: &mut RunParseState,
) -> Result<(), CliError> {
    if arg == "-h" || arg == "--help" {
        return Err(CliError::HelpRequested);
    }

    if arg == "--agent" {
        let raw = iter.next().ok_or(CliError::MissingAgentValue)?;
        state.agent = Some(Agent::parse(&raw)?);
        return Ok(());
    }

    if let Some(raw) = arg.strip_prefix("--agent=") {
        if raw.is_empty() {
            return Err(CliError::MissingAgentValue);
        }
        state.agent = Some(Agent::parse(raw)?);
        return Ok(());
    }

    if arg == "--browser" {
        state.browser = true;
        return Ok(());
    }

    if arg == "--tmux" {
        state.tmux = true;
        return Ok(());
    }

    if arg == "--psp" {
        state.psp = true;
        return Ok(());
    }

    if arg == "--psp-keep" {
        state.psp_keep = true;
        return Ok(());
    }

    if arg == "--yolo" {
        state.yolo = true;
        return Ok(());
    }

    if arg == "--root" {
        state.root = true;
        return Ok(());
    }

    if arg == "--stop-when-done" {
        state.stop_when_done = true;
        return Ok(());
    }

    if arg == "--config" {
        let raw = iter.next().ok_or(CliError::MissingConfigValue)?;
        state.config_path = Some(PathBuf::from(raw));
        return Ok(());
    }

    if let Some(raw) = arg.strip_prefix("--config=") {
        if raw.is_empty() {
            return Err(CliError::MissingConfigValue);
        }
        state.config_path = Some(PathBuf::from(raw));
        return Ok(());
    }

    if arg == "--add-dir" || arg == "-d" {
        let raw = iter.next().ok_or(CliError::MissingMountPathValue)?;
        state.add_dirs.push(PathBuf::from(raw));
        return Ok(());
    }

    if let Some(raw) = arg.strip_prefix("--add-dir=") {
        if raw.is_empty() {
            return Err(CliError::MissingMountPathValue);
        }
        state.add_dirs.push(PathBuf::from(raw));
        return Ok(());
    }

    if arg.starts_with('-') {
        return Err(CliError::UnexpectedFlag(arg.to_owned()));
    }

    Err(CliError::UnexpectedPositional(arg.to_owned()))
}

fn parse_install_args<I>(iter: I) -> Result<InstallOptions, CliError>
where
    I: Iterator<Item = String>,
{
    let mut link_self = false;
    let mut force = false;
    let mut add_agent_mounts = false;

    for arg in iter {
        if arg == "-h" || arg == "--help" {
            return Err(CliError::HelpRequested);
        }
        if arg == "--link-self" {
            link_self = true;
            continue;
        }
        if arg == "--force" {
            force = true;
            continue;
        }
        if arg == "--add-agent-mounts" {
            add_agent_mounts = true;
            continue;
        }
        if arg.starts_with('-') {
            return Err(CliError::UnexpectedFlag(arg));
        }
        return Err(CliError::UnexpectedPositional(arg));
    }

    Ok(InstallOptions {
        link_self,
        force,
        add_agent_mounts,
    })
}

fn parse_create_aliases_args<I>(mut iter: I) -> Result<CreateAliasesOptions, CliError>
where
    I: Iterator<Item = String>,
{
    let mut shell = None;
    let mut mode = AliasMode::Wrappers;
    let mut force = false;

    while let Some(arg) = iter.next() {
        if arg == "-h" || arg == "--help" {
            return Err(CliError::HelpRequested);
        }

        if arg == "--force" {
            force = true;
            continue;
        }

        if arg == "--shell" {
            let value = iter.next().ok_or(CliError::MissingShellValue)?;
            shell = Some(Shell::parse(&value)?);
            continue;
        }

        if let Some(value) = arg.strip_prefix("--shell=") {
            if value.is_empty() {
                return Err(CliError::MissingShellValue);
            }
            shell = Some(Shell::parse(value)?);
            continue;
        }

        if arg == "--mode" {
            let value = iter.next().ok_or(CliError::MissingAliasModeValue)?;
            mode = AliasMode::parse(&value)?;
            continue;
        }

        if let Some(value) = arg.strip_prefix("--mode=") {
            if value.is_empty() {
                return Err(CliError::MissingAliasModeValue);
            }
            mode = AliasMode::parse(value)?;
            continue;
        }

        if arg.starts_with('-') {
            return Err(CliError::UnexpectedFlag(arg));
        }
        return Err(CliError::UnexpectedPositional(arg));
    }

    Ok(CreateAliasesOptions { shell, mode, force })
}

fn parse_completions_args<I>(mut iter: I) -> Result<CompletionsOptions, CliError>
where
    I: Iterator<Item = String>,
{
    let mut shell = None;

    while let Some(arg) = iter.next() {
        if arg == "-h" || arg == "--help" {
            return Err(CliError::HelpRequested);
        }

        if arg == "--shell" {
            let value = iter.next().ok_or(CliError::MissingShellValue)?;
            shell = Some(Shell::parse(&value)?);
            continue;
        }

        if let Some(value) = arg.strip_prefix("--shell=") {
            if value.is_empty() {
                return Err(CliError::MissingShellValue);
            }
            shell = Some(Shell::parse(value)?);
            continue;
        }

        if arg.starts_with('-') {
            return Err(CliError::UnexpectedFlag(arg));
        }
        return Err(CliError::UnexpectedPositional(arg));
    }

    let shell = shell.ok_or(CliError::MissingShellValue)?;
    Ok(CompletionsOptions { shell })
}

pub fn help_text() -> &'static str {
    HELP_TEXT
}
