use std::fmt;
use std::io::{self, Write};

use crate::cli::{CompletionsOptions, Shell};

#[derive(Debug)]
pub enum CompletionsError {
    Io(io::Error),
}

impl fmt::Display for CompletionsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "I/O error: {e}"),
        }
    }
}

impl std::error::Error for CompletionsError {}

impl From<io::Error> for CompletionsError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

pub fn run(opts: &CompletionsOptions) -> Result<(), CompletionsError> {
    let script = render(opts.shell);
    let mut stdout = io::stdout().lock();
    stdout.write_all(script.as_bytes())?;
    Ok(())
}

fn render(shell: Shell) -> &'static str {
    match shell {
        Shell::Bash => BASH,
        Shell::Zsh => ZSH,
        Shell::Fish => FISH,
    }
}

const BASH: &str = r#"_ags_completion() {
  local cur prev
  cur="${COMP_WORDS[COMP_CWORD]}"
  prev=""
  if (( COMP_CWORD > 0 )); then
    prev="${COMP_WORDS[COMP_CWORD-1]}"
  fi

  local commands="setup doctor update update-agents install uninstall create-aliases completions"
  local agents="pi claude codex gemini opencode shell"
  local shells="fish zsh bash"
  local modes="wrappers aliases both"

  local word
  for word in "${COMP_WORDS[@]}"; do
    if [[ "$word" == "--" ]]; then
      return 0
    fi
  done

  if (( COMP_CWORD == 1 )); then
    COMPREPLY=( $(compgen -W "$commands --agent --browser --tmux --psp --psp-keep --config --add-dir -d -h --help" -- "$cur") )
    return 0
  fi

  case "${COMP_WORDS[1]}" in
    setup|doctor|update|update-agents|uninstall)
      COMPREPLY=( $(compgen -W "-h --help" -- "$cur") )
      return 0
      ;;
    install)
      COMPREPLY=( $(compgen -W "--link-self --force --add-agent-mounts -h --help" -- "$cur") )
      return 0
      ;;
    create-aliases)
      case "$prev" in
        --shell)
          COMPREPLY=( $(compgen -W "$shells" -- "$cur") )
          return 0
          ;;
        --mode)
          COMPREPLY=( $(compgen -W "$modes" -- "$cur") )
          return 0
          ;;
      esac

      if [[ "$cur" == --shell=* ]]; then
        local value="${cur#--shell=}"
        COMPREPLY=( $(compgen -W "$shells" -- "$value") )
        COMPREPLY=( "${COMPREPLY[@]/#/--shell=}" )
        return 0
      fi

      if [[ "$cur" == --mode=* ]]; then
        local value="${cur#--mode=}"
        COMPREPLY=( $(compgen -W "$modes" -- "$value") )
        COMPREPLY=( "${COMPREPLY[@]/#/--mode=}" )
        return 0
      fi

      COMPREPLY=( $(compgen -W "--shell --mode --force -h --help" -- "$cur") )
      return 0
      ;;
    completions)
      case "$prev" in
        --shell)
          COMPREPLY=( $(compgen -W "$shells" -- "$cur") )
          return 0
          ;;
      esac

      if [[ "$cur" == --shell=* ]]; then
        local value="${cur#--shell=}"
        COMPREPLY=( $(compgen -W "$shells" -- "$value") )
        COMPREPLY=( "${COMPREPLY[@]/#/--shell=}" )
        return 0
      fi

      COMPREPLY=( $(compgen -W "--shell -h --help" -- "$cur") )
      return 0
      ;;
  esac

  case "$prev" in
    --agent)
      COMPREPLY=( $(compgen -W "$agents" -- "$cur") )
      return 0
      ;;
    --config|--add-dir|-d)
      COMPREPLY=( $(compgen -d -- "$cur") )
      return 0
      ;;
  esac

  if [[ "$cur" == --agent=* ]]; then
    local value="${cur#--agent=}"
    COMPREPLY=( $(compgen -W "$agents" -- "$value") )
    COMPREPLY=( "${COMPREPLY[@]/#/--agent=}" )
    return 0
  fi

  if [[ "$cur" == --config=* ]]; then
    local value="${cur#--config=}"
    COMPREPLY=( $(compgen -f -- "$value") )
    COMPREPLY=( "${COMPREPLY[@]/#/--config=}" )
    return 0
  fi

  if [[ "$cur" == --add-dir=* ]]; then
    local value="${cur#--add-dir=}"
    COMPREPLY=( $(compgen -d -- "$value") )
    COMPREPLY=( "${COMPREPLY[@]/#/--add-dir=}" )
    return 0
  fi

  COMPREPLY=( $(compgen -W "--agent --browser --tmux --psp --config --add-dir -d -h --help" -- "$cur") )
}

complete -F _ags_completion ags
"#;

const ZSH: &str = r#"#compdef ags

local -a commands agents shells modes
commands=(setup doctor update update-agents install uninstall create-aliases completions)
agents=(pi claude codex gemini opencode shell)
shells=(fish zsh bash)
modes=(wrappers aliases both)

if (( CURRENT == 2 )); then
  _alternative \
    'subcommand:subcommand:(setup doctor update update-agents install uninstall create-aliases completions)' \
    'run-flag:run flag:(--agent --browser --tmux --psp --psp-keep --config --add-dir -d -h --help)'
  return
fi

case "$words[2]" in
  setup|doctor|update|update-agents|uninstall)
    _values 'option' -h --help
    return
    ;;
  install)
    _arguments \
      '--link-self[Link current ags executable to ~/.local/bin/ags]' \
      '--force[Replace existing ~/.local/bin/ags when used with --link-self]' \
      '--add-agent-mounts[Append default [[agent_mount]] entries to ~/.config/ags/config.toml]' \
      '(-h --help)'{-h,--help}'[Show help]'
    return
    ;;
  create-aliases)
    _arguments \
      '--shell[Target shell]:shell:(fish zsh bash)' \
      '--mode[Alias generation mode]:mode:(wrappers aliases both)' \
      '--force[Replace existing non-managed targets]' \
      '(-h --help)'{-h,--help}'[Show help]'
    return
    ;;
  completions)
    _arguments \
      '--shell[Shell to generate completion script for]:shell:(fish zsh bash)' \
      '(-h --help)'{-h,--help}'[Show help]'
    return
    ;;
esac

_arguments -S \
  '--agent[Agent to run]:agent:(pi claude codex gemini opencode shell)' \
  '--browser[Enable browser sidecar]' \
  '--tmux[Launch the agent inside a tmux session]' \
  '--psp[Enable podman-socket-proxy mode]' \
  '--psp-keep[Keep PSP-managed containers on exit (debug)]' \
  '--config[Override config file path]:config file:_files' \
  '(-d)--add-dir[Add an extra same-path directory mount for this run]:host directory:_files -/' \
  '(--add-dir)-d[Add an extra same-path directory mount for this run]:host directory:_files -/' \
  '(-h --help)'{-h,--help}'[Show help]'
"#;

const FISH: &str = r#"complete -c ags -f

set -l __ags_subcommands setup doctor update update-agents install uninstall create-aliases completions
set -l __ags_agents pi claude codex gemini opencode shell
set -l __ags_shells fish zsh bash
set -l __ags_modes wrappers aliases both

# Top-level: subcommands + run-mode flags.
complete -c ags -n "__fish_use_subcommand" -a setup -d "Generate SSH keys and configure secrets"
complete -c ags -n "__fish_use_subcommand" -a doctor -d "Run environment and config health checks"
complete -c ags -n "__fish_use_subcommand" -a update -d "Rebuild sandbox image"
complete -c ags -n "__fish_use_subcommand" -a update-agents -d "Install/update agent CLIs"
complete -c ags -n "__fish_use_subcommand" -a install -d "Install assets/config layout"
complete -c ags -n "__fish_use_subcommand" -a uninstall -d "Reserved no-op"
complete -c ags -n "__fish_use_subcommand" -a create-aliases -d "Create wrappers and/or aliases"
complete -c ags -n "__fish_use_subcommand" -a completions -d "Print completion script"

complete -c ags -n "__fish_use_subcommand" -l agent -r -a "$__ags_agents" -d "Agent to run"
complete -c ags -n "__fish_use_subcommand" -l browser -d "Enable browser sidecar"
complete -c ags -n "__fish_use_subcommand" -l tmux -d "Launch the agent inside a tmux session"
complete -c ags -n "__fish_use_subcommand" -l psp -d "Enable podman-socket-proxy mode"
complete -c ags -n "__fish_use_subcommand" -l psp-keep -d "Keep PSP-managed containers on exit (debug)"
complete -c ags -n "__fish_use_subcommand" -l config -r -d "Override config file path"
complete -c ags -n "__fish_use_subcommand" -l add-dir -s d -r -d "Add an extra same-path directory mount for this run"
complete -c ags -n "__fish_use_subcommand" -s h -l help -d "Show help"

# install
complete -c ags -n "__fish_seen_subcommand_from install" -l link-self -d "Link ags to ~/.local/bin/ags"
complete -c ags -n "__fish_seen_subcommand_from install" -l force -d "Replace existing target"
complete -c ags -n "__fish_seen_subcommand_from install" -l add-agent-mounts -d "Append default [[agent_mount]] entries"
complete -c ags -n "__fish_seen_subcommand_from install" -s h -l help -d "Show help"

# create-aliases
complete -c ags -n "__fish_seen_subcommand_from create-aliases" -l shell -r -a "$__ags_shells" -d "Target shell"
complete -c ags -n "__fish_seen_subcommand_from create-aliases" -l mode -r -a "$__ags_modes" -d "wrappers|aliases|both"
complete -c ags -n "__fish_seen_subcommand_from create-aliases" -l force -d "Replace existing non-managed targets"
complete -c ags -n "__fish_seen_subcommand_from create-aliases" -s h -l help -d "Show help"

# completions
complete -c ags -n "__fish_seen_subcommand_from completions" -l shell -r -a "$__ags_shells" -d "Shell to generate for"
complete -c ags -n "__fish_seen_subcommand_from completions" -s h -l help -d "Show help"

# simple subcommands
for __ags_cmd in setup doctor update update-agents uninstall
  complete -c ags -n "__fish_seen_subcommand_from $__ags_cmd" -s h -l help -d "Show help"
end
"#;

#[cfg(test)]
mod tests {
    use super::render;
    use crate::cli::Shell;

    #[test]
    fn bash_completion_contains_core_flags() {
        let script = render(Shell::Bash);
        assert!(script.contains("--agent"));
        assert!(script.contains("--browser"));
        assert!(script.contains("--tmux"));
        assert!(script.contains("create-aliases"));
        assert!(script.contains("completions"));
        assert!(script.contains("--add-agent-mounts"));
        assert!(script.contains("--add-dir"));
        assert!(script.contains("-d"));
    }

    #[test]
    fn zsh_completion_contains_compdef() {
        let script = render(Shell::Zsh);
        assert!(script.starts_with("#compdef ags"));
        assert!(script.contains("update-agents"));
    }

    #[test]
    fn fish_completion_contains_subcommands() {
        let script = render(Shell::Fish);
        assert!(script.contains("complete -c ags"));
        assert!(script.contains("-a completions"));
    }
}
