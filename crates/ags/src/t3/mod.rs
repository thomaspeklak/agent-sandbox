//! Persistent per-repository T3 environments and their host-side SSH transport.
pub mod compatibility;
mod container;
mod credentials;
mod dispatch;
mod environment;
mod environment_plan;
mod identity;
mod owner;
mod owner_process;
mod providers;
mod registration;
pub mod repository;
mod sidecars;
mod ssh_agent;
mod transport;
mod transport_io;

pub(crate) use environment::runtime_version as installed_version;

use crate::cli::{RunOptions, T3Action, T3Options};
use registration::Registration;
use std::io;

pub fn run(options: &RunOptions) -> io::Result<()> {
    let registration = registration::register(options)?;
    identity::configure(&registration)?;
    owner::ensure(&registration, &options.env)?;
    owner::request(&registration, "start")?;
    println!(
        "T3 repository environment ready: {}\nAdd SSH connection '{}' in the T3 desktop.\nDiagnostics: {}\nStop with `ags t3 stop`.",
        registration.repository.main.display(),
        registration.alias(),
        registration.private_dir()?.join("owner.log").display()
    );
    Ok(())
}

pub fn command(options: &T3Options) -> io::Result<()> {
    let path = if let Some(path) = &options.registration {
        path.clone()
    } else {
        let repository = repository::Repository::resolve(
            &options
                .repository
                .clone()
                .unwrap_or(std::env::current_dir()?),
        )?;
        registration::root()?
            .join("registry")
            .join(format!("{}.json", repository.id))
    };
    let registration = Registration::read(&path).map_err(|error| {
        io::Error::other(format!(
            "cannot load T3 environment ({error}); register it with `ags --agent t3`"
        ))
    })?;
    match options.action {
        T3Action::Owner => owner::run(registration),
        T3Action::Proxy => owner::proxy(&registration),
        T3Action::Status => {
            match owner::request(&registration, "status") {
                Ok(status) => print!("{status}"),
                Err(_) => println!(
                    "{}",
                    serde_json::json!({"repository": registration.repository.main,
                    "ssh_alias": registration.alias(), "owner_running": false,
                    "runtime": environment::specification(&registration)?.map(|spec| spec.version)})
                ),
            }
            Ok(())
        }
        T3Action::Stop => {
            if owner::request(&registration, "stop").is_err() {
                container::stop(&registration)?;
            }
            println!("T3 repository environment stopped.");
            Ok(())
        }
        T3Action::Upgrade => {
            owner::ensure(&registration, &[])?;
            owner::request(&registration, "upgrade")?;
            println!("T3 repository environment upgraded; mounted data was preserved.");
            Ok(())
        }
    }
}

#[cfg(test)]
#[path = "compatibility_tests.rs"]
mod compatibility_tests;
#[cfg(test)]
#[path = "storage_tests.rs"]
mod storage_tests;
#[cfg(test)]
#[path = "transport_tests.rs"]
mod transport_tests;
