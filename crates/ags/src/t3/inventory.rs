//! Read-only host registry discovery and live environment observations.
use super::{container, environment, owner, registration::Registration};
use serde_json::{Value, json};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub struct Entry {
    pub registration: Option<Registration>,
    pub status: Value,
}

pub fn registry() -> io::Result<PathBuf> {
    dirs::data_dir()
        .map(|root| root.join("ags/t3/registry"))
        .ok_or_else(|| io::Error::other("cannot locate T3 data directory"))
}

pub fn paths() -> io::Result<Vec<PathBuf>> {
    let entries = match fs::read_dir(registry()?) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut paths = Vec::new();
    for entry in entries {
        let path = entry?.path();
        if path
            .extension()
            .is_some_and(|extension| extension == "json")
        {
            paths.push(path);
        }
    }
    paths.sort();
    Ok(paths)
}

pub fn matches(path: &Path, selector: &str) -> bool {
    let id = path
        .file_stem()
        .and_then(|name| name.to_str())
        .unwrap_or("");
    selector == id
        || (id.len() == 64
            && id.bytes().all(|byte| byte.is_ascii_hexdigit())
            && selector == format!("ags-t3-{}", &id[..16]))
}

pub fn load() -> io::Result<Vec<Entry>> {
    Ok(paths()?.iter().map(|path| Entry::read(path)).collect())
}

impl Entry {
    pub fn read(path: &Path) -> Self {
        let result = (|| {
            if fs::symlink_metadata(path)?.file_type().is_symlink() {
                return Err(io::Error::other(
                    "T3 registry entry must not be a symbolic link",
                ));
            }
            Registration::read(path)
        })();
        match result {
            Ok(registration) => {
                let status = observe(&registration);
                Self {
                    registration: Some(registration),
                    status,
                }
            }
            Err(error) => {
                let id = path.file_stem().unwrap_or_default().to_string_lossy();
                let environment =
                    if id.len() == 64 && id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
                        format!("ags-t3-{}", &id[..16])
                    } else {
                        id.to_string()
                    };
                Self {
                    registration: None,
                    status: json!({"environment": environment, "repository_id": id,
                        "registration": path.display().to_string(), "repository": null, "ssh_alias": null,
                        "owner_running": false, "server_running": false,
                        "container_running": false, "running": false, "runtime": null,
                        "state": "unavailable", "error": error.to_string()}),
                }
            }
        }
    }

    pub fn working(&self) -> bool {
        self.registration.is_some()
            && self.status["running"] == true
            && self.status.get("error").is_none()
    }
}

fn observe(registration: &Registration) -> Value {
    let mut status = json!({"environment": registration.alias(),
        "repository_id": registration.repository.id, "repository": registration.repository.main,
        "ssh_alias": registration.alias(), "owner_running": false, "server_running": false,
        "container_running": false, "running": false, "runtime": null, "generation": null,
        "diagnostics": registration.private_dir().ok().map(|path| path.join("owner.log"))});
    let mut errors = Vec::new();
    match environment::specification(registration) {
        Ok(Some(spec)) => {
            status["runtime"] = json!(spec.version);
            status["generation"] = json!(spec.generation);
        }
        Ok(None) => {}
        Err(error) => errors.push(error.to_string()),
    }
    match owner::status(registration) {
        Ok(response) => match serde_json::from_str::<Value>(&response) {
            Ok(Value::Object(fields))
                if fields.get("running").and_then(Value::as_bool).is_some()
                    && fields.get("owner_pid").and_then(Value::as_u64).is_some() =>
            {
                status["server_running"] =
                    json!(fields.get("running").and_then(Value::as_bool) == Some(true));
                for (key, value) in fields {
                    if ["owner_pid", "runtime", "generation", "diagnostics"].contains(&key.as_str())
                    {
                        status[&key] = value;
                    }
                }
                status["owner_running"] = json!(true);
            }
            _ => errors.push("invalid T3 owner status response".into()),
        },
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::NotFound
                    | io::ErrorKind::ConnectionRefused
                    | io::ErrorKind::ConnectionReset
                    | io::ErrorKind::ConnectionAborted
                    | io::ErrorKind::BrokenPipe
                    | io::ErrorKind::UnexpectedEof
            ) => {}
        Err(error) => errors.push(format!("owner status unavailable: {error}")),
    }
    match container::inspect(registration) {
        Ok(Some(container)) => {
            status["container_running"] = json!(container["State"]["Running"] == true)
        }
        Ok(None) => {}
        Err(error) => errors.push(error.to_string()),
    }
    if status["container_running"] == true && status["server_running"] != true {
        errors.push("container is running without a responsive T3 server owner".into());
    } else if status["server_running"] == true && status["container_running"] != true {
        errors.push("owner reports a running server but its container is not running".into());
    }
    status["running"] = json!(
        errors.is_empty()
            && status["owner_running"] == true
            && status["server_running"] == true
            && status["container_running"] == true
    );
    status["state"] = json!(if !errors.is_empty() {
        "unavailable"
    } else if status["running"] == true {
        "running"
    } else {
        "stopped"
    });
    if !errors.is_empty() {
        status["error"] = json!(errors.join("; "));
    }
    status
}
