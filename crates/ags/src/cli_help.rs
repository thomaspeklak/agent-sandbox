pub const HELP_TEXT: &str = "\
Usage: ags [command] --agent <pi|claude|codex|gemini|opencode|t3|shell> [flags] -- [args...]

\
Commands:
\
  setup          Generate SSH keys and configure secrets
\
  doctor         Run health checks on sandbox configuration
\
  update-image   Check for and apply sandbox image updates
\
  update-agents  Reconcile selected agents in persistent volumes
\
  prune-workspace-caches  Collect orphaned checkout caches (cron-friendly)
\
  install        Install config/assets (optional self-link)
\
  uninstall      Reserved (currently no-op)
\
  create-aliases Create managed wrapper scripts and/or shell aliases
\
  completions    Print shell completion script to stdout
\
  tools          Choose sandbox tools and agent CLIs
\
  node           Install or list user-managed Node.js versions
\
  t3             Manage a registered T3 repository environment
\
                 status|stop|upgrade [--repository <checkout>]
\
Run flags:
\
  --agent <name>       Agent to run (required), or 'shell' for interactive bash
\
  --browser            Start browser sidecar and browser skill wiring
\
  --tmux               Launch the agent inside a tmux session
\
  --psp                Enable podman-socket-proxy for Docker/Testcontainers flows (policy-gated)
\
  --psp-keep           Keep PSP-created containers after session exit (debug; requires --psp)
\
  --yolo               Disable AGS Pi/Claude guard integrations for this run
\
  --root               Run agent with root access inside the sandbox
\
  --lockdown           Minimize host exposure for this run (fail-closed)
\
  --wayland-compositor-passthrough
\
                       Mount the real Wayland compositor socket (broad desktop access)
\
  --stop-when-done     Exit container when agent finishes (tmux mode)
\
  --defaults, -D       Apply AGS-managed defaults for the selected agent harness
\
  --config <path>      Use an alternate AGS config file
\
  --add-dir, -d <path> Add an extra host directory mount (repeatable)
\
  --env <NAME=VALUE>   Set a container environment variable (repeatable)
\
  --op-secret-set, -1 <vault/item>
\
                       Inject fields from a 1Password Secure Note (repeatable; CLI-only)

\
Update-image flags:
\
  --keep-existing Keep the previous image after a successful rebuild
\
  --rebase        Refresh the Fedora base within its release and restart OS update layers
\
  --config <path> Config file to build from (default: ~/.config/ags/config.toml)

\
Update-agents flags:
\
  --config <path> Config file to reconcile from (default: ~/.config/ags/config.toml)

\
Prune-workspace-caches flags:
\
  --config <path>       Host config only (no repository overlays or prompts)
\
  --dry-run             Report eligible caches without deleting or marking orphans
\
  --grace-days <n>      Days since first observed orphaned (default: 7)
\
  --max-caches <n>      Maximum cache trees processed (default: 2)
\
  --max-deletions <n>   Maximum file/directory unlinks (default: 1000; minimum: 3)
\
  --quiet               Suppress normal output (errors still reported)

\
Install flags:
\
  --link-self        Link current ags executable to ~/.local/bin/ags
\
  --force            Replace existing ~/.local/bin/ags when used with --link-self
\
  --add-agent-mounts Append default [[agent_mount]] entries to ~/.config/ags/config.toml

\
Create-aliases flags:
\
  --shell <name> Target shell for alias blocks (fish|zsh|bash; autodetect if omitted)
\
  --mode <kind>  wrappers|aliases|both (default: wrappers)
\
  --force        Replace existing non-managed targets

\
Completions flags:
\
  --shell <name> Shell to generate completion script for (fish|zsh|bash)

\
Tools flags:
\
  --packages <path> Tool catalog JSON file (or pass as first positional argument)
\
  --config <path>   Config file to update (default: ~/.config/ags/config.toml)

\
Node runtime commands:
\
  ags node install <version>  Install a numeric Node version through mise
\
  ags node list               List installed user-managed Node versions
\
  --config <path>             Use an alternate AGS config file
";

const T3_HELP_TEXT: &str = r#"Usage: ags t3 <status|stop|upgrade> [environment | --repository <checkout> | --all]

Manage all registered T3 environments from any directory.
Start or register an environment with: ags --agent t3

Commands:
  status         List all registered environments and their status as JSON
  stop           Choose working environments to stop using unchecked checkboxes
  upgrade        Choose registered environments to recreate, including stopped ones

Flags:
  environment              Target the environment ID (SSH alias) shown by status
  --repository <checkout>  Target one main checkout or worktree explicitly
  --all                    Upgrade all valid registrations; stop all working instances
  -h, --help               Show help for this command

Global stop/upgrade require a terminal for selection, or explicit --all.
Escape/Ctrl-C or an empty selection leaves all environments unchanged.

Examples:
  ags t3 status
  ags t3 upgrade
  ags t3 stop ags-t3-0123456789abcdef
  ags t3 upgrade --repository /path/to/checkout
  ags t3 upgrade --all
"#;

pub fn t3_help_text(action: Option<super::T3Action>) -> String {
    use super::T3Action;
    let (command, description) = match action {
        Some(T3Action::Status) => (
            "status",
            "List all registered environments as JSON, including environment IDs, repository paths, owner/server/container state, runtime, and errors. With a selector, return one JSON object.",
        ),
        Some(T3Action::Stop) => (
            "stop",
            "Choose working T3 environments to stop, preserving mounted data for a later start. A selector stops one environment directly.",
        ),
        Some(T3Action::Upgrade) => (
            "upgrade",
            "Stop active jobs and recreate selected registered environments using the current image/runtime, preserving mounted data. Stopped/unavailable instances with valid registrations can be selected for recovery. A selector upgrades one environment directly.",
        ),
        _ => return T3_HELP_TEXT.to_owned(),
    };
    let bulk = if action == Some(T3Action::Status) {
        ""
    } else {
        "  --all                    Act on all eligible environments without prompting\n\nWithout a selector, use unchecked checkboxes (Space to toggle, Enter to apply).\nEscape/Ctrl-C or an empty selection changes nothing. Without a terminal,\nsupply a selector or --all. Upgrade includes stopped registrations and starts them;\nrunning selections will be interrupted. Stop includes only working instances.\n"
    };
    let selectors = if action == Some(T3Action::Status) {
        "environment | --repository <checkout>"
    } else {
        "environment | --repository <checkout> | --all"
    };
    format!(
        "Usage: ags t3 {command} [{selectors}]\n\n{description}\n\nFlags:\n  environment              Select an environment ID (SSH alias) from status\n  --repository <checkout>  Select one main checkout or worktree explicitly\n  -h, --help               Show help for this command\n{bulk}"
    )
}
