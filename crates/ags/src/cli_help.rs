pub const HELP_TEXT: &str = "\
Usage: ags [command] --agent <pi|claude|codex|gemini|opencode|shell> [flags] -- [args...]

\
Commands:
\
  setup          Generate SSH keys and configure secrets
\
  doctor         Run health checks on sandbox configuration
\
  update-image   Rebuild container image and refresh bundled br/dcg
\
  update-agents  Reconcile selected agents in persistent volumes
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
Run flags:
\
  --agent <name>       Agent to run (required), or 'shell' for interactive bash
\
  --tty <true|false>   Allocate a terminal (default: true; false requires an existing image)
\
  --container-name <name>
\
                       Override the generated container name (1–80 ASCII characters)
\
  --timeout-seconds <seconds>
\
                       Limit container lifetime to 1–86400 seconds
\
  --no-repo-config    Skip repository-local config and trust lookup for this run
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
  --config <path> Config file to build from (default: ~/.config/ags/config.toml)

\
Update-agents flags:
\
  --config <path> Config file to reconcile from (default: ~/.config/ags/config.toml)

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
