use std::collections::HashSet;
use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::cli::Agent;
use crate::config::ValidatedConfig;

use crate::cmd::doctor_util::Checker;

pub(super) fn check_agent_runtimes(ck: &mut Checker, config: &ValidatedConfig) {
    ck.section("Agent CLIs");
    let cache = &config.sandbox.cache_dir;
    for agent in &config.sandbox.enabled_agents {
        let binary = match agent {
            Agent::Pi => cache.join("pnpm-home/bin/pi"),
            Agent::Claude => cache.join("claude-install/.local/bin/claude"),
            Agent::Codex => cache.join("pnpm-home/codex"),
            Agent::Gemini => cache.join("pnpm-home/bin/gemini"),
            Agent::Opencode => cache.join("pnpm-home/bin/opencode"),
            Agent::Shell => continue,
        };
        let label = format!("{} runtime", agent.display_name());
        let present = if *agent == Agent::Codex {
            codex_runtime_present(cache)
        } else {
            binary.exists()
        };
        if present {
            ck.ok(&format!("{label} present: {}", binary.display()));
        } else {
            ck.warn(&format!("{label} missing: {}", binary.display()));
        }
    }
    if config.sandbox.enabled_agents.is_empty() {
        ck.ok("no agent CLIs enabled; shell remains available");
    }
}

fn codex_runtime_present(cache: &Path) -> bool {
    let pnpm_home = cache.join("pnpm-home");
    let codex_home = cache.join("codex-install");
    let mounts = [
        (Path::new("/usr/local/pnpm"), pnpm_home.as_path()),
        (Path::new("/opt/codex-home"), codex_home.as_path()),
    ];
    resolve_mounted_path(Path::new("/usr/local/pnpm/codex"), &mounts)
        .is_some_and(|path| path.is_file() && crate::util::is_executable(&path))
}

fn resolve_mounted_path(path: &Path, mounts: &[(&Path, &Path)]) -> Option<PathBuf> {
    let mut container_path = normalize_container_path(path)?;
    let mut seen = HashSet::new();

    for _ in 0..32 {
        if !seen.insert(container_path.clone()) {
            return None;
        }
        let (container_root, host_root, relative) =
            mounts.iter().find_map(|(container_root, host_root)| {
                container_path
                    .strip_prefix(container_root)
                    .ok()
                    .map(|relative| (*container_root, *host_root, relative.to_owned()))
            })?;
        let components = relative.components().collect::<Vec<_>>();
        let mut container_cursor = container_root.to_owned();
        let mut host_cursor = host_root.to_owned();
        let mut followed_symlink = false;

        for (index, component) in components.iter().enumerate() {
            let Component::Normal(name) = component else {
                return None;
            };
            container_cursor.push(name);
            host_cursor.push(name);
            let metadata = fs::symlink_metadata(&host_cursor).ok()?;
            if !metadata.file_type().is_symlink() {
                continue;
            }

            let target = fs::read_link(&host_cursor).ok()?;
            let mut next = if target.is_absolute() {
                normalize_container_path(&target)?
            } else {
                normalize_container_path(&container_cursor.parent()?.join(target))?
            };
            for remaining in &components[index + 1..] {
                let Component::Normal(name) = remaining else {
                    return None;
                };
                next.push(name);
            }
            container_path = normalize_container_path(&next)?;
            followed_symlink = true;
            break;
        }

        if !followed_symlink {
            return Some(host_cursor);
        }
    }
    None
}

fn normalize_container_path(path: &Path) -> Option<PathBuf> {
    if !path.is_absolute() {
        return None;
    }
    let mut normalized = PathBuf::from("/");
    for component in path.components() {
        match component {
            Component::RootDir | Component::CurDir => {}
            Component::ParentDir => {
                if !normalized.pop() {
                    return None;
                }
            }
            Component::Normal(name) => normalized.push(name),
            Component::Prefix(_) => return None,
        }
    }
    Some(normalized)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::{PermissionsExt, symlink};

    use super::codex_runtime_present;

    #[test]
    fn official_codex_absolute_symlinks_are_resolved_through_runtime_mounts() {
        let dir = tempfile::tempdir().unwrap();
        let cache = dir.path();
        let pnpm_home = cache.join("pnpm-home");
        let release_bin = cache.join("codex-install/packages/standalone/releases/v1/bin");
        let current = cache.join("codex-install/packages/standalone/current");
        fs::create_dir_all(&pnpm_home).unwrap();
        fs::create_dir_all(&release_bin).unwrap();
        let codex = release_bin.join("codex");
        fs::write(&codex, "#!/bin/sh\n").unwrap();
        fs::set_permissions(&codex, fs::Permissions::from_mode(0o755)).unwrap();
        symlink("/opt/codex-home/packages/standalone/releases/v1", &current).unwrap();
        symlink(
            "/opt/codex-home/packages/standalone/current/bin/codex",
            pnpm_home.join("codex"),
        )
        .unwrap();

        assert!(codex_runtime_present(cache));
    }

    #[test]
    fn codex_launcher_with_missing_target_is_not_present() {
        let dir = tempfile::tempdir().unwrap();
        let pnpm_home = dir.path().join("pnpm-home");
        fs::create_dir_all(&pnpm_home).unwrap();
        symlink(
            "/opt/codex-home/packages/standalone/current/bin/codex",
            pnpm_home.join("codex"),
        )
        .unwrap();

        assert!(!codex_runtime_present(dir.path()));
    }

    #[test]
    fn codex_launcher_outside_runtime_mounts_is_not_present() {
        let dir = tempfile::tempdir().unwrap();
        let pnpm_home = dir.path().join("pnpm-home");
        fs::create_dir_all(&pnpm_home).unwrap();
        symlink("/tmp/unmanaged-codex", pnpm_home.join("codex")).unwrap();

        assert!(!codex_runtime_present(dir.path()));
    }
}
