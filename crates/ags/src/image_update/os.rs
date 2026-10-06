//! OS checkpoints. The baseline holds the stable package selection; newly
//! available RPM updates are applied on top of the previous checkpoint, never
//! replayed from the baseline, and only package-state changes create one.

use std::collections::BTreeSet;

use super::context::{BuildContext, BuildStep, find_component};
use super::engine;
use super::error::ImageUpdateError;
use super::inputs::{self, sha256_hex};
use super::state::{BaseRecord, OsRecord};
use crate::podman::LayerCache;

/// Where the next OS checkpoint starts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Lineage {
    Continue(OsRecord),
    Restart(&'static str),
}

pub fn select_lineage(previous: Option<&OsRecord>, baseline_key: &str, rebase: bool) -> Lineage {
    match previous {
        None => Lineage::Restart("new OS baseline"),
        Some(_) if rebase => Lineage::Restart("rebased; new OS lineage"),
        Some(record) if record.baseline_key != baseline_key => {
            Lineage::Restart("package selection or base changed; rebuilt from baseline")
        }
        Some(record) => Lineage::Continue(record.clone()),
    }
}

/// `dnf check-upgrade`: 0 means current, 100 means updates, anything else fails.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckResult {
    Current,
    UpdatesAvailable,
    Failed,
}

pub fn classify_check(code: Option<i32>) -> CheckResult {
    match code {
        Some(0) => CheckResult::Current,
        Some(100) => CheckResult::UpdatesAvailable,
        _ => CheckResult::Failed,
    }
}

pub fn check_args(image_id: &str) -> Vec<String> {
    [
        "run",
        "--rm",
        "--pull=never",
        "--user=0",
        image_id,
        "dnf",
        "check-upgrade",
        "--refresh",
        "--setopt=skip_if_unavailable=False",
    ]
    .map(str::to_owned)
    .to_vec()
}

pub fn inventory_args(image_id: &str) -> Vec<String> {
    [
        "run",
        "--rm",
        "--pull=never",
        "--network=none",
        "--user=0",
        image_id,
        "rpm",
        "-qa",
        "--qf",
        "%{NAME}\\t%{EPOCHNUM}\\t%{VERSION}\\t%{RELEASE}\\t%{ARCH}\\n",
    ]
    .map(str::to_owned)
    .to_vec()
}

/// Installed RPMs as sorted `name epoch version release arch` lines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Inventory {
    pub entries: BTreeSet<String>,
}

impl Inventory {
    /// Byte-wise sorted, like `LC_ALL=C sort`.
    pub fn parse(stdout: &str) -> Self {
        Self {
            entries: stdout
                .lines()
                .map(str::trim)
                .filter(|line| !line.is_empty())
                .map(str::to_owned)
                .collect(),
        }
    }

    pub fn identity(&self) -> String {
        let joined: Vec<&str> = self.entries.iter().map(String::as_str).collect();
        sha256_hex(joined.join("\n").as_bytes())
    }

    /// Package versions present here but not in `older`.
    pub fn changed_since(&self, older: &Self) -> usize {
        self.entries.difference(&older.entries).count()
    }
}

fn inventory(image_id: &str) -> Result<Inventory, ImageUpdateError> {
    let output = engine::run_container(&inventory_args(image_id), None)?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(ImageUpdateError::build(
            "OS inventory",
            format!("rpm -qa exited with {} {stderr}", output.status),
        ));
    }
    Ok(Inventory::parse(&String::from_utf8_lossy(&output.stdout)))
}

fn check(image_id: &str) -> Result<CheckResult, ImageUpdateError> {
    println!("Checking for RPM updates");
    let output = engine::run_container(&check_args(image_id), None)?;
    match classify_check(output.status.code()) {
        CheckResult::Failed => {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
            Err(ImageUpdateError::Metadata {
                component: "RPM",
                message: format!("dnf check-upgrade exited with {} {stderr}", output.status)
                    .trim_end()
                    .to_owned(),
            })
        }
        result => Ok(result),
    }
}

pub struct OsResult {
    pub record: OsRecord,
    pub summary: String,
}

/// Select or build the matching lineage, check for RPM updates, and apply
/// them as a new checkpoint on top of the current one.
pub fn update(
    ctx: &BuildContext,
    base: &BaseRecord,
    packages: &[String],
    previous: Option<&OsRecord>,
    rebase: bool,
) -> Result<OsResult, ImageUpdateError> {
    let baseline_key = inputs::baseline_key(&ctx.platform, &base.image_id, packages);
    let lineage = match select_lineage(previous, &baseline_key, rebase) {
        Lineage::Continue(record) if checkpoint_usable(ctx, &record)? => Lineage::Continue(record),
        Lineage::Continue(_) => {
            Lineage::Restart("recorded checkpoint missing; rebuilt from baseline")
        }
        restart => restart,
    };
    let (mut record, mut summary) = match lineage {
        Lineage::Continue(record) => (record, "current; checkpoint reused".to_owned()),
        Lineage::Restart(reason) => {
            let baseline_id = baseline(ctx, base, packages, &baseline_key, previous)?;
            let found = inventory(&baseline_id)?;
            let record = OsRecord {
                baseline_key: baseline_key.clone(),
                baseline_id: baseline_id.clone(),
                checkpoint_id: baseline_id,
                inventory: found.identity(),
                packages: found.entries.len(),
                generation: 0,
            };
            (record, reason.to_owned())
        }
    };

    if check(&record.checkpoint_id)? == CheckResult::UpdatesAvailable {
        let before = inventory(&record.checkpoint_id)?;
        let candidate = ctx.build(BuildStep {
            component: "os",
            label_component: "os-checkpoint",
            key: &baseline_key,
            containerfile: &ctx.recipe("os-refresh.Containerfile"),
            context_dir: &ctx.recipe_dir(),
            build_args: &[("CHECKPOINT_IMAGE", record.checkpoint_id.clone())],
            cache: LayerCache::Rebuild,
        })?;
        let after = inventory(&candidate)?;
        if after == before {
            // Metadata-only result: keep the existing checkpoint.
            ctx.hold("os", &record.checkpoint_id)?;
            let _ = engine::remove_image(&candidate);
            summary.push_str("; update check found no package changes");
        } else {
            summary = format!(
                "{} package version(s) updated on checkpoint generation {}",
                after.changed_since(&before),
                record.generation + 1
            );
            record = OsRecord {
                checkpoint_id: candidate,
                inventory: after.identity(),
                packages: after.entries.len(),
                generation: record.generation + 1,
                ..record
            };
        }
    }
    ctx.hold("os", &record.checkpoint_id)?;
    Ok(OsResult { record, summary })
}

fn checkpoint_usable(ctx: &BuildContext, record: &OsRecord) -> Result<bool, ImageUpdateError> {
    Ok(engine::inspect(&record.checkpoint_id)?.is_some_and(|info| {
        ["os-baseline", "os-checkpoint"]
            .iter()
            .any(|component| info.is_component(component, &record.baseline_key, &ctx.platform))
    }))
}

fn baseline(
    ctx: &BuildContext,
    base: &BaseRecord,
    packages: &[String],
    key: &str,
    previous: Option<&OsRecord>,
) -> Result<String, ImageUpdateError> {
    let recorded = previous
        .filter(|record| record.baseline_key == key)
        .map(|record| record.baseline_id.as_str());
    if let Some(id) = find_component("os-baseline", key, &ctx.platform, recorded)? {
        ctx.hold("os-baseline", &id)?;
        return Ok(id);
    }
    ctx.build(BuildStep {
        component: "os-baseline",
        label_component: "os-baseline",
        key,
        containerfile: &ctx.recipe("os-baseline.Containerfile"),
        context_dir: &ctx.recipe_dir(),
        build_args: &[
            ("BASE_IMAGE", base.image_id.clone()),
            (
                "EXTRA_DNF_PACKAGES",
                inputs::canonical_packages(packages).join(" "),
            ),
        ],
        cache: LayerCache::Reuse,
    })
}

#[cfg(test)]
#[path = "os_tests.rs"]
mod tests;
