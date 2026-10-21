//! Host-side prepare hooks: no container or agent settings protocol.
pub mod cli;
mod declarations;
mod merge;
mod protocol;
mod runner;
mod strict_json;
mod trust;

pub use declarations::{Hook, parse_declarations};
pub use merge::{Contributions, Materialized};
pub use protocol::{Context, Response, validate_response};
pub use runner::{Limits, run};
pub use trust::TrustStore;

/// Approve all declarations before executing any host code.
pub fn prepare(hooks: &[Hook], context: &Context) -> Result<Contributions, String> {
    if hooks.is_empty() {
        return Contributions::merge(Vec::new());
    }
    let store = TrustStore::default();
    check_store_location(&store, context)?;
    for hook in hooks {
        if let Some(project) = &hook.project {
            check_store_outside_project(&store, project)?;
        }
        store.ensure(hook, true)?;
    }
    let _signals = crate::host_process::SignalGuard::install()?;
    let responses = run(hooks, context, &store, Limits::default())?;
    Contributions::merge(responses)
}

#[cfg(test)]
#[path = "hooks_tests.rs"]
mod tests;

/// Approvals must never become project-controlled files.
pub(crate) fn check_store_location(store: &TrustStore, context: &Context) -> Result<(), String> {
    check_store_outside_project(store, &context.project)
}
pub(crate) fn check_store_outside_project(
    store: &TrustStore,
    project: &std::path::Path,
) -> Result<(), String> {
    let project = project.canonicalize().map_err(|e| e.to_string())?;
    let mut ancestor = store.path.as_path();
    while !ancestor.exists() {
        ancestor = ancestor
            .parent()
            .ok_or("cannot locate trust storage ancestor")?;
    }
    let parent = ancestor.canonicalize().map_err(|e| e.to_string())?;
    if parent.starts_with(&project) {
        return Err(
            "hook approvals must be stored outside the project; relocate project or trust storage"
                .into(),
        );
    }
    Ok(())
}
