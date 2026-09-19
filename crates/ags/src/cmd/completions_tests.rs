use super::render;
use crate::cli::Shell;

#[test]
fn bash_completion_contains_core_flags() {
    let script = render(Shell::Bash);
    assert!(script.contains("--agent"));
    assert!(script.contains("--browser"));
    assert!(script.contains("--tmux"));
    assert!(script.contains("--lockdown"));
    assert!(script.contains("--wayland-compositor-passthrough"));
    assert!(script.contains("--stop-when-done"));
    assert!(script.contains("--defaults"));
    assert!(script.contains("-D"));
    assert!(script.contains("create-aliases"));
    assert!(script.contains("completions"));
    assert!(script.contains("--add-agent-mounts"));
    assert!(script.contains("--keep-existing"));
    assert!(script.contains("--keep-existing --rebase --config -h --help"));
    assert!(script.contains("tools"));
    assert!(script.contains("COMPREPLY=( $(compgen -f -- \"$cur\") $(compgen -W \"--packages --config -h --help\" -- \"$cur\") )"));
    assert!(script.contains("--add-dir"));
    assert!(script.contains("-d"));
    assert!(script.contains("--env"));
    assert!(script.contains("--op-secret-set"));
    assert!(script.contains("-1"));
}

#[test]
fn bash_completion_handles_equals_form_file_options() {
    let script = render(Shell::Bash);

    assert!(script.contains("if [[ \"$cur\" == --config=* ]]; then"));
    assert!(script.contains("local value=\"${cur#--config=}\""));
    assert!(script.contains("COMPREPLY=( \"${COMPREPLY[@]/#/--config=}\" )"));
    assert!(script.contains("if [[ \"$cur\" == --packages=* || \"$cur\" == --config=* ]]; then"));
    assert!(script.contains("local prefix=\"${cur%%=*}=\""));
    assert!(script.contains("local value=\"${cur#*=}\""));
    assert!(script.contains("COMPREPLY=( \"${COMPREPLY[@]/#/$prefix}\" )"));
}

#[test]
fn zsh_completion_contains_compdef() {
    let script = render(Shell::Zsh);
    assert!(script.starts_with("#compdef ags"));
    assert!(script.contains("update-agents"));
    assert!(script.contains("--keep-existing[Keep the previous image after a successful rebuild]"));
    assert!(script.contains("--rebase[Refresh the Fedora base and restart OS update layers]"));
    assert!(script.contains("--packages[Tool catalog JSON file]"));
    assert!(script.contains("'1:catalog file:_files'"));
    assert!(script.contains("--psp[Enable podman-socket-proxy mode (policy-gated)]"));
    assert!(script.contains("--env[Set a container environment variable (repeatable)]"));
    assert!(script.contains("--op-secret-set[Inject fields from a 1Password Secure Note]"));
    assert!(script.contains("-1[Inject fields from a 1Password Secure Note]"));
    assert!(
        script.contains("--psp-keep[Keep PSP-managed containers on exit (debug; requires --psp)]")
    );
}

#[test]
fn fish_completion_contains_subcommands() {
    let script = render(Shell::Fish);
    assert!(script.contains("complete -c ags"));
    assert!(script.contains("-a completions"));
    assert!(
        script
            .contains("-l keep-existing -d \"Keep the previous image after a successful rebuild\"")
    );
    assert!(
        script.contains("-l rebase -d \"Refresh the Fedora base and restart OS update layers\"")
    );
    assert!(script.contains("-a tools"));
    assert!(script.contains(
        "complete -c ags -n \"__fish_seen_subcommand_from tools\" -F -d \"Tool catalog JSON file\""
    ));
    assert!(script.contains("-l packages -r"));
    assert!(script.contains("-l psp -d \"Enable podman-socket-proxy mode (policy-gated)\""));
    assert!(script.contains(
        "-l psp-keep -d \"Keep PSP-managed containers on exit (debug; requires --psp)\""
    ));
    assert!(script.contains("-l env -r"));
    assert!(script.contains("-l op-secret-set -s 1 -r"));
}
