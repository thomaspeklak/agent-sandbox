use super::registration::Registration;
use crate::config::ValidatedConfig;
use std::collections::HashMap;
use std::io;
use std::os::fd::OwnedFd;

pub fn prepare(registration: &Registration, config: &ValidatedConfig) -> io::Result<Vec<OwnedFd>> {
    let mut environment = crate::secrets::resolve_secrets_for_run(
        &config.secrets,
        &NoninteractiveBackend,
        &crate::secrets::OsHostCommandRunner,
        false,
    );
    for name in &config.sandbox.passthrough_env {
        if !environment.contains_key(name)
            && let Ok(value) = std::env::var(name)
        {
            environment.insert(name.clone(), value);
        }
    }
    environment.retain(|name, _| !name.starts_with("OP_"));
    for name in &registration.settings.env_names {
        match std::env::var(name) {
            Ok(value) => {
                environment.insert(name.clone(), value);
            }
            Err(_) if environment.contains_key(name) => {}
            Err(_) => {
                return Err(io::Error::other(format!(
                    "T3 cold start needs registered --env {name}; prepare its source interactively or configure a reacquirable secret"
                )));
            }
        }
    }
    let mut descriptors = vec![sealed_environment(&environment)?];
    let sources = registration
        .settings
        .op_sources
        .iter()
        .map(|source| crate::onepassword::SourceRef::parse(source))
        .collect::<Result<Vec<_>, _>>()
        .map_err(io::Error::other)?;
    descriptors.extend(
        crate::onepassword::prepare_noninteractive(&sources)
            .map_err(io::Error::other)?
            .into_iter()
            .map(|item| item.into_fd()),
    );
    Ok(descriptors)
}

struct NoninteractiveBackend;
impl crate::secrets::SecretBackend for NoninteractiveBackend {
    fn env_var(&self, name: &str) -> Option<String> {
        std::env::var(name).ok().filter(|value| !value.is_empty())
    }
    fn secret_tool_lookup(&self, attributes: &[(&str, &str)]) -> Option<String> {
        use crate::secrets::HostCommandRunner;
        let mut command = vec!["secret-tool".into(), "lookup".into()];
        command.extend(
            attributes
                .iter()
                .flat_map(|(name, value)| [(*name).to_owned(), (*value).to_owned()]),
        );
        crate::secrets::OsHostCommandRunner
            .lookup(&command, crate::secrets::COMMAND_SECRET_TIMEOUT)
            .ok()
    }
}

#[cfg(target_os = "linux")]
fn sealed_environment(environment: &HashMap<String, String>) -> io::Result<OwnedFd> {
    use std::fs::File;
    use std::io::{Seek, Write};
    use std::os::fd::{AsRawFd, FromRawFd};

    let raw = unsafe {
        libc::memfd_create(
            c"ags-t3-environment".as_ptr(),
            libc::MFD_CLOEXEC | libc::MFD_ALLOW_SEALING,
        )
    };
    if raw < 0 {
        return Err(io::Error::last_os_error());
    }
    let mut file = unsafe { File::from_raw_fd(raw) };
    serde_json::to_writer(&mut file, environment)?;
    file.flush()?;
    file.rewind()?;
    let seals = libc::F_SEAL_WRITE | libc::F_SEAL_GROW | libc::F_SEAL_SHRINK | libc::F_SEAL_SEAL;
    if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_ADD_SEALS, seals) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(file.into())
}

#[cfg(not(target_os = "linux"))]
fn sealed_environment(_: &HashMap<String, String>) -> io::Result<OwnedFd> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "sealed T3 boot descriptors require Linux",
    ))
}

#[cfg(test)]
#[path = "credentials_tests.rs"]
mod tests;
