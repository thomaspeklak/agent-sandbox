use std::fs;
use std::io::{self, BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::cli::Agent;
use crate::config::ValidatedConfig;

/// Run the setup command: generate SSH keys, bootstrap agent sandboxes,
/// and optionally store secrets.
pub fn run(config: &ValidatedConfig) -> Result<(), SetupError> {
    let auth_key = &config.sandbox.auth_key;
    let sign_key = &config.sandbox.sign_key;

    generate_key_if_missing(auth_key, "ags-agent-auth")?;
    generate_key_if_missing(sign_key, "ags-agent-signing")?;

    println!("\nAdd these public keys to GitHub:\n");
    print_pub_key("Auth key (SSH key for git push)", auth_key)?;
    print_pub_key("Signing key (SSH signing key)", sign_key)?;

    bootstrap_agent_sandboxes(config)?;

    // Write embedded assets for Pi sandbox
    let pi_sandbox = config.sandbox.sandbox_dir_for(Agent::Pi);
    if let Err(e) = crate::assets::ensure_guard_extension(&pi_sandbox) {
        eprintln!("warning: could not write guard extension: {e}");
    }
    if let Err(e) = crate::assets::ensure_settings_template(&pi_sandbox) {
        eprintln!("warning: could not write settings template: {e}");
    }

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

// --- agent sandbox bootstrap ---

/// Per-agent host config sources. `None` means the agent has no known
/// host config directory (sandbox will be created empty at launch time).
struct AgentHostConfig {
    agent: Agent,
    /// Host directory to copy from (e.g. ~/.claude).
    host_dir: Option<PathBuf>,
    /// Extra files outside the host dir to copy into the sandbox.
    /// Each entry is (host_path, filename_in_sandbox).
    extra_files: Vec<(PathBuf, &'static str)>,
}

fn agent_host_configs(config: &ValidatedConfig) -> Vec<AgentHostConfig> {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/home"));
    vec![
        AgentHostConfig {
            agent: Agent::Pi,
            host_dir: Some(config.sandbox.host_pi_dir.clone()),
            extra_files: vec![],
        },
        AgentHostConfig {
            agent: Agent::Claude,
            host_dir: Some(config.sandbox.host_claude_dir.clone()),
            extra_files: vec![],
        },
        AgentHostConfig {
            agent: Agent::Codex,
            host_dir: Some(home.join(".codex")),
            extra_files: vec![],
        },
        AgentHostConfig {
            agent: Agent::Gemini,
            host_dir: Some(home.join(".gemini")),
            extra_files: vec![],
        },
        AgentHostConfig {
            agent: Agent::Opencode,
            host_dir: Some(home.join(".config/opencode")),
            extra_files: vec![],
        },
    ]
}

/// Bootstrap per-agent sandbox directories by copying host config.
/// Skips agents whose sandbox already exists or whose host config is missing.
fn bootstrap_agent_sandboxes(config: &ValidatedConfig) -> Result<(), SetupError> {
    println!("\nBootstrapping agent sandboxes...");

    for ac in agent_host_configs(config) {
        let sandbox = config.sandbox.sandbox_dir_for(ac.agent);

        if sandbox.exists() {
            println!("  {} sandbox exists: {}", ac.agent, sandbox.display());
        } else {
            // Copy host dir → sandbox (if host dir exists)
            let copied_dir = if let Some(ref host_dir) = ac.host_dir {
                if host_dir.is_dir() {
                    fs::create_dir_all(&sandbox)?;
                    copy_dir_contents(host_dir, &sandbox)?;
                    println!(
                        "  {} copied {} → {}",
                        ac.agent,
                        host_dir.display(),
                        sandbox.display()
                    );
                    true
                } else {
                    false
                }
            } else {
                false
            };

            if !copied_dir {
                fs::create_dir_all(&sandbox)?;
                println!(
                    "  {} no host config found, created empty: {}",
                    ac.agent,
                    sandbox.display()
                );
            }
        }

        // Always sync extra files from host (these are config files that
        // should reflect the current host state on each setup run).
        for (host_file, sandbox_name) in &ac.extra_files {
            if host_file.is_file() {
                let dest = sandbox.join(sandbox_name);
                fs::copy(host_file, &dest)?;
                println!(
                    "  {} synced {} → {}",
                    ac.agent,
                    host_file.display(),
                    dest.display()
                );
            }
        }
    }

    // Sync ~/.claude.json → agent_sandbox_base/.claude.json
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/home"));
    let host_claude_json = home.join(".claude.json");
    let dest_claude_json = config.sandbox.agent_sandbox_base.join(".claude.json");
    if host_claude_json.is_file() {
        fs::copy(&host_claude_json, &dest_claude_json)?;
        println!(
            "  synced {} → {}",
            host_claude_json.display(),
            dest_claude_json.display()
        );
    }

    Ok(())
}

/// Recursively copy directory contents from `src` to `dst`.
/// `dst` must already exist. Skips files/dirs that can't be read (broken
/// symlinks, permission errors) with a warning rather than failing.
fn copy_dir_contents(src: &Path, dst: &Path) -> Result<(), SetupError> {
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let file_type = match entry.metadata() {
            Ok(m) => m.file_type(),
            Err(e) => {
                eprintln!("    warning: skipping {}: {e}", entry.path().display());
                continue;
            }
        };
        let src_path = entry.path();
        let dst_path = dst.join(entry.file_name());

        if file_type.is_dir() {
            fs::create_dir_all(&dst_path)?;
            copy_dir_contents(&src_path, &dst_path)?;
        } else if file_type.is_file()
            && let Err(e) = fs::copy(&src_path, &dst_path)
        {
            eprintln!("    warning: skipping {}: {e}", src_path.display());
        }
        // Skip symlinks and other special file types
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

fn pub_key_path(key_path: &Path) -> std::path::PathBuf {
    let mut p = key_path.as_os_str().to_owned();
    p.push(".pub");
    std::path::PathBuf::from(p)
}
