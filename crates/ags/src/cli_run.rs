use std::path::PathBuf;

use super::{Agent, CliError, Command, RunOptions};
use crate::run_defaults;

pub(super) fn parse_args<I: Iterator<Item = String>>(
    first: String,
    mut iter: I,
) -> Result<Command, CliError> {
    let mut state = RunParseState::default();
    let mut passthrough_args = Vec::new();
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
    if state.use_defaults {
        run_defaults::prepend_passthrough_args(agent, &mut passthrough_args);
    }

    if !state.tty && state.tmux {
        return Err(CliError::InvalidRunOption(
            "--tty=false cannot be combined with --tmux".to_owned(),
        ));
    }

    Ok(Command::Run(RunOptions {
        agent,
        tty: state.tty,
        container_name: state.container_name,
        timeout_seconds: state.timeout_seconds,
        repo_config: state.repo_config,
        browser: state.browser,
        tmux: state.tmux,
        psp: state.psp,
        psp_keep: state.psp_keep,
        yolo: state.yolo,
        root: state.root,
        lockdown: state.lockdown,
        wayland_compositor_passthrough: state.wayland_compositor_passthrough,
        stop_when_done: state.stop_when_done,
        config_path: state.config_path,
        add_dirs: state.add_dirs,
        env: state.env,
        op_secret_sets: state.op_secret_sets,
        passthrough_args,
    }))
}

struct RunParseState {
    agent: Option<Agent>,
    tty: bool,
    container_name: Option<String>,
    timeout_seconds: Option<u32>,
    repo_config: bool,
    browser: bool,
    tmux: bool,
    psp: bool,
    psp_keep: bool,
    yolo: bool,
    root: bool,
    lockdown: bool,
    wayland_compositor_passthrough: bool,
    stop_when_done: bool,
    use_defaults: bool,
    config_path: Option<PathBuf>,
    add_dirs: Vec<PathBuf>,
    env: Vec<(String, String)>,
    op_secret_sets: Vec<String>,
}

impl Default for RunParseState {
    fn default() -> Self {
        Self {
            agent: None,
            tty: true,
            container_name: None,
            timeout_seconds: None,
            repo_config: true,
            browser: false,
            tmux: false,
            psp: false,
            psp_keep: false,
            yolo: false,
            root: false,
            lockdown: false,
            wayland_compositor_passthrough: false,
            stop_when_done: false,
            use_defaults: false,
            config_path: None,
            add_dirs: Vec::new(),
            env: Vec::new(),
            op_secret_sets: Vec::new(),
        }
    }
}

fn run_option_value<I: Iterator<Item = String>>(
    arg: &str,
    flag: &str,
    iter: &mut I,
) -> Result<String, CliError> {
    arg.strip_prefix(&format!("{flag}="))
        .map(str::to_owned)
        .or_else(|| iter.next())
        .filter(|value| !value.is_empty())
        .ok_or_else(|| CliError::InvalidRunOption(format!("missing value for {flag}")))
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

    if arg == "--tty" || arg.starts_with("--tty=") {
        let value = run_option_value(arg, "--tty", iter)?;
        state.tty = match value.as_str() {
            "true" => true,
            "false" => false,
            _ => {
                return Err(CliError::InvalidRunOption(
                    "--tty expects true or false".to_owned(),
                ));
            }
        };
        return Ok(());
    }

    if arg == "--no-repo-config" {
        state.repo_config = false;
        return Ok(());
    }

    if arg == "--container-name" || arg.starts_with("--container-name=") {
        let name = run_option_value(arg, "--container-name", iter)?;
        if name.len() > 80
            || !name
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
            || !name.as_bytes()[0].is_ascii_alphanumeric()
        {
            return Err(CliError::InvalidRunOption(
                "container name must be 1–80 ASCII letters, digits, hyphens or underscores, starting with a letter or digit".to_owned(),
            ));
        }
        state.container_name = Some(name);
        return Ok(());
    }

    if arg == "--timeout-seconds" || arg.starts_with("--timeout-seconds=") {
        let value = run_option_value(arg, "--timeout-seconds", iter)?;
        let seconds = value
            .parse::<u32>()
            .ok()
            .filter(|seconds| (1..=86_400).contains(seconds))
            .ok_or_else(|| {
                CliError::InvalidRunOption("timeout must be 1–86400 seconds".to_owned())
            })?;
        state.timeout_seconds = Some(seconds);
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

    if arg == "--lockdown" {
        state.lockdown = true;
        return Ok(());
    }

    if arg == "--wayland-compositor-passthrough" {
        state.wayland_compositor_passthrough = true;
        return Ok(());
    }

    if arg == "--stop-when-done" {
        state.stop_when_done = true;
        return Ok(());
    }

    if arg == "--defaults" || arg == "-D" {
        state.use_defaults = true;
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

    if arg == "--env" {
        let raw = iter.next().ok_or(CliError::MissingEnvValue)?;
        state.env.push(parse_env_assignment(&raw)?);
        return Ok(());
    }

    if let Some(raw) = arg.strip_prefix("--env=") {
        if raw.is_empty() {
            return Err(CliError::MissingEnvValue);
        }
        state.env.push(parse_env_assignment(raw)?);
        return Ok(());
    }

    if arg == "--op-secret-set" || arg == "-1" {
        let raw = iter.next().ok_or(CliError::MissingOpSecretSetValue)?;
        state.op_secret_sets.push(raw);
        return Ok(());
    }

    if let Some(raw) = arg.strip_prefix("--op-secret-set=") {
        if raw.is_empty() {
            return Err(CliError::MissingOpSecretSetValue);
        }
        state.op_secret_sets.push(raw.to_owned());
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

fn parse_env_assignment(raw: &str) -> Result<(String, String), CliError> {
    let (name, value) = raw
        .split_once('=')
        .ok_or_else(|| CliError::InvalidEnvAssignment(raw.to_owned()))?;
    let mut chars = name.chars();
    let valid_name = chars
        .next()
        .is_some_and(|first| first == '_' || first.is_ascii_alphabetic())
        && chars.all(|ch| ch == '_' || ch.is_ascii_alphanumeric());
    if !valid_name {
        return Err(CliError::InvalidEnvAssignment(raw.to_owned()));
    }
    if name.starts_with("AGS_") {
        return Err(CliError::ReservedEnvName(name.to_owned()));
    }
    Ok((name.to_owned(), value.to_owned()))
}
