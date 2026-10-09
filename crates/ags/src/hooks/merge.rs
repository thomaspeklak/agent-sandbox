use super::Response;
use super::protocol::{BindMount, EnvValue, GeneratedFile, Mode, overlaps};
use crate::config::{MountMode, ValidatedConfig};
use crate::plan::{LaunchPlan, PlanMount};
use std::collections::{BTreeMap, HashMap};
use std::path::Path;

#[derive(Debug)]
enum Placement {
    File(GeneratedFile),
    Mount(BindMount),
}
#[derive(Debug, Default)]
pub struct Contributions {
    env: BTreeMap<String, EnvValue>,
    placements: BTreeMap<String, Placement>,
}
impl Contributions {
    /// Caller supplies global responses followed by project responses, in declaration order.
    pub fn merge(responses: Vec<Response>) -> Result<Self, String> {
        let mut merged = Self::default();
        for response in responses {
            response.validate()?;
            merged.env.extend(response.env);
            for file in response.files {
                merged
                    .placements
                    .insert(file.destination.clone(), Placement::File(file));
            }
            for mount in response.mounts {
                merged
                    .placements
                    .insert(mount.destination.clone(), Placement::Mount(mount));
            }
        }
        let destinations: Vec<_> = merged.placements.keys().collect();
        for (i, a) in destinations.iter().enumerate() {
            for b in &destinations[i + 1..] {
                if overlaps(Path::new(a), Path::new(b)) {
                    return Err(format!("ambiguous hook destinations {a:?} and {b:?}"));
                }
            }
        }
        Ok(merged)
    }
    /// Apply CLI precedence before materialization or secret lookup. Literal and secret
    /// contributions share exactly one keyspace. Never reveal response content in summaries.
    pub fn redact_summary(&self) -> serde_json::Value {
        serde_json::json!({"environment_keys": self.env.keys().collect::<Vec<_>>(),
            "secret_reference_keys": self.env.iter().filter_map(|(k,v)| matches!(v, EnvValue::Secret(_)).then_some(k)).collect::<Vec<_>>(),
            "destinations": self.placements.keys().collect::<Vec<_>>(), "values": "redacted"})
    }
    pub fn materialize(
        mut self,
        config: &mut ValidatedConfig,
        opts: &crate::cli::RunOptions,
    ) -> Result<Materialized, String> {
        for (key, _) in &opts.env {
            self.env.remove(key);
        }
        let cli_destinations = opts
            .add_dirs
            .iter()
            .map(|p| p.canonicalize().map_err(|e| format!("--add-dir: {e}")))
            .collect::<Result<Vec<_>, _>>()?;
        for destination in &cli_destinations {
            self.placements
                .remove(&destination.to_string_lossy().to_string());
        }
        if opts.lockdown
            && (self.env.values().any(|v| matches!(v, EnvValue::Secret(_)))
                || self
                    .placements
                    .values()
                    .any(|v| matches!(v, Placement::Mount(_))))
        {
            return Err("--lockdown rejects surviving prepare hook op references and host bind mounts; literal env and generated files are allowed".into());
        }
        for destination in self.placements.keys() {
            for cli in &cli_destinations {
                if overlaps(Path::new(destination), cli) {
                    return Err(format!(
                        "ambiguous hook/CLI mount overlap at {destination:?}"
                    ));
                }
            }
            if !opts.lockdown {
                for mount in &config.mounts {
                    if (mount.when == crate::config::MountWhen::Browser && !opts.browser)
                        || mount
                            .agent_owner()
                            .is_some_and(|a| !config.sandbox.is_agent_enabled(a))
                        || (mount.optional && !mount.create && !mount.host.exists())
                    {
                        continue;
                    }
                    if mount.container != *destination
                        && overlaps(Path::new(destination), Path::new(&mount.container))
                    {
                        return Err(format!(
                            "ambiguous hook/config mount overlap at {destination:?}"
                        ));
                    }
                }
            }
        }
        // Exact user-config destinations are defaults; hook replacements supersede them.
        config
            .mounts
            .retain(|m| !self.placements.contains_key(&m.container));
        config
            .sandbox
            .passthrough_env
            .retain(|key| !self.env.contains_key(key) && !opts.env.iter().any(|(k, _)| k == key));
        let dir = tempfile::Builder::new()
            .prefix("ags-hook-files-")
            .tempdir()
            .map_err(|e| e.to_string())?;
        let mut mounts = Vec::new();
        for (index, (destination, placement)) in self.placements.into_iter().enumerate() {
            let (host, mode) = match placement {
                Placement::File(file) => {
                    use std::io::Write;
                    use std::os::unix::fs::OpenOptionsExt;
                    let path = dir.path().join(index.to_string());
                    let mut out = std::fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .mode(0o600)
                        .open(&path)
                        .map_err(|e| e.to_string())?;
                    out.write_all(file.content.as_bytes())
                        .map_err(|e| e.to_string())?;
                    (path, MountMode::Ro)
                }
                Placement::Mount(mount) => {
                    let host = mount
                        .source
                        .canonicalize()
                        .map_err(|e| format!("hook mount source unavailable: {e}"))?;
                    if !host.is_file() && !host.is_dir() {
                        return Err("hook mounts support regular files and directories only".into());
                    }
                    let trust_path = super::TrustStore::default().path;
                    let trust = trust_path.canonicalize().unwrap_or(trust_path);
                    let cache = config
                        .sandbox
                        .cache_dir
                        .canonicalize()
                        .unwrap_or_else(|_| config.sandbox.cache_dir.clone());
                    let overlay_trust =
                        canonical_storage_path(&crate::trust::default_trust_store_path())?;
                    if overlaps(&host, &trust)
                        || overlaps(&host, &cache)
                        || overlaps(&host, &overlay_trust)
                    {
                        return Err("hook source exposes protected AGS host storage".into());
                    }
                    (
                        host,
                        match mount.mode {
                            Mode::Ro => MountMode::Ro,
                            Mode::Rw => MountMode::Rw,
                        },
                    )
                }
            };
            mounts.push(PlanMount {
                host,
                container: destination,
                mode,
            });
        }
        let mut env = Vec::new();
        let mut references = BTreeMap::new();
        for (key, value) in self.env {
            match value {
                EnvValue::Literal(value) => env.push((key, value)),
                EnvValue::Secret(reference) => {
                    references.insert(key, reference.op);
                }
            }
        }
        let overridden_keys = env
            .iter()
            .map(|(k, _)| k.clone())
            .chain(references.keys().cloned())
            .chain(opts.env.iter().map(|(k, _)| k.clone()))
            .collect();
        env.extend(opts.env.clone());
        Ok(Materialized {
            _dir: dir,
            mounts,
            env,
            references,
            overridden_keys,
        })
    }
}
// Resolve existing ancestors too: the trust file may not have been created yet,
// while a source alias already exposes its future location.
fn canonical_storage_path(path: &Path) -> Result<std::path::PathBuf, String> {
    match path.canonicalize() {
        Ok(path) => Ok(path),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // Do not treat a dangling symlink as a missing lexical component:
            // later trust writes would follow it somewhere else. Fail closed.
            if let Ok(meta) = std::fs::symlink_metadata(path)
                && meta.file_type().is_symlink()
            {
                return Err("cannot resolve protected AGS host storage symlink".into());
            }
            let parent = path.parent().ok_or("protected storage has no parent")?;
            let name = path
                .file_name()
                .ok_or("protected storage has no filename")?;
            Ok(canonical_storage_path(parent)?.join(name))
        }
        Err(e) => Err(format!("cannot resolve protected AGS host storage: {e}")),
    }
}
pub struct Materialized {
    _dir: tempfile::TempDir,
    pub mounts: Vec<PlanMount>,
    pub env: Vec<(String, String)>,
    pub references: BTreeMap<String, String>,
    overridden_keys: Vec<String>,
}
impl Materialized {
    pub fn filter_default_secrets(&self, secrets: &mut HashMap<String, String>) {
        for key in &self.overridden_keys {
            secrets.remove(key);
        }
    }
    /// Remove lower-precedence inherited/file values before rendering explicit values.
    pub fn finish_plan_env(&self, plan: &mut LaunchPlan) {
        plan.env
            .passthrough_names
            .retain(|key| !self.overridden_keys.contains(key));
        plan.env
            .env_file_entries
            .retain(|(key, _)| !self.overridden_keys.contains(key));
    }
    /// Verify actual plan placements, including AGS-owned mounts and workspace mappings.
    /// Existing plan builder also checks protected cache exposure before this check.
    pub fn validate_plan(&self, plan: &LaunchPlan) -> Result<(), String> {
        for hook in &self.mounts {
            for mount in &plan.mounts {
                if mount.container == hook.container && mount.host == hook.host {
                    continue;
                }
                if overlaps(Path::new(&mount.container), Path::new(&hook.container)) {
                    return Err(format!(
                        "ambiguous hook/effective-plan mount overlap at {:?}",
                        hook.container
                    ));
                }
            }
        }
        Ok(())
    }
    pub fn inject_secrets(&self, plan: &mut LaunchPlan) -> Result<(), String> {
        // Resolve once, only after the effective plan is validated and CLI losers removed.
        if self.references.is_empty() {
            return Ok(());
        }
        let _signals = crate::host_process::SignalGuard::install()?;
        let values = crate::onepassword_refs::resolve(&self.references)?;
        for key in self.references.keys() {
            plan.env.inline.retain(|(k, _)| k != key);
            plan.env.passthrough_names.retain(|k| k != key);
            plan.env.env_file_entries.retain(|(k, _)| k != key);
        }
        plan.env.env_file_entries.extend(values);
        Ok(())
    }
}
