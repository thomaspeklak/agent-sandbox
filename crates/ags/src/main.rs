use std::process::ExitCode;

use ags::cli::{self, Command, SubCommand};
use ags::config::ValidatedConfig;

fn main() -> ExitCode {
    let update_check = ags::update_check::UpdateCheck::from_default_cache();

    let code = match cli::parse_args(std::env::args()) {
        Ok(Command::Run(opts)) => ags::lifecycle::run_agent(opts),
        Ok(Command::Sub(sub)) => {
            let skip_notice = matches!(
                sub,
                SubCommand::Completions(_)
                    | SubCommand::UpdateImage(_)
                    | SubCommand::UpdateDeprecated(_)
            );
            let code = run_subcommand(sub);
            if skip_notice {
                return code;
            }
            code
        }
        Err(cli::CliError::HelpRequested) => {
            println!("{}", cli::help_text());
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("error: {error}");
            eprintln!("\n{}", cli::help_text());
            ExitCode::from(2)
        }
    };

    update_check.notify_if_available();
    code
}

/// Run a fallible subcommand, printing `"{label} error: …"` on failure.
fn try_sub(label: &str, result: Result<(), impl std::fmt::Display>) -> ExitCode {
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{label} error: {e}");
            ExitCode::FAILURE
        }
    }
}

fn run_update_image(config: &ValidatedConfig, opts: ags::cli::UpdateImageOptions) -> ExitCode {
    if let Err(e) = ags::assets::ensure_image_build_context(&config.sandbox.containerfile) {
        eprintln!("update-image error: could not prepare image build context: {e}");
        return ExitCode::FAILURE;
    }
    try_sub(
        "update-image",
        ags::cmd::update::run(
            config,
            &ags::cmd::update::UpdateOptions {
                keep_existing: opts.keep_existing,
                ..Default::default()
            },
        ),
    )
}

fn run_subcommand(sub: SubCommand) -> ExitCode {
    // Subcommands that don't need a config file.
    match sub {
        SubCommand::Install(ref opts) => return try_sub("install", ags::cmd::install::run(opts)),
        SubCommand::Uninstall => return try_sub("uninstall", ags::cmd::install::uninstall()),
        SubCommand::CreateAliases(ref opts) => {
            return try_sub("create-aliases", ags::cmd::create_aliases::run(opts));
        }
        SubCommand::Completions(ref opts) => {
            return try_sub("completions", ags::cmd::completions::run(opts));
        }
        SubCommand::Config => {
            let config_path = ags::config::default_config_path();
            return try_sub("config", ags::cmd::config_editor::run(&config_path));
        }
        SubCommand::Setup
        | SubCommand::Doctor
        | SubCommand::UpdateImage(_)
        | SubCommand::UpdateDeprecated(_)
        | SubCommand::UpdateAgents => {}
    }

    let config = match ags::lifecycle::load_config(None) {
        Ok(c) => c,
        Err(code) => return code,
    };

    match sub {
        SubCommand::Setup => try_sub("setup", ags::cmd::setup::run(&config)),
        SubCommand::Doctor => {
            if ags::cmd::doctor::run(&config) {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        SubCommand::UpdateImage(opts) => run_update_image(&config, opts),
        SubCommand::UpdateDeprecated(opts) => {
            eprintln!("warning: `ags update` is deprecated; use `ags update-image` instead.");
            run_update_image(&config, opts)
        }
        SubCommand::UpdateAgents => try_sub(
            "update-agents",
            ags::cmd::update_agents::run(
                &config,
                &ags::cmd::update_agents::UpdateAgentsOptions::default(),
            ),
        ),
        SubCommand::Install(_)
        | SubCommand::Uninstall
        | SubCommand::CreateAliases(_)
        | SubCommand::Completions(_)
        | SubCommand::Config => {
            unreachable!()
        }
    }
}
