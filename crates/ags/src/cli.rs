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
    /// Subcommands: setup, doctor, update, install, uninstall.
    Sub(SubCommand),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunOptions {
    pub agent: Agent,
    pub browser: bool,
    pub config_path: Option<PathBuf>,
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubCommand {
    Setup,
    Doctor,
    Update,
    UpdateAgents,
    Install(InstallOptions),
    Uninstall,
    CreateAliases(CreateAliasesOptions),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliError {
    HelpRequested,
    MissingAgent,
    MissingAgentValue,
    MissingConfigValue,
    MissingShellValue,
    MissingAliasModeValue,
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
        "update" => return Ok(Command::Sub(SubCommand::Update)),
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
        _ => {}
    }

    // Parse run command flags
    let mut agent: Option<Agent> = None;
    let mut browser = false;
    let mut config_path: Option<PathBuf> = None;
    let mut passthrough_args = Vec::new();

    // Process the first arg we already consumed (handle `--` as passthrough separator)
    if first == "--" {
        passthrough_args.extend(iter);
    } else {
        parse_run_arg(
            &first,
            &mut iter,
            &mut agent,
            &mut browser,
            &mut config_path,
        )?;

        while let Some(arg) = iter.next() {
            if arg == "--" {
                passthrough_args.extend(iter);
                break;
            }
            parse_run_arg(&arg, &mut iter, &mut agent, &mut browser, &mut config_path)?;
        }
    }

    let agent = agent.ok_or(CliError::MissingAgent)?;

    Ok(Command::Run(RunOptions {
        agent,
        browser,
        config_path,
        passthrough_args,
    }))
}

fn parse_run_arg<I: Iterator<Item = String>>(
    arg: &str,
    iter: &mut I,
    agent: &mut Option<Agent>,
    browser: &mut bool,
    config_path: &mut Option<PathBuf>,
) -> Result<(), CliError> {
    if arg == "-h" || arg == "--help" {
        return Err(CliError::HelpRequested);
    }

    if arg == "--agent" {
        let raw = iter.next().ok_or(CliError::MissingAgentValue)?;
        *agent = Some(Agent::parse(&raw)?);
        return Ok(());
    }

    if let Some(raw) = arg.strip_prefix("--agent=") {
        if raw.is_empty() {
            return Err(CliError::MissingAgentValue);
        }
        *agent = Some(Agent::parse(raw)?);
        return Ok(());
    }

    if arg == "--browser" {
        *browser = true;
        return Ok(());
    }

    if arg == "--config" {
        let raw = iter.next().ok_or(CliError::MissingConfigValue)?;
        *config_path = Some(PathBuf::from(raw));
        return Ok(());
    }

    if let Some(raw) = arg.strip_prefix("--config=") {
        if raw.is_empty() {
            return Err(CliError::MissingConfigValue);
        }
        *config_path = Some(PathBuf::from(raw));
        return Ok(());
    }

    if arg.starts_with('-') {
        return Err(CliError::UnexpectedFlag(arg.to_owned()));
    }

    Err(CliError::UnexpectedPositional(arg.to_owned()))
}

fn parse_install_args<I>(mut iter: I) -> Result<InstallOptions, CliError>
where
    I: Iterator<Item = String>,
{
    let mut link_self = false;
    let mut force = false;

    while let Some(arg) = iter.next() {
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
        if arg.starts_with('-') {
            return Err(CliError::UnexpectedFlag(arg));
        }
        return Err(CliError::UnexpectedPositional(arg));
    }

    Ok(InstallOptions { link_self, force })
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

pub fn help_text() -> &'static str {
    "Usage: ags [command] --agent <pi|claude|codex|gemini|opencode|shell> [flags] -- [args...]\n\
     \n\
     Commands:\n\
     \x20 setup          Generate SSH keys and configure secrets\n\
     \x20 doctor         Run health checks on sandbox configuration\n\
     \x20 update         Rebuild the container image (deps only)\n\
     \x20 update-agents  Install/update agents in persistent volumes\n\
     \x20 install         Install config/assets (optional self-link)\n\
     \x20 uninstall       Reserved (currently no-op)\n\
     \x20 create-aliases  Create managed wrapper scripts and/or shell aliases\n\
     \n\
     install flags:\n\
     \x20 --link-self      Link current ags executable to ~/.local/bin/ags\n\
     \x20 --force          Replace existing ~/.local/bin/ags when used with --link-self\n\
     \n\
     create-aliases flags:\n\
     \x20 --shell <name>    Target shell for alias blocks (fish|zsh|bash; autodetect if omitted)\n\
     \x20 --mode <kind>     wrappers|aliases|both (default: wrappers)\n\
     \x20 --force           Replace existing non-managed targets\n\
     \n\
     Run flags:\n\
     \x20 --agent <name>   Agent to run (required), or 'shell' for interactive bash\n\
     \x20 --browser        Enable browser sidecar\n\
     \x20 --config <path>  Override config file path\n"
}
