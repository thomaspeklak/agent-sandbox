use std::path::Path;

use crate::agent::AgentProfile;
use crate::cli::Agent;
use crate::clipboard::ClipboardGuard;
use crate::config::MountMode;
use crate::plan::PlanMount;

pub(super) fn add_mounts(
    runtime_dir: &Path,
    agent: Agent,
    mounts: &mut Vec<PlanMount>,
    profile: &mut AgentProfile,
) {
    mounts.push(PlanMount {
        host: runtime_dir.to_owned(),
        container: ClipboardGuard::container_runtime_dir().to_owned(),
        mode: MountMode::Rw,
    });
    let shim_host = runtime_dir.join(crate::clipboard::SHIM_NAME);
    for name in ["wl-paste", "wl-copy"] {
        mounts.push(PlanMount {
            host: shim_host.clone(),
            container: format!("/home/dev/.local/bin/{name}"),
            mode: MountMode::Ro,
        });
    }
    if agent == Agent::Pi {
        let container_dir = format!("{}/pi-extension", ClipboardGuard::container_runtime_dir());
        // The extension is session-scoped, not installed into the user's Pi home.
        mounts.push(PlanMount {
            host: runtime_dir.join("pi-extension"),
            container: container_dir.clone(),
            mode: MountMode::Ro,
        });
        profile
            .command_args
            .extend(["-e".to_owned(), format!("{container_dir}/index.ts")]);
    }
}
