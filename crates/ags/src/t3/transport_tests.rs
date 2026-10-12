use super::{
    dispatch::{Shared, State},
    registration::{Registration, Settings},
    repository::Repository,
    transport::Bridge,
};
use russh::{
    ChannelMsg, client,
    keys::{Algorithm, PrivateKey, PrivateKeyWithHashAlg, PublicKey, PublicKeyOrCertificate},
};
use std::sync::{Arc, Mutex};
use std::time::Duration;

struct Client(PublicKey);
impl client::Handler for Client {
    type Error = russh::Error;
    async fn check_server_key(
        &mut self,
        key: &PublicKeyOrCertificate,
    ) -> Result<bool, Self::Error> {
        Ok(key.public_key() == self.0)
    }
}

fn state() -> Shared {
    Arc::new(Mutex::new(State {
        environment: None,
        registration: Registration {
            schema: 1,
            data_root: "/nonexistent/data".into(),
            control_dir: "/nonexistent/control".into(),
            repository: Repository {
                id: "a".repeat(64),
                common: "/nonexistent/.git".into(),
                main: "/nonexistent".into(),
                worktrees: Vec::new(),
                device: 0,
                inode: 0,
            },
            config: "/nonexistent/config.toml".into(),
            overlay: None,
            home: "/nonexistent/home".into(),
            settings: Settings {
                browser: false,
                psp: false,
                psp_keep: false,
                yolo: false,
                root: false,
                wayland: false,
                add_dirs: Vec::new(),
                env_names: Vec::new(),
                op_sources: Vec::new(),
            },
        },
    }))
}

async fn connect(state: Shared, client_key: &PrivateKey) -> client::Handle<Client> {
    let host = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).unwrap();
    let expected = host.public_key().clone();
    let server = Arc::new(russh::server::Config {
        keys: vec![host],
        auth_rejection_time: Duration::from_millis(1),
        ..Default::default()
    });
    let (client_stream, server_stream) = tokio::net::UnixStream::pair().unwrap();
    let public = client_key.public_key().clone();
    tokio::spawn(async move {
        let session = russh::server::run_stream(server, server_stream, Bridge::new(state, public))
            .await
            .unwrap();
        let _ = session.await;
    });
    client::connect_stream(
        Arc::new(client::Config {
            inactivity_timeout: Some(Duration::from_secs(10)),
            ..Default::default()
        }),
        client_stream,
        Client(expected),
    )
    .await
    .unwrap()
}

async fn command(client: &client::Handle<Client>, script: &str) -> (Vec<u8>, Vec<u8>, Option<u32>) {
    let mut channel = client.channel_open_session().await.unwrap();
    channel.exec(true, "sh -s").await.unwrap();
    // Real SSH framing must tolerate scripts fragmented into multiple packets.
    for chunk in script.as_bytes().chunks(23) {
        channel.data(chunk).await.unwrap();
    }
    channel.eof().await.unwrap();
    let mut stdout = Vec::new();
    let mut stderr = Vec::new();
    let mut status = None;
    tokio::time::timeout(Duration::from_secs(10), async {
        while let Some(message) = channel.wait().await {
            match message {
                ChannelMsg::Data { data } => stdout.extend_from_slice(&data),
                ChannelMsg::ExtendedData { data, ext: 1 } => stderr.extend_from_slice(&data),
                ChannelMsg::ExitStatus { exit_status } => status = Some(exit_status),
                _ => {}
            }
        }
    })
    .await
    .unwrap();
    (stdout, stderr, status)
}

#[tokio::test]
async fn genuine_ssh_authentication_eof_stdout_stderr_exit_and_late_teardown() {
    let state = state();
    let key = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).unwrap();
    let mut client = connect(state.clone(), &key).await;
    let wrong = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).unwrap();
    assert!(
        !client
            .authenticate_publickey("dev", PrivateKeyWithHashAlg::new(Arc::new(wrong), None))
            .await
            .unwrap()
            .success()
    );
    assert!(
        client
            .authenticate_publickey("dev", PrivateKeyWithHashAlg::new(Arc::new(key), None))
            .await
            .unwrap()
            .success()
    );
    let stop = super::compatibility::STOP.replace("@@T3_STATE_KEY@@", "0123456789abcdef");
    let (stdout, stderr, status) = command(&client, &stop).await;
    assert_eq!(stdout, b"{\"stopped\":true}\n");
    assert!(stderr.is_empty());
    assert_eq!(status, Some(0));
    assert!(state.lock().unwrap().environment.is_none()); // No container/config/credential lookup needed.
    let (stdout, stderr, status) =
        command(&client, "set -eu\ncurl https://invalid.example/installer\n").await;
    assert!(stdout.is_empty());
    assert!(String::from_utf8_lossy(&stderr).contains("unsupported T3 SSH bootstrap"));
    assert_eq!(status, Some(2));
    assert!(
        client
            .channel_open_direct_tcpip("192.0.2.1", 22, "127.0.0.1", 1234)
            .await
            .is_err()
    );
    client
        .disconnect(russh::Disconnect::ByApplication, "", "English")
        .await
        .unwrap();
}
