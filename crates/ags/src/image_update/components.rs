//! Build foundation and tool artifacts (Rust, pnpm, Glimpse). Each artifact is
//! reused whenever an image with its exact key exists; the foundation is only
//! built when some artifact actually needs building.

use super::context::{BuildContext, BuildStep, find_component};
use super::engine;
use super::error::ImageUpdateError;
use super::inputs;
use super::metadata::{PnpmRelease, RustRelease};
use super::state::{ArtifactRecord, BaseRecord, FoundationRecord, PnpmRecord, RustRecord};
use crate::podman::LayerCache;

pub struct Foundation {
    pub key: String,
    pub epoch: u32,
    base_id: String,
    recorded: Option<String>,
    refresh: bool,
    id: Option<String>,
}

impl Foundation {
    /// `--rebase` starts a new epoch, so the foundation is rebuilt without the
    /// layer cache and every artifact compiled against it is rebuilt.
    pub fn plan(
        ctx: &BuildContext,
        base: &BaseRecord,
        previous: Option<&FoundationRecord>,
        rebase: bool,
    ) -> Self {
        let epoch = previous.map_or(0, |record| record.epoch + u32::from(rebase));
        let key = inputs::foundation_key(&ctx.platform, &base.image_id, epoch);
        let recorded = previous
            .filter(|record| record.key == key && !record.image_id.is_empty())
            .map(|record| record.image_id.clone());
        Self {
            key,
            epoch,
            base_id: base.image_id.clone(),
            recorded,
            refresh: rebase,
            id: None,
        }
    }

    pub fn ensure(&mut self, ctx: &BuildContext) -> Result<String, ImageUpdateError> {
        if let Some(id) = &self.id {
            return Ok(id.clone());
        }
        let found = if self.refresh {
            None
        } else {
            find_component(
                "foundation",
                &self.key,
                &ctx.platform,
                self.recorded.as_deref(),
            )?
        };
        let id = match found {
            Some(id) => {
                ctx.hold("foundation", &id)?;
                id
            }
            None => ctx.build(BuildStep {
                component: "foundation",
                label_component: "foundation",
                key: &self.key,
                containerfile: &ctx.recipe("build-foundation.Containerfile"),
                context_dir: &ctx.recipe_dir(),
                build_args: &[("BASE_IMAGE", self.base_id.clone())],
                cache: if self.refresh {
                    LayerCache::Rebuild
                } else {
                    LayerCache::Reuse
                },
            })?,
        };
        self.id = Some(id.clone());
        Ok(id)
    }

    pub fn record(&self) -> FoundationRecord {
        FoundationRecord {
            key: self.key.clone(),
            image_id: self
                .id
                .clone()
                .or_else(|| self.recorded.clone())
                .unwrap_or_default(),
            epoch: self.epoch,
        }
    }
}

/// Reuse `recorded` when it still matches `key`, or any image carrying the key.
fn reusable(
    ctx: &BuildContext,
    component: &str,
    key: &str,
    recorded: Option<(&str, &str)>,
) -> Result<Option<String>, ImageUpdateError> {
    let recorded = recorded
        .filter(|(recorded_key, _)| *recorded_key == key)
        .map(|(_, id)| id);
    let found = find_component(component, key, &ctx.platform, recorded)?;
    if let Some(id) = &found {
        ctx.hold(component, id)?;
    }
    Ok(found)
}

pub fn rust(
    ctx: &BuildContext,
    foundation: &mut Foundation,
    previous: Option<&RustRecord>,
    release: &RustRelease,
    rustup: &str,
) -> Result<(RustRecord, [String; 2]), ImageUpdateError> {
    let key = inputs::rust_key(&ctx.platform, release, rustup);
    let record = |image_id: String| RustRecord {
        release: release.clone(),
        rustup: rustup.to_owned(),
        key: key.clone(),
        image_id,
    };
    let recorded = previous.map(|record| (record.key.as_str(), record.image_id.as_str()));
    if let Some(id) = reusable(ctx, "rust", &key, recorded)? {
        let summary = ["current; artifact reused".to_owned(), "current".to_owned()];
        return Ok((record(id), summary));
    }

    let foundation_id = foundation.ensure(ctx)?;
    // Seed from the previous artifact so an unchanged compiler is kept.
    let seed = match previous {
        Some(record) => engine::inspect(&record.image_id)?
            .filter(|info| {
                info.labels.get(engine::LABEL_COMPONENT).map(String::as_str) == Some("rust")
            })
            .filter(|info| ctx.platform.matches_image(&info.os, &info.arch))
            .map(|info| info.id),
        None => None,
    };
    let id = ctx.build(BuildStep {
        component: "rust",
        label_component: "rust",
        key: &key,
        containerfile: &ctx.recipe("rust.Containerfile"),
        context_dir: &ctx.recipe_dir(),
        build_args: &[
            ("FOUNDATION_IMAGE", foundation_id.clone()),
            ("RUST_SEED_IMAGE", seed.unwrap_or(foundation_id)),
            ("RUST_TRIPLE", ctx.platform.rust_triple().to_owned()),
            ("RUSTUP_VERSION", rustup.to_owned()),
            ("RUSTC_VERSION", release.rustc.clone()),
        ],
        cache: LayerCache::Reuse,
    })?;
    let compiler = match previous {
        Some(old) if old.release == *release => "current; artifact rebuilt".to_owned(),
        Some(_) => format!("updated to rustc {}", release.rustc),
        None => format!("installed rustc {}", release.rustc),
    };
    let manager = match previous {
        Some(old) if old.rustup == rustup => "current".to_owned(),
        Some(_) => format!("updated to {rustup}"),
        None => format!("installed {rustup}"),
    };
    Ok((record(id), [compiler, manager]))
}

pub fn pnpm(
    ctx: &BuildContext,
    foundation: &mut Foundation,
    previous: Option<&PnpmRecord>,
    release: &PnpmRelease,
) -> Result<(PnpmRecord, String), ImageUpdateError> {
    let key = inputs::pnpm_key(&ctx.platform, release);
    let record = |image_id: String| PnpmRecord {
        release: release.clone(),
        key: key.clone(),
        image_id,
    };
    let recorded = previous.map(|record| (record.key.as_str(), record.image_id.as_str()));
    if let Some(id) = reusable(ctx, "pnpm", &key, recorded)? {
        return Ok((record(id), "current; artifact reused".to_owned()));
    }
    let foundation_id = foundation.ensure(ctx)?;
    let id = ctx.build(BuildStep {
        component: "pnpm",
        label_component: "pnpm",
        key: &key,
        containerfile: &ctx.recipe("pnpm.Containerfile"),
        context_dir: &ctx.recipe_dir(),
        build_args: &[
            ("FOUNDATION_IMAGE", foundation_id),
            ("PNPM_VERSION", release.version.clone()),
            ("PNPM_SHA512", release.sha512.clone()),
        ],
        cache: LayerCache::Reuse,
    })?;
    let summary = match previous {
        Some(old) if old.release == *release => "current; artifact rebuilt".to_owned(),
        Some(_) => format!("updated to {}; artifact rebuilt", release.version),
        None => format!("installed {}", release.version),
    };
    Ok((record(id), summary))
}

pub fn glimpse(
    ctx: &BuildContext,
    foundation: &mut Foundation,
    previous: Option<&ArtifactRecord>,
    release: &RustRelease,
    rust_image: &str,
) -> Result<(ArtifactRecord, String), ImageUpdateError> {
    let key = inputs::glimpse_key(&ctx.platform, release, &foundation.key);
    let recorded = previous.map(|record| (record.key.as_str(), record.image_id.as_str()));
    if let Some(id) = reusable(ctx, "glimpse", &key, recorded)? {
        let record = ArtifactRecord { key, image_id: id };
        return Ok((record, "reused".to_owned()));
    }
    let foundation_id = foundation.ensure(ctx)?;
    let id = ctx.build(BuildStep {
        component: "glimpse",
        label_component: "glimpse",
        key: &key,
        containerfile: &ctx.recipe("glimpse.Containerfile"),
        context_dir: ctx.root(),
        build_args: &[
            ("FOUNDATION_IMAGE", foundation_id),
            ("RUST_IMAGE", rust_image.to_owned()),
        ],
        cache: LayerCache::Reuse,
    })?;
    Ok((ArtifactRecord { key, image_id: id }, "rebuilt".to_owned()))
}
