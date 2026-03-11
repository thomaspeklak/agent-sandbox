use std::fs;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::ValidatedConfig;

/// Run the setup command: generate SSH keys, ensure Pi assets on mounted host path,
/// and optionally store secrets.
pub fn run(config: &ValidatedConfig) -> Result<(), SetupError> {
    let auth_key = &config.sandbox.auth_key;
    let sign_key = &config.sandbox.sign_key;

    generate_key_if_missing(auth_key, "ags-agent-auth")?;
    generate_key_if_missing(sign_key, "ags-agent-signing")?;

    println!("\nAdd these public keys to GitHub:\n");
    print_pub_key("Auth key (SSH key for git push)", auth_key)?;
    print_pub_key("Signing key (SSH signing key)", sign_key)?;

    ensure_pi_assets(config)?;
    ensure_claude_assets(config)?;

    if !has_command("secret-tool") {
        println!(
            "\nsecret-tool not found; skipping secret-store setup.\n\
             You can still provide secrets via environment variables at runtime."
        );
        return Ok(());
    }

    store_secrets_interactive(config)?;

    println!(
        "\nSetup complete.\n\n\
         Next steps:\n\
         1) Add auth key as GitHub SSH key.\n\
         2) Add signing key as GitHub SSH signing key.\n\
         3) Run: ags doctor\n\
         4) Start: ags --agent pi"
    );
    Ok(())
}

#[derive(Debug)]
pub enum SetupError {
    KeyGen(String),
    Io(io::Error),
}

impl std::fmt::Display for SetupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::KeyGen(msg) => write!(f, "key generation failed: {msg}"),
            Self::Io(e) => write!(f, "I/O error: {e}"),
        }
    }
}

impl std::error::Error for SetupError {}

impl From<io::Error> for SetupError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

fn ensure_pi_assets(config: &ValidatedConfig) -> Result<(), SetupError> {
    let Some(pi_host) = config.mount_host_for_container("/home/dev/.pi") else {
        eprintln!(
            "warning: no mount found for /home/dev/.pi; cannot install Pi guard/settings assets"
        );
        return Ok(());
    };

    let pi_agent_dir = pi_host.join("agent");
    fs::create_dir_all(pi_agent_dir.join("extensions"))?;

    if let Err(e) = crate::assets::ensure_guard_extension(&pi_agent_dir) {
        eprintln!("warning: could not write guard extension: {e}");
    }
    if let Err(e) = crate::assets::ensure_settings_template(&pi_agent_dir) {
        eprintln!("warning: could not write settings template: {e}");
    }

    Ok(())
}

fn ensure_claude_assets(config: &ValidatedConfig) -> Result<(), SetupError> {
    let hooks_dir = config.sandbox.cache_dir.join("ags-hooks");
    if let Err(e) = crate::assets::ensure_claude_guard_hook(&hooks_dir) {
        eprintln!("warning: could not write Claude guard hook: {e}");
    }
    if let Err(e) = crate::assets::ensure_claude_guard_skill(&hooks_dir) {
        eprintln!("warning: could not write Claude guard skill: {e}");
    }
    Ok(())
}

// --- SSH key generation ---

fn generate_key_if_missing(key_path: &Path, comment: &str) -> Result<(), SetupError> {
    if key_path.exists() {
        return Ok(());
    }

    if let Some(parent) = key_path.parent() {
        fs::create_dir_all(parent)?;
    }

    println!("Generating {}", key_path.display());
    let status = Command::new("ssh-keygen")
        .args(["-t", "ed25519", "-a", "64", "-f"])
        .arg(key_path)
        .args(["-C", comment])
        .status()
        .map_err(|e| SetupError::KeyGen(e.to_string()))?;

    if !status.success() {
        return Err(SetupError::KeyGen(format!(
            "ssh-keygen exited with {}",
            status
        )));
    }
    Ok(())
}

fn print_pub_key(label: &str, key_path: &Path) -> Result<(), SetupError> {
    let pub_path = pub_key_path(key_path);
    println!("{label}:");
    if pub_path.exists() {
        let content = fs::read_to_string(&pub_path)?;
        print!("{content}");
    } else {
        println!("  (public key not found: {})", pub_path.display());
    }
    println!();
    Ok(())
}

fn store_secrets_interactive(config: &ValidatedConfig) -> Result<(), SetupError> {
    use crate::config::SecretSource;
    use std::collections::BTreeSet;

    let env_names: BTreeSet<&str> = config.secrets.iter().map(|s| s.env.as_str()).collect();

    if env_names.is_empty() {
        return Ok(());
    }

    println!("\nOptional: store configured secrets in keyring now.");
    println!("Press Enter on empty prompt to skip an env var.\n");

    let stdin = io::stdin();
    let mut reader = stdin.lock();

    for env_name in &env_names {
        eprint!("{env_name}: ");
        io::stderr().flush()?;

        let mut value = String::new();
        reader.read_line(&mut value)?;
        let value = value.trim_end_matches('\n').trim_end_matches('\r');

        if value.is_empty() {
            println!("Skipped {env_name}");
            continue;
        }

        let mut stored = false;
        for secret in &config.secrets {
            if secret.env != *env_name {
                continue;
            }
            if let SecretSource::SecretTool { attributes } = &secret.source {
                if attributes.is_empty() {
                    continue;
                }
                let mut args: Vec<&str> = vec!["store", "--label"];
                let label = format!("ags {env_name}");
                args.push(&label);
                for (k, v) in attributes {
                    args.push(k);
                    args.push(v);
                }

                let mut child = Command::new("secret-tool")
                    .args(&args)
                    .stdin(std::process::Stdio::piped())
                    .spawn()
                    .map_err(|e| SetupError::KeyGen(e.to_string()))?;

                if let Some(ref mut stdin_pipe) = child.stdin {
                    let _ = stdin_pipe.write_all(value.as_bytes());
                }
                let status = child
                    .wait()
                    .map_err(|e| SetupError::KeyGen(e.to_string()))?;

                if status.success() {
                    println!("Stored {env_name} via secret-store");
                    stored = true;
                }
            }
        }

        if !stored {
            println!("No secret_store configured for {env_name} (env-only).");
        }
    }

    Ok(())
}

fn has_command(name: &str) -> bool {
    Command::new("which")
        .arg(name)
        .output()
        .is_ok_and(|o| o.status.success())
}

fn pub_key_path(key_path: &Path) -> PathBuf {
    let mut p = key_path.as_os_str().to_owned();
    p.push(".pub");
    PathBuf::from(p)
}
