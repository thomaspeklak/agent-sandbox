use super::registration::Registration;
use serde_json::Value;
use std::io;
use std::process::{Command, Output, Stdio};

pub fn output(args: &[&str]) -> io::Result<Output> {
    let output = Command::new("podman")
        .args(args)
        .stdin(Stdio::null())
        .output()?;
    if !output.status.success() {
        return Err(io::Error::other(format!(
            "podman {} failed: {}",
            args.first().unwrap_or(&""),
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    Ok(output)
}

pub fn inspect(registration: &Registration) -> io::Result<Option<Value>> {
    let exists = Command::new("podman")
        .args(["container", "exists", &registration.name()])
        .stdin(Stdio::null())
        .status()?;
    if exists.code() == Some(1) {
        return Ok(None);
    }
    if !exists.success() {
        return Err(io::Error::other("cannot inspect T3 container existence"));
    }
    let output = output(&["container", "inspect", &registration.name()])?;
    let values: Vec<Value> = serde_json::from_slice(&output.stdout)?;
    let value = values
        .into_iter()
        .next()
        .ok_or_else(|| io::Error::other("missing container inspection"))?;
    let uid = unsafe { libc::geteuid() }.to_string();
    if value["Config"]["Labels"]["io.ags.t3.repository"] != registration.repository.id
        || value["Config"]["Labels"]["io.ags.t3.uid"].as_str() != Some(uid.as_str())
        || value["Config"]["Labels"]["io.ags.t3.registration"].as_str()
            != registration.path()?.to_str()
    {
        return Err(io::Error::other(
            "T3 container name belongs to another environment; refusing reuse/removal",
        ));
    }
    Ok(Some(value))
}

pub fn stop(registration: &Registration) -> io::Result<()> {
    if let Some(container) = inspect(registration)?
        && container["State"]["Running"] == true
    {
        output(&["stop", "--time", "10", &inspected_id(&container)?])?;
    }
    Ok(())
}

fn inspected_id(container: &Value) -> io::Result<String> {
    let id = container["Id"]
        .as_str()
        .ok_or_else(|| io::Error::other("missing T3 container ID"))?;
    if id.len() != 64 || !id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(io::Error::other("invalid T3 container ID"));
    }
    Ok(id.to_owned())
}

pub fn id(registration: &Registration) -> io::Result<String> {
    inspected_id(
        &inspect(registration)?
            .ok_or_else(|| io::Error::other("T3 environment container is missing"))?,
    )
}

pub fn validate_layout(
    container: &Value,
    plan: &crate::plan::LaunchPlan,
    spec: &super::environment::Specification,
) -> io::Result<()> {
    let labels = &container["Config"]["Labels"];
    if labels["io.ags.t3.generation"].as_str() != spec.generation.to_str()
        || labels["io.ags.t3.configuration"] != spec.configuration
        || labels["io.ags.t3.mounts"] != spec.mounts
        || container["Image"] != spec.image
    {
        return Err(io::Error::other(
            "T3 container metadata differs from its registered environment; use explicit `ags t3 upgrade`",
        ));
    }
    let actual = container["Mounts"]
        .as_array()
        .ok_or_else(|| io::Error::other("missing T3 container mount inventory"))?;
    for expected in &plan.mounts {
        let source = expected
            .host
            .canonicalize()
            .unwrap_or_else(|_| expected.host.clone());
        let found = actual.iter().any(|mount| {
            let path = std::path::PathBuf::from(mount["Source"].as_str().unwrap_or(""));
            mount["Destination"] == expected.container
                && path.canonicalize().unwrap_or(path) == source
                && mount["RW"].as_bool() == Some(expected.mode == crate::config::MountMode::Rw)
        });
        if !found {
            return Err(io::Error::other(
                "T3 container bind mounts differ from registration; use explicit `ags t3 upgrade`",
            ));
        }
    }
    Ok(())
}

pub fn exec_output(registration: &Registration, args: &[&str]) -> io::Result<Output> {
    let id = id(registration)?;
    let mut command = vec!["exec", &id];
    command.extend_from_slice(args);
    output(&command)
}
