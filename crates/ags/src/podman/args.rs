use std::path::Path;

use crate::plan::LaunchPlan;

/// Registry pull behavior for `podman build`. AGS always passes it explicitly
/// instead of relying on Podman's default or a bare `--pull`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PullPolicy {
    Always,
    Missing,
    Never,
}

impl PullPolicy {
    fn flag(self) -> &'static str {
        match self {
            Self::Always => "--pull=always",
            Self::Missing => "--pull=missing",
            Self::Never => "--pull=never",
        }
    }
}

/// Whether a build may reuse Podman's layer cache.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LayerCache {
    /// Normal builds: `--layers=true`.
    Reuse,
    /// Tiny recipes that must execute (OS refresh, rebase): adds `--no-cache`.
    Rebuild,
}

/// One `podman build` invocation for an AGS image component.
#[derive(Debug, Clone)]
pub struct ImageBuild<'a> {
    pub containerfile: &'a Path,
    pub context_dir: &'a Path,
    pub tag: &'a str,
    pub iidfile: &'a Path,
    pub pull: PullPolicy,
    pub cache: LayerCache,
    pub build_args: &'a [(&'a str, String)],
    pub labels: &'a [(&'a str, String)],
}

pub fn build_image_args(build: &ImageBuild<'_>) -> Vec<String> {
    let mut args = vec![
        "build".to_owned(),
        "--layers=true".to_owned(),
        build.pull.flag().to_owned(),
    ];
    if build.cache == LayerCache::Rebuild {
        args.push("--no-cache".to_owned());
    }
    args.extend([
        "-f".to_owned(),
        build.containerfile.display().to_string(),
        "-t".to_owned(),
        build.tag.to_owned(),
        "--iidfile".to_owned(),
        build.iidfile.display().to_string(),
    ]);
    for (name, value) in build.labels {
        args.extend(["--label".to_owned(), format!("{name}={value}")]);
    }
    for (name, value) in build.build_args {
        args.extend(["--build-arg".to_owned(), format!("{name}={value}")]);
    }
    args.push(build.context_dir.display().to_string());
    args
}

/// Build the complete `podman run` argument list from a launch plan.
///
/// The returned Vec does NOT include the `podman` binary itself — the caller
/// prepends it when spawning the process.
pub fn build_run_args(plan: &LaunchPlan, env_file: &Path) -> Vec<String> {
    let mut args: Vec<String> = Vec::with_capacity(64);

    // Base flags
    // Keep attached terminal output, but never persist TUI redraws or session
    // contents through the host's default logging driver (often journald).
    args.extend(["run", "--rm", "-it", "--pull=never", "--log-driver=none"].map(String::from));
    if let Some(ref userns) = plan.security.userns {
        args.push(format!("--userns={userns}"));
    }
    if let Some(ref user) = plan.security.user {
        args.push(format!("--user={user}"));
    }

    for opt in &plan.security.security_opts {
        args.push(format!("--security-opt={opt}"));
    }

    if let Some(ref cap_drop) = plan.security.cap_drop {
        args.push(format!("--cap-drop={cap_drop}"));
    }
    args.push(format!("--pids-limit={}", plan.security.pids_limit));
    for tmpfs in &plan.security.tmpfs {
        args.push("--tmpfs".into());
        args.push(tmpfs.clone());
    }
    args.push("--network".into());
    args.push(plan.network_mode.clone());

    // Anonymous descriptors are inherited directly, never serialized into args.
    if plan.payload_fd_count > 0 {
        args.push(format!("--preserve-fds={}", plan.payload_fd_count));
    }

    // Container name
    args.push("--name".into());
    args.push(plan.container_name.clone());

    // Inline environment variables
    for (key, value) in &plan.env.inline {
        args.push("-e".into());
        args.push(format!("{key}={value}"));
    }

    // Passthrough env vars (inherit from host by name)
    for name in &plan.env.passthrough_names {
        args.push("-e".into());
        args.push(name.clone());
    }

    // Guard roots
    args.push("-e".into());
    args.push(format!(
        "AGS_GUARD_READ_ROOTS_JSON={}",
        plan.env.read_roots_json
    ));
    args.push("-e".into());
    args.push(format!(
        "AGS_GUARD_WRITE_ROOTS_JSON={}",
        plan.env.write_roots_json
    ));

    // Env file
    args.push("--env-file".into());
    args.push(env_file.to_string_lossy().into_owned());

    // Mounts — first mount is the workdir, render with -w
    let mut first = true;
    for m in &plan.mounts {
        args.push("-v".into());
        args.push(format!("{}:{}:{}", m.host.display(), m.container, m.mode));
        if first {
            args.push("-w".into());
            args.push(plan.workdir.container.clone());
            first = false;
        }
    }

    // Image
    args.push(plan.image.clone());

    // Entrypoint: bash -lc "<script>" _ <passthrough_args>
    args.push("bash".into());
    args.push("-lc".into());
    args.push(plan.entrypoint.clone());
    args.push("_".into());

    args
}
