//! Persistent per-repository T3 environments and their host-side SSH transport.
pub mod compatibility;
mod container;
mod credentials;
mod dispatch;
mod environment;
mod environment_plan;
mod identity;
mod inventory;
mod management;
mod owner;
mod owner_process;
mod providers;
mod registration;
pub mod repository;
mod selection;
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
        "T3 repository environment ready: {}\nAdd SSH connection '{}' in the T3 desktop.\nSSH transport: local ProxyCommand; no listening SSH port (leave port at default 22).\nSSH config: {}\nDiagnostics: {}\nStatus with `ags t3 status`; stop with `ags t3 stop`.",
        registration.repository.main.display(),
        registration.alias(),
        registration
            .data_root
            .join("ssh")
            .join(format!("{}.conf", registration.repository.id))
            .display(),
        registration.private_dir()?.join("owner.log").display()
    );
    Ok(())
}

pub fn command(options: &T3Options) -> io::Result<()> {
    if !matches!(options.action, T3Action::Owner | T3Action::Proxy) {
        return management::command(options);
    }
    let path = options
        .registration
        .as_ref()
        .ok_or_else(|| io::Error::other("missing T3 owner registration"))?;
    let registration = Registration::read(path).map_err(|error| {
        io::Error::other(format!(
            "cannot load T3 environment ({error}); register it with `ags --agent t3`"
        ))
    })?;
    match options.action {
        T3Action::Owner => owner::run(registration),
        T3Action::Proxy => owner::proxy(&registration),
        _ => unreachable!("public actions are dispatched to management"),
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
