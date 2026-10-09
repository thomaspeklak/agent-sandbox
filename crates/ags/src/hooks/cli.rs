//! Discoverability commands never need Podman or credentials. `test` runs HOST CODE.
use super::protocol::{INPUT_SCHEMA, OUTPUT_SCHEMA};
use super::{Context, Contributions, TrustStore};
use crate::cli::{Agent, CliError};
use std::io::Read;
use std::path::PathBuf;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Describe,
    SchemaInput,
    SchemaOutput,
    Validate(PathBuf),
    Test(String),
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Options {
    pub action: Action,
    pub config_path: Option<PathBuf>,
    pub context_path: Option<PathBuf>,
    pub workdir: Option<PathBuf>,
    pub agent: Agent,
}
pub fn parse_args(mut iter: impl Iterator<Item = String>) -> Result<Options, CliError> {
    let mut positional = Vec::new();
    let mut config_path = None;
    let mut context_path = None;
    let mut workdir = None;
    let mut agent = Agent::Shell;
    let mut agent_supplied = false;
    while let Some(arg) = iter.next() {
        if arg == "--help" || arg == "-h" {
            return Err(CliError::HelpRequested);
        }
        let (flag, inline) = arg
            .split_once('=')
            .map_or((arg.as_str(), None), |(k, v)| (k, Some(v)));
        if matches!(flag, "--config" | "--context" | "--workdir" | "--agent") {
            let value = inline
                .map(str::to_owned)
                .or_else(|| iter.next())
                .filter(|s| !s.is_empty())
                .ok_or_else(|| CliError::UnexpectedFlag(format!("{flag} requires a value")))?;
            match flag {
                "--config" => config_path = Some(value.into()),
                "--context" => context_path = Some(value.into()),
                "--workdir" => workdir = Some(value.into()),
                "--agent" => {
                    agent = Agent::parse(&value)?;
                    agent_supplied = true;
                }
                _ => unreachable!(),
            }
        } else if arg.starts_with('-') && arg != "-" {
            return Err(CliError::UnexpectedFlag(arg));
        } else {
            positional.push(arg);
        }
    }
    let action = match positional.iter().map(String::as_str).collect::<Vec<_>>().as_slice() {
        ["describe", "prepare"] => Action::Describe,
        ["schema", "prepare", "input"] => Action::SchemaInput,
        ["schema", "prepare", "output"] => Action::SchemaOutput,
        ["validate", path] => Action::Validate(PathBuf::from(path)),
        ["test", name] => Action::Test((*name).to_owned()),
        _ => return Err(CliError::UnexpectedPositional("expected hooks describe prepare | schema prepare input/output | validate FILE/- | test NAME".into())),
    };
    if !matches!(action, Action::Test(_))
        && (config_path.is_some() || context_path.is_some() || workdir.is_some() || agent_supplied)
    {
        return Err(CliError::UnexpectedFlag(
            "context/config/workdir/agent options are only supported with hooks test".into(),
        ));
    }
    if context_path.is_some() && (workdir.is_some() || agent_supplied) {
        return Err(CliError::UnexpectedFlag(
            "--context cannot be combined with --workdir/--agent".into(),
        ));
    }
    Ok(Options {
        action,
        config_path,
        context_path,
        workdir,
        agent,
    })
}
pub fn run(opts: &Options) -> Result<(), String> {
    match &opts.action {
        Action::Describe => println!("{}", describe()),
        Action::SchemaInput => print!("{INPUT_SCHEMA}"),
        Action::SchemaOutput => print!("{OUTPUT_SCHEMA}"),
        Action::Validate(path) => {
            let response = super::validate_response(&read(path, 1024 * 1024)?)?;
            let contributions = Contributions::merge(vec![response])?;
            println!("{}", contributions.redact_summary());
        }
        Action::Test(name) => test(opts, name)?,
    }
    Ok(())
}
pub fn describe() -> &'static str {
    "prepare v1 runs before container creation. Host executables receive the same versioned JSON stdin context and return strict JSON stdout; stderr is diagnostic. Contributions: env literals/op refs, generated files, bind mounts. Global then project declarations accumulate; defaults < global < project < CLI, later declarations win, overlapping destinations fail. Default concurrency 4, per-hook 30s, overall 60s, stdout 1MiB, stderr 64KiB; no retries. Approval binds executable bytes and declaration/argv and grants HOST USER execution. Libraries, imports, interpreters and scripts passed as args are NOT tracked; declare the directly executable script to track its content. `ags hooks test NAME [--config PATH] [--agent shell] [--workdir PATH | --context FILE]` EXECUTES HOST CODE, is not a side-effect-free dry run, and returns a redacted summary without resolving op references. `ags hooks validate FILE/-` validates a response without executing. started/finished are future events; no interactive hook protocol. See docs/STARTUP_HOOKS.md."
}
fn read(path: &std::path::Path, limit: usize) -> Result<Vec<u8>, String> {
    let mut source: Box<dyn Read> = if path == std::path::Path::new("-") {
        Box::new(std::io::stdin())
    } else {
        Box::new(std::fs::File::open(path).map_err(|e| e.to_string())?)
    };
    let mut bytes = Vec::new();
    source
        .by_ref()
        .take((limit + 1) as u64)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() > limit {
        return Err("input exceeds byte limit".into());
    }
    Ok(bytes)
}
fn test(opts: &Options, name: &str) -> Result<(), String> {
    let mut config = crate::lifecycle::load_config(opts.config_path.as_deref())
        .map_err(|_| "could not load hook configuration")?;
    let matching: Vec<_> = config
        .prepare_hooks
        .iter()
        .filter(|h| h.name == name)
        .cloned()
        .collect();
    if matching.len() != 1 {
        return Err(format!(
            "expected one configured hook named {name:?}; found {}; use unique global/project names",
            matching.len()
        ));
    }
    let context = if let Some(path) = &opts.context_path {
        super::strict_json::parse::<Context>(&read(path, 64 * 1024)?)?
    } else {
        let workdir = opts
            .workdir
            .clone()
            .unwrap_or(std::env::current_dir().map_err(|e| e.to_string())?);
        Context::new(&workdir, opts.agent)?
    };
    context.validate()?;
    let store = TrustStore::default();
    super::check_store_location(&store, &context)?;
    if let Some(project) = &matching[0].project {
        super::check_store_outside_project(&store, project)?;
    }
    eprintln!(
        "WARNING: hooks test executes host code and can create external side effects; it is NOT a side-effect-free dry run."
    );
    store.ensure(&matching[0], true)?;
    let _signals = crate::host_process::SignalGuard::install()?;
    let responses = super::run(&matching, &context, &store, super::Limits::default())?;
    let contributions = Contributions::merge(responses)?;
    let summary = contributions.redact_summary();
    let run_opts =
        match crate::cli::parse_args(["ags".into(), "--agent".into(), context.agent.to_string()])
            .map_err(|e| e.to_string())?
        {
            crate::cli::Command::Run(opts) => opts,
            _ => unreachable!(),
        };
    let _materialized = contributions.materialize(&mut config, &run_opts)?;
    println!("{summary}");
    Ok(())
}
