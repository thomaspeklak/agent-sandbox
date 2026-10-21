use super::registration::Registration;
use russh::keys::{Algorithm, PrivateKey, PublicKey, ssh_key::LineEnding};
use std::fs;
use std::io;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;

pub fn keys(registration: &Registration) -> io::Result<(PrivateKey, PublicKey)> {
    let directory = registration.private_dir()?;
    crate::util::ensure_private_dir(&directory)?;
    let lock = super::owner::lock(&directory.join("identity.lock"))?;
    lock.lock()?;
    let host = key(&directory.join("host_key"))?;
    let client = key(&directory.join("client_key"))?;
    let expected = format!(
        "{} {}\n",
        registration.alias(),
        host.public_key().to_openssh().map_err(io::Error::other)?
    );
    let known = directory.join("known_hosts");
    if known.exists() {
        if fs::read_to_string(&known)? != expected {
            return Err(io::Error::other(
                "T3 host identity differs from the registered known-host key",
            ));
        }
    } else {
        fs::write(&known, expected)?;
        fs::set_permissions(&known, fs::Permissions::from_mode(0o600))?;
    }
    Ok((host, client.public_key().clone()))
}

fn key(path: &Path) -> io::Result<PrivateKey> {
    if path.exists() {
        return russh::keys::load_secret_key(path, None).map_err(io::Error::other);
    }
    let key = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).map_err(io::Error::other)?;
    let encoded = key.to_openssh(LineEnding::LF).map_err(io::Error::other)?;
    use std::io::Write;
    let mut file = tempfile::NamedTempFile::new_in(path.parent().unwrap())?;
    file.as_file()
        .set_permissions(fs::Permissions::from_mode(0o600))?;
    file.write_all(encoded.as_bytes())?;
    file.as_file().sync_all()?;
    file.persist_noclobber(path).map_err(|error| error.error)?;
    Ok(key)
}

fn ssh_quote(path: &Path) -> io::Result<String> {
    let path = path
        .to_str()
        .ok_or_else(|| io::Error::other("SSH paths must be UTF-8"))?;
    if path.contains(['\n', '\r', '\0']) {
        return Err(io::Error::other("invalid SSH path"));
    }
    Ok(format!(
        "\"{}\"",
        path.replace('\\', "\\\\").replace('"', "\\\"")
    ))
}

pub fn configure(registration: &Registration) -> io::Result<()> {
    keys(registration)?;
    let directory = registration.data_root.join("ssh");
    crate::util::ensure_private_dir(&directory)?;
    let identity = registration.private_dir()?;
    let executable = std::env::current_exe()?;
    let proxy = format!(
        "{} t3 _proxy --registration {}",
        crate::util::shell_quote(&executable.display().to_string()),
        crate::util::shell_quote(&registration.path()?.display().to_string())
    )
    .replace('%', "%%");
    let config = format!(
        "# AGS-managed T3 connection\nHost {}\n  HostName {}\n  User dev\n  ProxyCommand {proxy}\n  IdentityFile {}\n  IdentitiesOnly yes\n  StrictHostKeyChecking yes\n  UserKnownHostsFile {}\n  HostKeyAlias {}\n  BatchMode yes\n",
        registration.alias(),
        registration.alias(),
        ssh_quote(&identity.join("client_key"))?,
        ssh_quote(&identity.join("known_hosts"))?,
        registration.alias()
    );
    fs::write(
        directory.join(format!("{}.conf", registration.repository.id)),
        config,
    )?;
    let home = dirs::home_dir().ok_or_else(|| io::Error::other("cannot locate SSH home"))?;
    crate::util::ensure_private_dir(&home.join(".ssh"))?;
    let config_path = home.join(".ssh/config");
    let previous = if config_path.exists() {
        fs::read_to_string(&config_path)?
    } else {
        String::new()
    };
    let include = format!("Include {}\n", ssh_quote(&directory.join("*.conf"))?);
    if !previous.lines().any(|line| format!("{line}\n") == include) {
        fs::write(&config_path, format!("{include}{previous}"))?;
        fs::set_permissions(config_path, fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}
