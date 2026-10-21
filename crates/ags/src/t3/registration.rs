use super::repository::Repository;
use crate::cli::{Agent, RunOptions};
use crate::config::ValidatedConfig;
use serde::{Deserialize, Serialize};
use std::fs::{self, File};
use std::io::{self, Write};
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    pub browser: bool,
    pub psp: bool,
    pub psp_keep: bool,
    pub yolo: bool,
    pub root: bool,
    pub wayland: bool,
    pub add_dirs: Vec<PathBuf>,
    pub env_names: Vec<String>,
    pub op_sources: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Registration {
    pub schema: u32,
    pub data_root: PathBuf,
    pub control_dir: PathBuf,
    pub repository: Repository,
    pub config: PathBuf,
    pub overlay: Option<PathBuf>,
    pub home: PathBuf,
    pub settings: Settings,
}

pub fn root() -> io::Result<PathBuf> {
    let root = dirs::data_dir()
        .map(|dir| dir.join("ags/t3"))
        .ok_or_else(|| io::Error::other("cannot locate T3 data directory"))?;
    crate::util::ensure_private_dir(&root)?;
    root.canonicalize()
}

pub fn write_json(path: &Path, value: &impl Serialize) -> io::Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("missing registration parent"))?;
    crate::util::ensure_private_dir(parent)?;
    let mut file = tempfile::NamedTempFile::new_in(parent)?;
    file.as_file()
        .set_permissions(fs::Permissions::from_mode(0o600))?;
    serde_json::to_writer_pretty(&mut file, value)?;
    writeln!(file)?;
    file.as_file().sync_all()?;
    file.persist(path).map_err(|error| error.error)?;
    File::open(parent)?.sync_all()
}

impl Registration {
    pub fn path(&self) -> io::Result<PathBuf> {
        Ok(self
            .data_root
            .join("registry")
            .join(format!("{}.json", self.repository.id)))
    }
    pub fn private_dir(&self) -> io::Result<PathBuf> {
        Ok(self.data_root.join("identity").join(&self.repository.id))
    }
    pub fn runtime_dir(&self) -> io::Result<PathBuf> {
        Ok(self.control_dir.clone())
    }
    pub fn name(&self) -> String {
        format!("ags-t3-{}", &self.repository.id[..16])
    }
    pub fn alias(&self) -> String {
        self.name()
    }

    pub fn read(path: &Path) -> io::Result<Self> {
        let path = path.canonicalize()?;
        if path.parent().and_then(Path::file_name) != Some(std::ffi::OsStr::new("registry")) {
            return Err(io::Error::other(
                "T3 registration is outside the host registry",
            ));
        }
        let metadata = path.metadata()?;
        if !metadata.is_file()
            || metadata.uid() != unsafe { libc::geteuid() }
            || metadata.mode() & 0o077 != 0
        {
            return Err(io::Error::other(
                "T3 registration must be a private file owned by this user",
            ));
        }
        let registration: Self = serde_json::from_slice(&fs::read(&path)?)?;
        if registration.schema != 1
            || registration.path()? != path
            || registration.repository.id.len() != 64
            || !registration
                .repository
                .id
                .bytes()
                .all(|b| b.is_ascii_hexdigit())
        {
            return Err(io::Error::other("invalid T3 registration identity/schema"));
        }
        let actual_root = path
            .parent()
            .and_then(Path::parent)
            .ok_or_else(|| io::Error::other("invalid T3 registry root"))?;
        if registration.data_root != actual_root || !registration.control_dir.is_absolute() {
            return Err(io::Error::other(
                "invalid T3 registration storage/control context",
            ));
        }
        let expected_home = registration
            .data_root
            .join("environments")
            .join(&registration.repository.id)
            .join("home");
        if registration.home != expected_home {
            return Err(io::Error::other("invalid T3 home in registration"));
        }
        registration.repository.validate()?;
        Ok(registration)
    }

    pub fn load_config(&self) -> io::Result<ValidatedConfig> {
        self.repository.validate()?;
        if let Some(overlay) = &self.overlay
            && (!overlay.is_file()
                || !crate::trust::is_repo_trusted(
                    &crate::trust::default_trust_store_path(),
                    &self.repository.main,
                )
                .map_err(io::Error::other)?)
        {
            return Err(io::Error::other(
                "registered repository overlay is unavailable/untrusted; run `ags --agent t3` interactively to prepare trust",
            ));
        }
        let config =
            crate::config::parse_and_validate_with_overlay(&self.config, self.overlay.as_deref())
                .map_err(io::Error::other)?;
        if !config.sandbox.is_agent_enabled(Agent::T3) {
            return Err(io::Error::other(
                "T3 is disabled in the registered configuration; enable it in `ags tools` and run `ags update-agents`",
            ));
        }
        Ok(config)
    }
}

pub fn register(options: &RunOptions) -> io::Result<Registration> {
    crate::psp::validate_options(options).map_err(io::Error::other)?;
    if options.lockdown
        || options.tmux
        || options.stop_when_done
        || !options.passthrough_args.is_empty()
    {
        return Err(io::Error::other(
            "persistent T3 uses `ags --agent t3` without --lockdown, --tmux, --stop-when-done or CLI passthrough (use -- --version for runtime inspection)",
        ));
    }
    let cwd = std::env::current_dir()?;
    let repository = Repository::resolve(&cwd)?;
    let config_path = options
        .config_path
        .clone()
        .unwrap_or_else(crate::config::default_config_path);
    let config_path = if config_path.is_absolute() {
        config_path
    } else {
        cwd.join(config_path)
    };
    std::env::set_current_dir(&repository.main)?;
    let result = (|| {
        if !config_path.exists() {
            crate::config::create_default_config(&config_path)?;
        }
        let overlay = crate::trust::resolve_repo_local_overlay(
            &repository.main,
            &config_path,
            &crate::trust::default_trust_store_path(),
            &crate::trust::StdioRepoConfigPrompter,
        )
        .map_err(io::Error::other)?;
        let config =
            crate::config::parse_and_validate_with_overlay(&config_path, overlay.as_deref())
                .map_err(io::Error::other)?;
        if !config.sandbox.is_agent_enabled(Agent::T3) {
            return Err(io::Error::other(
                "T3 is disabled; select it in `ags tools` then run `ags update-agents`",
            ));
        }
        for source in &options.op_secret_sets {
            crate::onepassword::SourceRef::parse(source).map_err(io::Error::other)?;
        }
        let data_root = root()?;
        let control_dir = crate::util::runtime_dir()?.join(format!("t3-{}", &repository.id[..16]));
        if control_dir
            .join("control.sock")
            .as_os_str()
            .as_encoded_bytes()
            .len()
            >= 108
        {
            return Err(io::Error::other(
                "T3 control path is too long for a Unix socket; use a shorter XDG_RUNTIME_DIR",
            ));
        }
        let home = data_root
            .join("environments")
            .join(&repository.id)
            .join("home");
        crate::util::ensure_private_dir(&home)?;
        let mut registration = Registration {
            schema: 1,
            data_root,
            control_dir,
            repository,
            config: config_path.canonicalize()?,
            overlay,
            home,
            settings: Settings {
                browser: options.browser,
                psp: options.psp,
                psp_keep: options.psp_keep,
                yolo: options.yolo,
                root: options.root,
                wayland: options.wayland_compositor_passthrough,
                add_dirs: options
                    .add_dirs
                    .iter()
                    .map(|dir| cwd.join(dir).canonicalize())
                    .collect::<io::Result<_>>()?,
                env_names: options.env.iter().map(|(name, _)| name.clone()).collect(),
                op_sources: options.op_secret_sets.clone(),
            },
        };
        let path = registration.path()?;
        if path.exists() {
            let previous = Registration::read(&path)?;
            if previous.config != registration.config
                || previous.overlay != registration.overlay
                || previous.settings != registration.settings
            {
                return Err(io::Error::other(
                    "repository already has a T3 registration with different launch settings; use its registered settings (one environment per repository)",
                ));
            }
            registration.control_dir = previous.control_dir;
        }
        registration.validate_host_control_mounts(&config)?;
        write_json(&path, &registration)?;
        Ok(registration)
    })();
    std::env::set_current_dir(cwd)?;
    result
}

impl Registration {
    pub fn validate_host_control_mounts(&self, config: &ValidatedConfig) -> io::Result<()> {
        let protected = [
            self.data_root.join("registry"),
            self.data_root.join("identity"),
            self.control_dir.clone(),
        ];
        let sources = self
            .repository
            .worktrees
            .iter()
            .chain(std::iter::once(&self.repository.common))
            .chain(self.settings.add_dirs.iter())
            .chain(
                config
                    .mounts
                    .iter()
                    .filter(|mount| {
                        mount
                            .agent_owner()
                            .is_none_or(|agent| config.sandbox.is_agent_enabled(agent))
                    })
                    .map(|mount| &mount.host),
            )
            .chain(config.tools.iter().map(|tool| &tool.path));
        for source in sources {
            let source = source.canonicalize().unwrap_or_else(|_| source.clone());
            if protected
                .iter()
                .any(|private| private.starts_with(&source) || source.starts_with(private))
            {
                return Err(io::Error::other(
                    "a sandbox mount would expose T3's host-only SSH keys or lifecycle control; use narrower mounts or a separate XDG_DATA_HOME/XDG_RUNTIME_DIR",
                ));
            }
        }
        Ok(())
    }
}
