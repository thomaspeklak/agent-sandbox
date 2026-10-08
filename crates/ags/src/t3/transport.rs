use super::{
    compatibility,
    dispatch::{self, Shared},
};
use russh::keys::PublicKey;
use russh::{
    Channel, ChannelId,
    server::{Auth, ChannelOpenHandle, Handler, Msg, Session},
};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::task::JoinHandle;

pub struct Bridge {
    pub state: Shared,
    pub public_key: PublicKey,
    pending: HashMap<ChannelId, (String, Vec<u8>)>,
    tasks: HashMap<ChannelId, JoinHandle<()>>,
}

impl Bridge {
    pub fn new(state: Shared, public_key: PublicKey) -> Self {
        Self {
            state,
            public_key,
            pending: HashMap::new(),
            tasks: HashMap::new(),
        }
    }
}

impl Handler for Bridge {
    type Error = russh::Error;

    async fn auth_publickey(&mut self, user: &str, key: &PublicKey) -> Result<Auth, Self::Error> {
        Ok(if user == "dev" && key == &self.public_key {
            Auth::Accept
        } else {
            Auth::reject()
        })
    }

    async fn channel_open_session(
        &mut self,
        _channel: Channel<Msg>,
        reply: ChannelOpenHandle,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        if self.pending.len() + self.tasks.len() < 32 {
            reply.accept().await;
        }
        Ok(())
    }

    async fn exec_request(
        &mut self,
        channel: ChannelId,
        command: &[u8],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        let command = std::str::from_utf8(command).unwrap_or("");
        if self.pending.contains_key(&channel)
            || self.tasks.contains_key(&channel)
            || !(command == "sh -s" || command.starts_with("sh -l -s -- "))
        {
            session.channel_failure(channel)?;
            session.extended_data(
                channel,
                1,
                b"AGS only accepts supported T3 SSH bootstrap commands\n".to_vec(),
            )?;
            session.exit_status_request(channel, 2)?;
            session.eof(channel)?;
            session.close(channel)?;
        } else {
            self.pending
                .insert(channel, (command.to_owned(), Vec::new()));
            session.channel_success(channel)?;
        }
        Ok(())
    }

    async fn data(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        if let Some((_, script)) = self.pending.get_mut(&channel) {
            if script.len() + data.len() > compatibility::MAX_SCRIPT_BYTES {
                self.pending.remove(&channel);
                session.extended_data(
                    channel,
                    1,
                    b"T3 SSH bootstrap exceeds AGS size limit\n".to_vec(),
                )?;
                session.exit_status_request(channel, 2)?;
                session.eof(channel)?;
                session.close(channel)?;
            } else {
                script.extend_from_slice(data);
            }
        }
        Ok(())
    }

    async fn channel_eof(
        &mut self,
        channel: ChannelId,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        if let Some((command, script)) = self.pending.remove(&channel) {
            let operation = compatibility::classify(&command, &script);
            let state = Arc::clone(&self.state);
            let handle = session.handle();
            self.tasks.insert(
                channel,
                tokio::spawn(async move {
                    let result = match operation {
                        Ok(operation) => dispatch::operation(state, operation).await,
                        Err(error) => Err(error),
                    };
                    let (stdout, stderr, status) = match result {
                        Ok(reply) => (reply.stdout, reply.stderr, reply.status),
                        Err(error) => (Vec::new(), format!("AGS T3: {error}\n").into_bytes(), 2),
                    };
                    if !stdout.is_empty() {
                        let _ = handle.data(channel, stdout).await;
                    }
                    if !stderr.is_empty() {
                        let _ = handle.extended_data(channel, 1, stderr).await;
                    }
                    let _ = handle.exit_status_request(channel, status).await;
                    let _ = handle.eof(channel).await;
                    let _ = handle.close(channel).await;
                }),
            );
        }
        Ok(())
    }

    async fn channel_close(
        &mut self,
        channel: ChannelId,
        _session: &mut Session,
    ) -> Result<(), Self::Error> {
        self.pending.remove(&channel);
        if let Some(task) = self.tasks.remove(&channel) {
            task.abort();
        }
        Ok(())
    }

    async fn channel_open_direct_tcpip(
        &mut self,
        channel: Channel<Msg>,
        host: &str,
        port: u32,
        _originator: &str,
        _originator_port: u32,
        reply: ChannelOpenHandle,
        session: &mut Session,
    ) -> Result<(), Self::Error> {
        let state = self.state.clone();
        let host = host.to_owned();
        let target = tokio::task::spawn_blocking(move || {
            state
                .lock()
                .map_err(|_| std::io::Error::other("owner state poisoned"))?
                .forwarding_target(&host, port)
        })
        .await;
        let Ok(Ok(target)) = target else {
            return Ok(());
        };
        let mut child = match tokio::process::Command::new("podman")
            .args([
                "exec",
                "-i",
                &target,
                "socat",
                "STDIO",
                "TCP:127.0.0.1:3773",
            ])
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit())
            .kill_on_drop(true)
            .spawn()
        {
            Ok(child) => child,
            Err(_) => return Ok(()),
        };
        let id = channel.id();
        let handle = session.handle();
        reply.accept().await;
        self.tasks.insert(
            id,
            tokio::spawn(async move {
                let mut stream = channel.into_stream();
                let mut process = super::transport_io::Duplex {
                    reader: child.stdout.take().unwrap(),
                    writer: Some(child.stdin.take().unwrap()),
                };
                let _ = tokio::io::copy_bidirectional(&mut stream, &mut process).await;
                drop(process);
                let _ = child.wait().await;
                let _ = handle.eof(id).await;
                let _ = handle.close(id).await;
            }),
        );
        Ok(())
    }
}

impl Drop for Bridge {
    fn drop(&mut self) {
        for (_, task) in self.tasks.drain() {
            task.abort();
        }
    }
}
