use super::{
    dispatch::{Shared, State},
    registration::Registration,
};
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::os::unix::{fs::OpenOptionsExt, net::UnixStream, process::CommandExt};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub fn lock(path: &std::path::Path) -> io::Result<File> {
    File::options()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .mode(0o600)
        .open(path)
}

pub fn ensure(registration: &Registration, environment: &[(String, String)]) -> io::Result<()> {
    let directory = registration.runtime_dir()?;
    crate::util::ensure_private_dir(&directory)?;
    let startup = lock(&directory.join("startup.lock"))?;
    startup.lock()?;
    if request(registration, "ping").is_ok() {
        return Ok(());
    }
    let lifetime = lock(&directory.join("owner.lock"))?;
    lifetime.try_lock().map_err(|error| {
        io::Error::other(format!(
            "T3 owner is starting/unresponsive: {error}; inspect diagnostics before reconnecting"
        ))
    })?;
    lifetime.unlock()?;
    super::owner_process::recover(registration)?;
    let private = registration.private_dir()?;
    crate::util::ensure_private_dir(&private)?;
    let log = File::options()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(private.join("owner.log"))?;
    let mut command = Command::new(std::env::current_exe()?);
    command
        .args(["t3", "_owner", "--registration"])
        .arg(registration.path()?)
        .current_dir(&registration.repository.main)
        .envs(environment.iter().cloned())
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log);
    // Only the async-signal-safe session syscall runs between fork and exec.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() < 0 {
                Err(io::Error::last_os_error())
            } else {
                Ok(())
            }
        });
    }
    let mut child = command.spawn()?;
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        if request(registration, "ping").is_ok() {
            return Ok(());
        }
        if let Some(status) = child.try_wait()? {
            return Err(io::Error::other(format!(
                "T3 owner exited ({status}); see {}",
                private.join("owner.log").display()
            )));
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    Err(io::Error::other(format!(
        "T3 owner readiness timed out; see {}",
        private.join("owner.log").display()
    )))
}

pub fn request(registration: &Registration, command: &str) -> io::Result<String> {
    let mut stream = UnixStream::connect(registration.runtime_dir()?.join("control.sock"))?;
    stream.set_read_timeout(Some(Duration::from_secs(if command == "ping" {
        1
    } else {
        180
    })))?;
    stream.set_write_timeout(Some(Duration::from_secs(5)))?;
    stream.write_all(format!("{command}\n").as_bytes())?;
    stream.shutdown(std::net::Shutdown::Write)?;
    let mut response = String::new();
    stream.take(8192).read_to_string(&mut response)?;
    if let Some(error) = response.strip_prefix("ERR ") {
        return Err(io::Error::other(error.trim().to_owned()));
    }
    if response.is_empty() {
        return Err(io::Error::other("T3 owner returned no readiness response"));
    }
    Ok(response)
}

pub fn proxy(registration: &Registration) -> io::Result<()> {
    ensure(registration, &[])?;
    let mut stream = UnixStream::connect(registration.runtime_dir()?.join("ssh.sock"))?;
    let mut input = stream.try_clone()?;
    std::thread::spawn(move || {
        let _ = io::copy(&mut io::stdin().lock(), &mut input);
        let _ = input.shutdown(std::net::Shutdown::Write);
    });
    // stdout contains only the genuine SSH protocol, never diagnostics/notices.
    // Rust's stdout is line-buffered. SSH binary packets must flush even without
    // a newline, or the client and server can deadlock during key exchange.
    let mut output = io::stdout().lock();
    let mut buffer = [0u8; 16 * 1024];
    loop {
        let count = stream.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        output.write_all(&buffer[..count])?;
        output.flush()?;
    }
    let _ = stream.shutdown(std::net::Shutdown::Both);
    Ok(())
}

pub fn run(registration: Registration) -> io::Result<()> {
    std::env::set_current_dir(&registration.repository.main)?;
    let directory = registration.runtime_dir()?;
    crate::util::ensure_private_dir(&directory)?;
    let lifetime = lock(&directory.join("owner.lock"))?;
    lifetime
        .try_lock()
        .map_err(|_| io::Error::other("another T3 owner already supervises this repository"))?;
    super::owner_process::record(&registration)?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    runtime.block_on(serve(registration))
}

async fn serve(registration: Registration) -> io::Result<()> {
    let directory = registration.runtime_dir()?;
    for name in ["control.sock", "ssh.sock"] {
        match fs::remove_file(directory.join(name)) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
    }
    let control = tokio::net::UnixListener::bind(directory.join("control.sock"))?;
    let ssh = tokio::net::UnixListener::bind(directory.join("ssh.sock"))?;
    let (host_key, public_key) = super::identity::keys(&registration)?;
    let config = Arc::new(russh::server::Config {
        keys: vec![host_key],
        auth_rejection_time: Duration::from_millis(100),
        keepalive_interval: Some(Duration::from_secs(30)),
        keepalive_max: 3,
        ..Default::default()
    });
    let state = Arc::new(Mutex::new(State {
        registration,
        environment: None,
    }));
    let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    loop {
        tokio::select! {
            accepted = control.accept() => {
                let (stream, _) = accepted?;
                if stream.peer_cred()?.uid() == unsafe { libc::geteuid() } {
                    tokio::spawn(control_connection(stream, state.clone()));
                }
            }
            accepted = ssh.accept() => {
                let (stream, _) = accepted?;
                if stream.peer_cred()?.uid() != unsafe { libc::geteuid() } { continue; }
                let handler = super::transport::Bridge::new(state.clone(), public_key.clone());
                let config = config.clone();
                tokio::spawn(async move {
                    match russh::server::run_stream(config, stream, handler).await {
                        Ok(session) => { if let Err(error) = session.await { eprintln!("T3 SSH session: {error}"); } }
                        Err(error) => eprintln!("T3 SSH handshake: {error}"),
                    }
                });
            }
            _ = tokio::signal::ctrl_c() => break,
            _ = term.recv() => break,
        }
    }
    tokio::task::spawn_blocking(move || {
        state
            .lock()
            .map_err(|_| io::Error::other("owner state poisoned"))?
            .stop()
    })
    .await
    .map_err(io::Error::other)??;
    for name in ["control.sock", "ssh.sock"] {
        let _ = fs::remove_file(directory.join(name));
    }
    Ok(())
}

async fn control_connection(mut stream: tokio::net::UnixStream, state: Shared) -> io::Result<()> {
    let mut bytes = Vec::new();
    tokio::time::timeout(
        Duration::from_secs(5),
        (&mut stream).take(1024).read_to_end(&mut bytes),
    )
    .await
    .map_err(io::Error::other)??;
    let command = String::from_utf8(bytes).map_err(io::Error::other)?;
    // Readiness remains responsive while another connection is booting the server.
    if command.trim() == "ping" {
        stream.write_all(b"ready\n").await?;
        return stream.shutdown().await;
    }
    let result = tokio::task::spawn_blocking(move || {
        let mut state = state
            .lock()
            .map_err(|_| io::Error::other("owner state poisoned"))?;
        match command.trim() {
            "ping" => Ok("ready\n".to_owned()),
            "start" => {
                state.ensure_started(None)?;
                Ok("started\n".into())
            }
            "stop" => {
                state.stop()?;
                Ok("stopped\n".into())
            }
            "upgrade" => {
                state.upgrade()?;
                Ok("upgraded\n".into())
            }
            "status" => {
                let spec = super::environment::specification(&state.registration)?;
                Ok(format!(
                    "{}\n",
                    serde_json::json!({"repository": state.registration.repository.main,
                    "owner_pid": std::process::id(),
                    "ssh_alias": state.registration.alias(), "running": state.environment.is_some(),
                    "runtime": spec.as_ref().map(|spec| &spec.version),
                    "generation": spec.as_ref().map(|spec| &spec.generation),
                    "diagnostics": state.registration.private_dir()?.join("owner.log")})
                ))
            }
            _ => Err(io::Error::other("unknown AGS T3 control operation")),
        }
    })
    .await
    .map_err(io::Error::other)?;
    let response = match result {
        Ok(response) => response,
        Err(error) => format!("ERR {error}\n"),
    };
    stream.write_all(response.as_bytes()).await?;
    stream.shutdown().await
}
