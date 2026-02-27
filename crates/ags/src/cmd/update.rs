use std::fmt;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::config::ValidatedConfig;

/// Options for the update command.
pub struct UpdateOptions {
    pub pi_spec: Option<String>,
    pub minimum_release_age: Option<u32>,
    pub pull: bool,
}

impl Default for UpdateOptions {
    fn default() -> Self {
        Self {
            pi_spec: None,
            minimum_release_age: None,
            pull: true,
        }
    }
}

#[derive(Debug)]
pub enum UpdateError {
    MissingContainerfile(String),
    BuildFailed(String),
}

impl fmt::Display for UpdateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingContainerfile(p) => write!(f, "missing Containerfile: {p}"),
            Self::BuildFailed(msg) => write!(f, "podman build failed: {msg}"),
        }
    }
}

impl std::error::Error for UpdateError {}

/// Rebuild the sandbox container image.
pub fn run(config: &ValidatedConfig, opts: &UpdateOptions) -> Result<(), UpdateError> {
    let image = &config.sandbox.image;
    let containerfile = &config.sandbox.containerfile;

    if !containerfile.exists() {
        return Err(UpdateError::MissingContainerfile(
            containerfile.display().to_string(),
        ));
    }

    let pi_spec = opts.pi_spec.as_deref().unwrap_or(&config.update.pi_spec);
    let release_age = opts
        .minimum_release_age
        .unwrap_or(config.update.minimum_release_age);
    let refresh = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let context_dir = containerfile
        .parent()
        .expect("containerfile must have a parent directory");

    let mut args: Vec<String> = vec![
        "build".into(),
        "-t".into(),
        image.clone(),
        "-f".into(),
        containerfile.display().to_string(),
        format!("--build-arg=PI_CODING_AGENT_SPEC={pi_spec}"),
        format!("--build-arg=PNPM_MINIMUM_RELEASE_AGE={release_age}"),
        format!("--build-arg=PI_REFRESH={refresh}"),
    ];

    if opts.pull {
        args.push("--pull".into());
    }

    args.push(context_dir.display().to_string());

    println!("Rebuilding {image}");
    println!("  PI spec: {pi_spec}");
    println!("  pnpm minimum-release-age: {release_age}");
    println!("  refresh marker: {refresh}");

    let status = Command::new("podman")
        .args(&args)
        .status()
        .map_err(|e| UpdateError::BuildFailed(e.to_string()))?;

    if !status.success() {
        return Err(UpdateError::BuildFailed(format!("exited with {status}")));
    }

    println!("\nDone. Verify with: ags --agent pi -- --version");
    Ok(())
}
