use std::path::Path;

use crate::plan::LaunchPlan;

/// Build the complete `podman run` argument list from a launch plan.
///
/// The returned Vec does NOT include the `podman` binary itself — the caller
/// prepends it when spawning the process.
pub fn build_run_args(plan: &LaunchPlan, env_file: &Path) -> Vec<String> {
    let mut args: Vec<String> = Vec::with_capacity(64);

    // Base flags
    args.extend(["run", "--rm", "-it", "--pull=never"].map(into));
    args.push(format!("--userns={}", plan.security.userns));

    for opt in &plan.security.security_opts {
        args.push(format!("--security-opt={opt}"));
    }

    args.push(format!("--cap-drop={}", plan.security.cap_drop));
    args.push(format!("--pids-limit={}", plan.security.pids_limit));
    args.push("--network".into());
    args.push(plan.network_mode.clone());

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
        args.push(format!("{}:{}:{},z", m.host.display(), m.container, m.mode));
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

fn into(s: &str) -> String {
    s.to_owned()
}
