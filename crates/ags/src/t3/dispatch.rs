use super::{
    compatibility::{Operation, require_version},
    environment::Environment,
    registration::Registration,
};
use std::io;
use std::sync::{Arc, Mutex};

pub type Shared = Arc<Mutex<State>>;
pub struct State {
    pub registration: Registration,
    pub environment: Option<Environment>,
}

pub struct Reply {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub status: u32,
}

impl State {
    pub fn ensure_started(&mut self, requested: Option<&str>) -> io::Result<()> {
        self.registration.load_config()?;
        if let Some(environment) = &mut self.environment {
            let repository = self.registration.repository.validate()?;
            for worktree in repository.worktrees {
                if !environment.plan.mounts.iter().any(|mount| {
                    mount.mode == crate::config::MountMode::Rw
                        && worktree.starts_with(&mount.host)
                        && mount.container == mount.host.display().to_string()
                }) {
                    return Err(io::Error::other(
                        "a repository worktree is outside the live T3 mount layout; use explicit `ags t3 upgrade` to mount it without silently interrupting jobs",
                    ));
                }
            }
            if let Some(requested) = requested {
                require_version(
                    requested,
                    &environment.spec.version,
                    &environment.spec.version,
                )?;
            }
            if environment
                .process
                .as_mut()
                .is_some_and(|process| process.try_wait().ok().flatten().is_some())
            {
                self.stop()?;
            }
        }
        if self.environment.is_none() {
            self.environment = Some(Environment::start(&self.registration, requested, false)?);
        }
        Ok(())
    }

    pub fn stop(&mut self) -> io::Result<()> {
        if let Some(environment) = &mut self.environment {
            environment.stop(&self.registration)?;
        } else if super::environment::specification(&self.registration)?.is_some() {
            super::container::stop(&self.registration)?;
        }
        self.environment = None; // Drop sidecars only after the container stopped.
        Ok(())
    }

    pub fn upgrade(&mut self) -> io::Result<()> {
        self.registration.load_config()?;
        let _old_lease = self
            .environment
            .as_ref()
            .and_then(|env| env.plan.runtime_lease.clone());
        self.stop()?;
        self.environment = Some(Environment::start(&self.registration, None, true)?);
        Ok(())
    }

    pub fn operation(&mut self, operation: Operation) -> io::Result<Reply> {
        let stdout = match operation {
            Operation::Disconnect => b"{\"stopped\":true}\n".to_vec(),
            Operation::Logs => format!(
                "AGS owns this server. Diagnostics: {}\n",
                self.registration.private_dir()?.join("owner.log").display()
            )
            .into_bytes(),
            Operation::Launch(version) => {
                self.ensure_started(Some(&version))?;
                b"{\"remotePort\":3773,\"serverKind\":\"external\"}\n".to_vec()
            }
            Operation::Pair(version) => {
                self.ensure_started(Some(&version))?;
                let binary = self
                    .registration
                    .home
                    .join(".t3/runtime/versions")
                    .join(version)
                    .join("t3");
                let base = self.registration.home.join(".t3");
                let result = super::container::exec_output(
                    &self.registration,
                    &[
                        &binary.display().to_string(),
                        "auth",
                        "pairing",
                        "create",
                        "--base-dir",
                        &base.display().to_string(),
                        "--json",
                    ],
                )?;
                return Ok(Reply {
                    stdout: result.stdout,
                    stderr: result.stderr,
                    status: 0,
                });
            }
        };
        Ok(Reply {
            stdout,
            stderr: Vec::new(),
            status: 0,
        })
    }

    pub fn forwarding_target(&self, host: &str, port: u32) -> io::Result<String> {
        self.registration.load_config()?;
        if !["127.0.0.1", "localhost"].contains(&host) || port != 3773 || self.environment.is_none()
        {
            return Err(io::Error::other(
                "T3 forwarding requires the running environment's loopback server on port 3773",
            ));
        }
        super::container::id(&self.registration)
    }
}

pub async fn operation(state: Shared, operation: Operation) -> io::Result<Reply> {
    tokio::task::spawn_blocking(move || {
        state
            .lock()
            .map_err(|_| io::Error::other("T3 owner state poisoned"))?
            .operation(operation)
    })
    .await
    .map_err(io::Error::other)?
}
