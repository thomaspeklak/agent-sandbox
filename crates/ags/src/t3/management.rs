use super::{
    inventory::{self, Entry},
    owner,
    registration::Registration,
    repository, selection,
};
use crate::cli::{T3Action, T3Options};
use std::io;
use std::path::PathBuf;

pub fn command(options: &T3Options) -> io::Result<()> {
    if let Some(path) = target(options)? {
        let entry = Entry::read(&path);
        if options.action == T3Action::Status {
            println!("{}", serde_json::to_string_pretty(&entry.status)?);
            return status_result(&[entry]);
        }
        let registration = entry.registration.as_ref().ok_or_else(|| {
            io::Error::other(
                entry.status["error"]
                    .as_str()
                    .unwrap_or("invalid T3 registration"),
            )
        })?;
        perform(registration, options.action)?;
        println!(
            "{}: {} succeeded; mounted data was preserved.",
            registration.alias(),
            verb(options.action)
        );
        return Ok(());
    }
    let entries = inventory::load()?;
    if options.action == T3Action::Status {
        println!(
            "{}",
            serde_json::to_string_pretty(
                &entries
                    .iter()
                    .map(|entry| &entry.status)
                    .collect::<Vec<_>>()
            )?
        );
        return status_result(&entries);
    }
    for entry in &entries {
        if let Some(error) = entry.status["error"].as_str() {
            eprintln!(
                "{} {}: {error}",
                if eligible(entry, options.action) {
                    "Status"
                } else {
                    "Skipping"
                },
                entry.status["environment"]
                    .as_str()
                    .unwrap_or("unknown environment")
            );
        }
    }
    let candidates: Vec<_> = entries
        .iter()
        .filter(|entry| eligible(entry, options.action))
        .collect();
    if candidates.is_empty() {
        println!(
            "No {} T3 environments to {}.",
            if options.action == T3Action::Upgrade {
                "valid registered"
            } else {
                "working"
            },
            verb(options.action)
        );
        return Ok(());
    }
    let selected = if options.all {
        (0..candidates.len()).collect()
    } else {
        selection::choose(&candidates, verb(options.action))?.unwrap_or_default()
    };
    if selected.is_empty() {
        println!("No T3 environments selected; nothing changed.");
        return Ok(());
    }
    let mut succeeded = 0;
    let mut failed = 0;
    for index in selected {
        let previous = candidates[index];
        let registration = previous.registration.as_ref().unwrap();
        // A stopped selection explicitly opts into startup. Do not interrupt
        // newly started work or reverse a later stop using a stale checklist.
        let result = registration
            .path()
            .map(|path| Entry::read(&path))
            .and_then(|entry| {
                if previous.working() && !entry.working() {
                    return Err(io::Error::other(
                        "environment is no longer working; inspect `ags t3 status`",
                    ));
                }
                if !eligible(&entry, options.action) || entry.status["state"] != previous.status["state"] {
                    return Err(io::Error::other(format!(
                        "environment state changed since selection; reopen the checklist with `ags t3 {}` or inspect `ags t3 status`",
                        verb(options.action),
                    )));
                }
                perform(entry.registration.as_ref().unwrap(), options.action)
            });
        match result {
            Ok(()) => {
                succeeded += 1;
                println!(
                    "OK {}: {} succeeded; mounted data was preserved.",
                    registration.alias(),
                    verb(options.action)
                );
            }
            Err(error) => {
                failed += 1;
                eprintln!("FAILED {}: {error}", registration.alias());
            }
        }
    }
    println!(
        "T3 {}: {succeeded} succeeded, {failed} failed.",
        verb(options.action)
    );
    if failed > 0 {
        return Err(io::Error::other(format!(
            "{failed} T3 environment action(s) failed"
        )));
    }
    Ok(())
}

fn eligible(entry: &Entry, action: T3Action) -> bool {
    match action {
        // A valid registration can be explicitly recreated even when its
        // current server/container is stopped or its status needs recovery.
        T3Action::Upgrade => entry.registration.is_some(),
        T3Action::Stop => entry.working(),
        _ => false,
    }
}

fn target(options: &T3Options) -> io::Result<Option<PathBuf>> {
    if let Some(checkout) = &options.repository {
        let repository = repository::Repository::resolve(checkout)?;
        return Ok(Some(
            inventory::registry()?.join(format!("{}.json", repository.id)),
        ));
    }
    if let Some(environment) = &options.environment {
        let paths: Vec<_> = inventory::paths()?
            .into_iter()
            .filter(|path| inventory::matches(path, environment))
            .collect();
        return match paths.as_slice() {
            [path] => Ok(Some(path.clone())),
            [] => Err(io::Error::other(format!(
                "unknown T3 environment '{environment}'; choose an identifier from `ags t3 status`"
            ))),
            _ => Err(io::Error::other(
                "ambiguous T3 environment alias; use the full repository_id from `ags t3 status`",
            )),
        };
    }
    Ok(None)
}

fn status_result(entries: &[Entry]) -> io::Result<()> {
    let errors = entries
        .iter()
        .filter(|entry| entry.status.get("error").is_some())
        .count();
    if errors > 0 {
        Err(io::Error::other(format!(
            "{errors} T3 environment(s) unavailable; see status errors"
        )))
    } else {
        Ok(())
    }
}

fn perform(registration: &Registration, action: T3Action) -> io::Result<()> {
    match action {
        T3Action::Stop => owner::stop(registration),
        T3Action::Upgrade => owner::upgrade(registration),
        _ => Err(io::Error::other("invalid T3 management action")),
    }
}

fn verb(action: T3Action) -> &'static str {
    match action {
        T3Action::Upgrade => "upgrade",
        _ => "stop",
    }
}
