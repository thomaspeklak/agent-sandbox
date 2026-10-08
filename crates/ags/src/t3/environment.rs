use super::{
    container,
    registration::{Registration, write_json},
    sidecars::Sidecars,
};
use crate::plan::LaunchPlan;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs;
use std::io;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Specification {
    pub generation: PathBuf,
    pub image: String,
    pub version: String,
    pub configuration: String,
    pub mounts: String,
}

pub struct Environment {
    pub spec: Specification,
    pub plan: LaunchPlan,
    pub _sidecars: Sidecars,
    pub process: Option<crate::podman::SpawnedProcess>,
}

pub fn specification(registration: &Registration) -> io::Result<Option<Specification>> {
    let path = registration.private_dir()?.join("environment.json");
    if !path.exists() {
        return Ok(None);
    }
    let spec: Specification = serde_json::from_slice(&fs::read(path)?)?;
    let image = spec.image.strip_prefix("sha256:").unwrap_or("");
    if image.len() != 64 || !image.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(io::Error::other(
            "invalid pinned image ID in T3 environment metadata",
        ));
    }
    Ok(Some(spec))
}

fn digest(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

pub fn runtime_version(runtime: &std::path::Path) -> io::Result<String> {
    let root = runtime.join("pnpm-home/ags-t3-runtime");
    let version = fs::read_to_string(root.join("version")).map_err(|_| io::Error::other("T3 runtime is not installed in this generation; run `ags update-agents` and explicit `ags t3 upgrade`"))?;
    let version = version.trim().to_owned();
    let bundle = root.join("versions").join(&version);
    if !crate::util::is_executable(&bundle.join("t3"))
        || ["client", "resource-monitor", "node_modules"]
            .iter()
            .any(|entry| !bundle.join(entry).exists())
    {
        return Err(io::Error::other(
            "T3 runtime bundle is incomplete; run `ags update-agents`",
        ));
    }
    if !super::compatibility::valid_version(&version)
        || fs::read_to_string(
            root.join("versions")
                .join(&version)
                .join(".install-complete"),
        )?
        .trim()
            != version
    {
        return Err(io::Error::other(
            "T3 runtime has an invalid completion marker; run `ags update-agents`",
        ));
    }
    Ok(version)
}

impl Environment {
    pub fn start(
        registration: &Registration,
        requested: Option<&str>,
        upgrade: bool,
    ) -> io::Result<Self> {
        let mut config = registration.load_config()?;
        registration.validate_host_control_mounts(&config)?;
        let configuration = digest(&format!("{config:?}"));
        let previous = specification(registration)?;
        let lease = if !upgrade && let Some(previous) = &previous {
            crate::agent_runtime::pin_path(&config.sandbox.cache_dir, &previous.generation)?
        } else {
            crate::agent_runtime::pin(&config.sandbox.cache_dir)?
        };
        let version = runtime_version(&lease.path)?;
        if let Some(requested) = requested {
            super::compatibility::require_version(requested, &version, &version)?;
        }
        if !upgrade
            && previous
                .as_ref()
                .is_some_and(|spec| spec.configuration != configuration)
        {
            return Err(io::Error::other(
                "registered T3 environment configuration changed; use explicit `ags t3 upgrade` to recreate it",
            ));
        }
        let inspected = container::inspect(registration)?;
        if inspected.is_some() {
            container::stop(registration)?;
        }
        if inspected.is_some() && previous.is_none() && !upgrade {
            return Err(io::Error::other(
                "owned T3 container has incomplete startup metadata; use `ags t3 upgrade` to recover it",
            ));
        }
        if upgrade && inspected.is_some() {
            container::output(&["rm", &container::id(registration)?])?;
        }
        if upgrade || previous.is_none() {
            crate::podman::ensure_image(
                &config.sandbox.image,
                &config.sandbox.pnpm_version,
                &config.sandbox.extra_dnf_packages,
                &config.sandbox.tool_downloads,
            )
            .map_err(io::Error::other)?;
        }
        let image = if !upgrade && let Some(previous) = &previous {
            previous.image.clone()
        } else {
            let value = container::output(&[
                "image",
                "inspect",
                "--format",
                "{{.Id}}",
                &config.sandbox.image,
            ])?;
            String::from_utf8(value.stdout)
                .map_err(io::Error::other)?
                .trim()
                .to_owned()
        };
        let assets = super::providers::prepare(registration, &config)?;
        crate::git::ensure_gitconfig(
            &config.sandbox.gitconfig_path,
            "/home/dev/.ssh/ags-agent-signing.pub",
        )
        .map_err(|error| io::Error::other(error.to_string()))?;
        let ssh = super::ssh_agent::prepare(&config);
        let base = registration.runtime_dir()?;
        let sidecars = Sidecars::start(registration, &mut config, &base)?;
        let mut plan = super::environment_plan::build(
            registration,
            &config,
            &sidecars,
            &lease.path,
            &version,
            &assets,
            ssh.as_deref(),
        )?;
        crate::podman::adapt_network_mode_for_installed_podman(&mut plan);
        let mounts = digest(&format!("{:?}", plan.mounts));
        if !upgrade && previous.as_ref().is_some_and(|spec| spec.mounts != mounts) {
            return Err(io::Error::other(
                "T3 worktree/mount layout changed; use explicit `ags t3 upgrade` (mounted data is preserved)",
            ));
        }
        let spec = Specification {
            generation: lease.path.clone(),
            image,
            version,
            configuration,
            mounts,
        };
        if inspected.is_none() || upgrade {
            create(registration, &plan, &spec)?;
        } else if let Some(container) = &inspected {
            super::container::validate_layout(container, &plan, &spec)?;
        }
        write_json(&registration.private_dir()?.join("environment.json"), &spec)?;
        container::output(&["start", &container::id(registration)?])?;
        let mut environment = Self {
            spec,
            plan,
            _sidecars: sidecars,
            process: None,
        };
        if let Err(error) = environment.boot(registration, &config) {
            let _ = environment.stop(registration);
            return Err(error);
        }
        Ok(environment)
    }

    fn boot(
        &mut self,
        registration: &Registration,
        config: &crate::config::ValidatedConfig,
    ) -> io::Result<()> {
        self.plan.image = self.spec.image.clone();
        let binary = registration
            .home
            .join(".t3/runtime/versions")
            .join(&self.spec.version)
            .join("t3")
            .display()
            .to_string();
        let actual = container::exec_output(registration, &[&binary, "--version"])?;
        let actual = String::from_utf8(actual.stdout).map_err(io::Error::other)?;
        let actual = actual
            .trim()
            .trim_start_matches("t3 ")
            .trim_start_matches('v');
        super::compatibility::require_version(&self.spec.version, &self.spec.version, actual)?;
        let descriptors = super::credentials::prepare(registration, config)?;
        let count = descriptors.len() - 1;
        let args = vec![
            "exec".into(),
            format!("--preserve-fds={}", descriptors.len()),
            container::id(registration)?,
            "/run/ags-t3/environment-bootstrap".into(),
            count.to_string(),
            binary.clone(),
            "serve".into(),
            "--host".into(),
            "127.0.0.1".into(),
            "--port".into(),
            "3773".into(),
            "--base-dir".into(),
            registration.home.join(".t3").display().to_string(),
            "--no-browser".into(),
            "--auto-bootstrap-project-from-cwd".into(),
        ];
        self.process = Some(crate::podman::spawn_persistent_exec(&args, descriptors)?);
        container::exec_output(
            registration,
            &[
                &binary,
                "__ssh-helper",
                "wait-ready",
                "3773",
                "60000",
                "1000",
            ],
        )?;
        if self.process.as_mut().unwrap().try_wait()?.is_some() {
            return Err(io::Error::other(
                "T3 server exited during startup; see the owner log",
            ));
        }
        Ok(())
    }

    pub fn stop(&mut self, registration: &Registration) -> io::Result<()> {
        if self.process.is_some() {
            let runtime = registration.home.join(".t3/userdata/server-runtime.json");
            if let Err(error) = container::exec_output(
                registration,
                &["/run/ags-t3/stop-server", &runtime.display().to_string()],
            ) {
                eprintln!("warning: T3 graceful server stop: {error}; stopping the container");
            }
        }
        container::stop(registration)?;
        if let Some(process) = self.process.take() {
            process.wait()?;
        }
        Ok(())
    }
}

fn create(registration: &Registration, plan: &LaunchPlan, spec: &Specification) -> io::Result<()> {
    let mut plan = plan.clone();
    plan.image = spec.image.clone();
    let mut args = crate::podman::build_run_args(&plan, std::path::Path::new("/dev/null"));
    args[0] = "create".into();
    args.retain(|arg| arg != "--rm" && arg != "-it");
    args.splice(
        1..1,
        [
            format!(
                "--label=io.ags.t3.repository={}",
                registration.repository.id
            ),
            format!("--label=io.ags.t3.uid={}", unsafe { libc::geteuid() }),
            format!(
                "--label=io.ags.t3.registration={}",
                registration.path()?.display()
            ),
            format!("--label=io.ags.t3.generation={}", spec.generation.display()),
            format!("--label=io.ags.t3.configuration={}", spec.configuration),
            format!("--label=io.ags.t3.mounts={}", spec.mounts),
        ],
    );
    container::output(&args.iter().map(String::as_str).collect::<Vec<_>>())?;
    Ok(())
}
