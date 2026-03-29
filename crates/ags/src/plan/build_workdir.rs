fn resolve_workdir(workdir: &Path) -> Result<WorkdirMapping, PlanError> {
    let host = fs::canonicalize(workdir)
        .map_err(|e| PlanError::WorkdirResolve(format!("{}: {e}", workdir.display())))?;
    // If caller passed an absolute path, preserve it as the container workdir;
    // otherwise use the resolved path.
    let container = if workdir.is_absolute() {
        workdir.to_string_lossy().to_string()
    } else {
        host.to_string_lossy().to_string()
    };
    Ok(WorkdirMapping { host, container })
}

fn build_container_name(workdir: &Path) -> String {
    let name_base =
        crate::git::worktree_parent_repo_dir(workdir).unwrap_or_else(|| workdir.to_path_buf());
    let short_path = short_path_slug(&name_base);
    let id = short_id4();
    format!("ags-{short_path}-{id}")
}

fn short_path_slug(path: &Path) -> String {
    let parts: Vec<_> = path
        .components()
        .filter_map(|c| match c {
            std::path::Component::Normal(os) => Some(os.to_string_lossy()),
            _ => None,
        })
        .collect();

    if parts.is_empty() {
        return "work".to_owned();
    }

    let tail = if parts.len() > 3 {
        &parts[parts.len() - 3..]
    } else {
        &parts
    };
    let raw = tail.join("-");
    let mut slug = String::with_capacity(raw.len());
    let mut prev_dash = false;
    for ch in raw.chars() {
        if ch.is_ascii_alphanumeric() {
            prev_dash = false;
            slug.push(ch.to_ascii_lowercase());
        } else if !prev_dash {
            prev_dash = true;
            slug.push('-');
        }
    }

    const MAX_SLUG_LEN: usize = 40;
    let slug = slug.trim_matches('-');
    let slug = if slug.len() > MAX_SLUG_LEN {
        slug[..MAX_SLUG_LEN].trim_matches('-')
    } else {
        slug
    };
    if slug.is_empty() {
        "work".to_owned()
    } else {
        slug.to_owned()
    }
}

fn short_id4() -> String {
    let now_nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    now_nanos.hash(&mut hasher);
    std::process::id().hash(&mut hasher);
    let digest = hasher.finish();

    format!("{:04x}", digest & 0xffff)
}

// --- directory helpers ---

fn ensure_dir(path: &Path) -> Result<(), PlanError> {
    fs::create_dir_all(path).map_err(|e| PlanError::DirCreate {
        path: path.to_owned(),
        source: e,
    })
}

fn create_mount_host(path: &Path, kind: MountKind) -> Result<(), PlanError> {
    match kind {
        MountKind::Dir => ensure_dir(path)?,
        MountKind::File => {
            if let Some(parent) = path.parent() {
                ensure_dir(parent)?;
            }
            if !path.exists() {
                fs::File::create(path).map_err(|e| PlanError::DirCreate {
                    path: path.to_owned(),
                    source: e,
                })?;
            }
        }
    }
    Ok(())
}

// --- mount assembly ---

fn add_infrastructure_mounts(
    mounts: &mut Vec<PlanMount>,
    config: &ValidatedConfig,
    cache_dir: &Path,
) {
    // Gitconfig
    mounts.push(PlanMount {
        host: config.sandbox.gitconfig_path.clone(),
        container: CONTAINER_GITCONFIG.to_owned(),
        mode: MountMode::Ro,
    });

    // Cache volumes
    for (suffix, container_path, _) in CACHE_MOUNTS {
        mounts.push(PlanMount {
            host: cache_dir.join(suffix),
            container: container_path.to_string(),
            mode: MountMode::Rw,
        });
    }
}

fn expand_config_mounts(
    config_mounts: &[ValidatedMount],
    browser_mode: bool,
    mounts: &mut Vec<PlanMount>,
    read_roots: &mut Vec<String>,
    write_roots: &mut Vec<String>,
) -> Result<(), PlanError> {
    for m in config_mounts {
        // Filter by when
        if m.when == MountWhen::Browser && !browser_mode {
            continue;
        }

        // Check host path existence
        if !m.host.exists() {
            if m.create {
                create_mount_host(&m.host, m.kind)?;
            } else if m.optional {
                continue;
            } else {
                return Err(PlanError::MountMissing {
                    host: m.host.clone(),
                    context: m.source.clone(),
                });
            }
        }

        mounts.push(PlanMount {
            host: m.host.clone(),
            container: m.container.clone(),
            mode: m.mode,
        });

        read_roots.push(m.container.clone());
        if m.mode == MountMode::Rw {
            write_roots.push(m.container.clone());
        }
    }
    Ok(())
}

fn add_plan_mounts(
    extra_mounts: &[PlanMount],
    mounts: &mut Vec<PlanMount>,
    read_roots: &mut Vec<String>,
    write_roots: &mut Vec<String>,
) {
    for m in extra_mounts {
        mounts.push(m.clone());
        if !read_roots.contains(&m.container) {
            read_roots.push(m.container.clone());
        }
        if m.mode == MountMode::Rw && !write_roots.contains(&m.container) {
            write_roots.push(m.container.clone());
        }
    }
}

fn add_runtime_dir_mounts(
    extra_mount_dirs: &[PathBuf],
    mounts: &mut Vec<PlanMount>,
    read_roots: &mut Vec<String>,
    write_roots: &mut Vec<String>,
) -> Result<(), PlanError> {
    for raw_dir in extra_mount_dirs {
        let host = fs::canonicalize(raw_dir).map_err(|_| PlanError::MountMissing {
            host: raw_dir.clone(),
            context: "runtime --add-dir mount".to_owned(),
        })?;
        if !host.is_dir() {
            return Err(PlanError::MountNotDir {
                host,
                context: "runtime --add-dir mount".to_owned(),
            });
        }

        let container = host.to_string_lossy().to_string();
        if mounts
            .iter()
            .any(|m| m.host == host && m.container == container)
        {
            continue;
        }

        mounts.push(PlanMount {
            host,
            container: container.clone(),
            mode: MountMode::Rw,
        });
        if !read_roots.contains(&container) {
            read_roots.push(container.clone());
        }
        if !write_roots.contains(&container) {
            write_roots.push(container);
        }
    }

    Ok(())
}

fn add_pub_key_mount(mounts: &mut Vec<PlanMount>, key_path: &Path, container_name: &str) {
    let mut pub_os = key_path.as_os_str().to_owned();
    pub_os.push(".pub");
    let pub_path = PathBuf::from(pub_os);

    let is_nonempty = pub_path
        .metadata()
        .map(|m| m.is_file() && m.len() > 0)
        .unwrap_or(false);

    if is_nonempty {
        mounts.push(PlanMount {
            host: pub_path,
            container: format!("{CONTAINER_HOME}/.ssh/{container_name}.pub"),
            mode: MountMode::Ro,
        });
    }
}

// --- wayland detection ---

struct WaylandInfo {
    socket_path: PathBuf,
    display_name: String,
}

fn detect_wayland() -> Result<Option<WaylandInfo>, PlanError> {
    if !clipboard_enabled()? {
        return Ok(None);
    }

    let runtime_dir = match std::env::var("XDG_RUNTIME_DIR") {
        Ok(d) if !d.is_empty() => d,
        _ => return Ok(None),
    };
    let display = match std::env::var("WAYLAND_DISPLAY") {
        Ok(d) if !d.is_empty() => d,
        _ => return Ok(None),
    };

    let socket_path = PathBuf::from(&runtime_dir).join(&display);

    #[cfg(unix)]
    {
        use std::os::unix::fs::FileTypeExt;
        let is_socket = socket_path
            .symlink_metadata()
            .map(|m| m.file_type().is_socket())
            .unwrap_or(false);
        if !is_socket {
            return Ok(None);
        }
    }
    #[cfg(not(unix))]
    if !socket_path.exists() {
        return Ok(None);
    }

    Ok(Some(WaylandInfo {
        socket_path,
        display_name: display,
    }))
}

fn clipboard_enabled() -> Result<bool, PlanError> {
    let raw = std::env::var("AGS_ENABLE_CLIPBOARD").unwrap_or_else(|_| "1".to_owned());
    match raw.to_lowercase().as_str() {
        "1" | "true" | "yes" | "on" => Ok(true),
        "0" | "false" | "no" | "off" => Ok(false),
        _ => Err(PlanError::InvalidEnv {
            var: "AGS_ENABLE_CLIPBOARD".to_owned(),
            value: raw,
        }),
    }
}

// --- environment assembly ---
