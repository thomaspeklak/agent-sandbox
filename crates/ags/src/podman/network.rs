use std::process::Command;

use crate::plan::LaunchPlan;

const PASTA_NETWORK_MODE: &str = "pasta";
const PODMAN_RUN_ERROR_EXIT_CODE: u8 = 125;

/// Adapt legacy rootless networking for Podman versions that removed
/// slirp4netns support.
pub(crate) fn adapt_network_mode_for_installed_podman(plan: &mut LaunchPlan) {
    let Ok(version) = podman_version() else {
        return;
    };

    let network_mode = network_mode_for_podman_version(&plan.network_mode, &version);
    if network_mode != plan.network_mode {
        eprintln!(
            "[ags] Podman {version} detected; using --network={network_mode} instead of {}",
            plan.network_mode
        );
        plan.network_mode = network_mode;
    }
}

fn network_mode_for_podman_version(current: &str, version: &str) -> String {
    if is_slirp4netns_mode(current) && podman_version_requires_pasta(version) {
        PASTA_NETWORK_MODE.to_owned()
    } else {
        current.to_owned()
    }
}

pub(crate) fn fallback_network_mode_after_run_failure(
    current: &str,
    exit_code: u8,
    failure_output: &str,
) -> Option<String> {
    if should_probe_network_mode_after_run_failure(current, exit_code)
        && is_slirp4netns_removed_error(failure_output)
    {
        Some(PASTA_NETWORK_MODE.to_owned())
    } else {
        None
    }
}

pub(crate) fn should_probe_network_mode_after_run_failure(current: &str, exit_code: u8) -> bool {
    exit_code == PODMAN_RUN_ERROR_EXIT_CODE && is_slirp4netns_mode(current)
}

fn is_slirp4netns_removed_error(output: &str) -> bool {
    output.contains("slirp4netns support has been removed")
        || (output.contains("slirp4netns") && output.contains("--network=pasta"))
}

fn podman_version() -> Result<String, std::io::Error> {
    let output = Command::new("podman")
        .args(["version", "--format", "{{.Version}}"])
        .output()?;

    if !output.status.success() {
        return Err(std::io::Error::other("podman version failed"));
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn is_slirp4netns_mode(network_mode: &str) -> bool {
    network_mode == "slirp4netns" || network_mode.starts_with("slirp4netns:")
}

fn podman_version_requires_pasta(version: &str) -> bool {
    let Some(major) = version.split(['.', '-']).next() else {
        return false;
    };
    major.parse::<u64>().is_ok_and(|major| major >= 6)
}

#[cfg(test)]
mod tests {
    use super::{fallback_network_mode_after_run_failure, network_mode_for_podman_version};

    #[test]
    fn keeps_slirp4netns_for_podman_5() {
        assert_eq!(
            network_mode_for_podman_version("slirp4netns:allow_host_loopback=false", "5.8.3"),
            "slirp4netns:allow_host_loopback=false"
        );
    }

    #[test]
    fn switches_slirp4netns_to_pasta_for_podman_6() {
        assert_eq!(
            network_mode_for_podman_version("slirp4netns:allow_host_loopback=false", "6.0.0"),
            "pasta"
        );
        assert_eq!(
            network_mode_for_podman_version("slirp4netns:allow_host_loopback=true", "6.0.0"),
            "pasta"
        );
    }

    #[test]
    fn leaves_non_slirp_network_modes_unchanged() {
        assert_eq!(network_mode_for_podman_version("host", "6.0.0"), "host");
        assert_eq!(network_mode_for_podman_version("pasta", "6.0.0"), "pasta");
    }

    #[test]
    fn keeps_legacy_mode_when_version_is_not_parseable() {
        assert_eq!(
            network_mode_for_podman_version("slirp4netns", "not-a-version"),
            "slirp4netns"
        );
    }

    #[test]
    fn falls_back_to_pasta_after_podman_run_error() {
        assert_eq!(
            fallback_network_mode_after_run_failure(
                "slirp4netns:allow_host_loopback=false",
                125,
                "Error: slirp4netns support has been removed, use --network=pasta instead"
            ),
            Some("pasta".to_owned())
        );
    }

    #[test]
    fn does_not_retry_without_network_failure_signal() {
        assert_eq!(
            fallback_network_mode_after_run_failure(
                "slirp4netns:allow_host_loopback=false",
                125,
                ""
            ),
            None
        );
        assert_eq!(
            fallback_network_mode_after_run_failure("slirp4netns:allow_host_loopback=false", 1, ""),
            None
        );
        assert_eq!(
            fallback_network_mode_after_run_failure(
                "pasta",
                125,
                "Error: slirp4netns support has been removed, use --network=pasta instead"
            ),
            None
        );
    }

    #[test]
    fn probes_only_slirp_podman_run_errors() {
        assert!(super::should_probe_network_mode_after_run_failure(
            "slirp4netns:allow_host_loopback=false",
            125
        ));
        assert!(!super::should_probe_network_mode_after_run_failure(
            "slirp4netns:allow_host_loopback=false",
            1
        ));
        assert!(!super::should_probe_network_mode_after_run_failure(
            "pasta", 125
        ));
    }
}
