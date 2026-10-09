#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Run(RunOptions),
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
    pub rebase: bool,
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
    PruneWorkspaceCaches(PruneWorkspaceCachesOptions),
    Install(InstallOptions),
    Uninstall,
    CreateAliases(CreateAliasesOptions),
    Completions(CompletionsOptions),
    Config,
    Tools(ToolConfigOptions),
    Node(NodeOptions),
    Hooks(crate::hooks::cli::Options),
}
