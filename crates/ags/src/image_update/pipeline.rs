//! The single creation/update path shared by `ags update-image` and the
//! creation of a missing image on launch. Components are resolved in
//! dependency order; nothing touches the configured image until a verified
//! candidate is published.

use super::assemble::{self, Expectations};
use super::base;
use super::cleanup::PreviousImageCleanup;
use super::components::{self, Foundation};
use super::context::{BuildContext, find_component};
use super::engine;
use super::error::ImageUpdateError;
use super::inputs::{self, FinalInputs};
use super::lock;
use super::metadata;
use super::os;
use super::publish::{self, Publication};
use super::report::{ImageOutcome, UpdateReport, VendorSummary};
use super::state::{
    self, ArtifactRecord, ImageState, Loaded, OutputIdentity, RustRecord, STATE_SCHEMA, StatePaths,
};
use super::vendor;
use crate::config::{BASE_DNF_PACKAGES, LockedToolDownload};

/// The image a configuration asks for.
#[derive(Debug, Clone, Copy)]
pub struct ImageSpec<'a> {
    pub image: &'a str,
    pub extra_dnf_packages: &'a [String],
    pub tool_downloads: &'a [LockedToolDownload],
}

#[derive(Debug, Clone, Copy, Default)]
pub struct UpdateRequest {
    /// Refresh the Fedora base and start a new OS lineage.
    pub rebase: bool,
    /// Keep the superseded final image instead of removing it.
    pub keep_existing: bool,
    /// Only create a missing image; an image that exists by the time the
    /// lock is held is left alone (first launch).
    pub create_only: bool,
}

/// Fully qualify an image name the way Podman names local builds, so
/// `agent-sandbox` and `localhost/agent-sandbox:latest` share one state.
pub fn normalize_image_name(image: &str) -> String {
    let image = image.trim();
    let has_registry = image.split_once('/').is_some_and(|(first, _)| {
        first.contains('.') || first.contains(':') || first == "localhost"
    });
    let mut name = if has_registry {
        image.to_owned()
    } else {
        format!("localhost/{image}")
    };
    let last = name.rsplit('/').next().unwrap_or_default();
    if !last.contains(':') && !last.contains('@') {
        name.push_str(":latest");
    }
    name
}

fn state_error(error: std::io::Error) -> ImageUpdateError {
    ImageUpdateError::State(error.to_string())
}

pub fn run(spec: &ImageSpec<'_>, request: UpdateRequest) -> Result<UpdateReport, ImageUpdateError> {
    let host = engine::host_info()?;
    let image = normalize_image_name(spec.image);
    let output = OutputIdentity {
        image: image.clone(),
        platform: host.platform,
        storage: host.graph_root,
    };
    let output_id = output.id();
    let paths = StatePaths::new(&state::state_root().map_err(state_error)?, &output);

    // Decide whether anything is needed only while holding the lock.
    let _lock = lock::acquire(&paths.lock, &image)?;
    let recovery = publish::recover(&output_id, &paths, &image)?;
    let current_id = engine::inspect(&image)?.map(|info| info.id);
    if request.create_only && current_id.is_some() {
        return Ok(UpdateReport {
            recovery,
            ..UpdateReport::created_elsewhere()
        });
    }
    let previous = match state::load_state(&paths.state, &output) {
        Loaded::Found(state) => Some(state),
        Loaded::Missing => None,
        Loaded::Unusable(reason) => {
            eprintln!("Ignoring unusable image update state ({reason}); rechecking components");
            None
        }
    };
    let selections = vendor::select(&output.platform, spec.tool_downloads)?;
    let ctx = BuildContext::new(output_id.clone(), output.platform.clone())?;

    let (base, base_outcome) = base::resolve(
        previous.as_ref().map(|state| &state.base),
        request.rebase,
        &ctx.platform,
    )?;
    let rust_release = metadata::resolve_rust(ctx.platform.rust_triple())?;
    let rustup = metadata::resolve_rustup()?;
    let pnpm_release = metadata::resolve_pnpm()?;

    let mut foundation = Foundation::plan(
        &ctx,
        &base,
        previous.as_ref().map(|state| &state.foundation),
        request.rebase,
    );
    let os = os::update(
        &ctx,
        &base,
        spec.extra_dnf_packages,
        previous.as_ref().map(|state| &state.os),
        request.rebase,
    )?;
    let (rust, [rust_summary, rustup_summary]) = rust_with_retry(
        &ctx,
        &mut foundation,
        previous.as_ref().map(|state| &state.rust),
        rust_release,
        rustup,
    )?;
    let (pnpm, pnpm_summary) = components::pnpm(
        &ctx,
        &mut foundation,
        previous.as_ref().map(|state| &state.pnpm),
        &pnpm_release,
    )?;
    let vendor = vendor::artifacts(
        &ctx,
        &mut foundation,
        previous.as_ref().map_or(&[][..], |state| &state.vendor),
        &selections,
    )?;
    let (glimpse, glimpse_summary) = components::glimpse(
        &ctx,
        &mut foundation,
        previous.as_ref().map(|state| &state.glimpse),
        &rust.release,
        &rust.image_id,
    )?;

    let final_inputs = FinalInputs {
        os_checkpoint: os.record.checkpoint_id.clone(),
        rust: rust.image_id.clone(),
        pnpm: pnpm.image_id.clone(),
        glimpse: glimpse.image_id.clone(),
        vendor: vendor
            .records
            .iter()
            .map(|tool| (tool.install_as.clone(), tool.image_id.clone()))
            .collect(),
    };
    let final_key = inputs::final_key(&final_inputs);
    let mut next = ImageState {
        schema: STATE_SCHEMA,
        output,
        base,
        foundation: foundation.record(),
        os: os.record,
        rust,
        pnpm,
        vendor: vendor.records,
        glimpse,
        final_image: ArtifactRecord {
            key: final_key.clone(),
            image_id: String::new(),
        },
    };
    let mut report = UpdateReport {
        recovery,
        base: base_outcome.describe(),
        os: os.summary,
        rust: rust_summary,
        rustup: rustup_summary,
        pnpm: pnpm_summary,
        vendor: VendorSummary {
            reused: vendor.reused,
            changed: vendor.built,
            removed: vendor.removed,
        },
        glimpse: glimpse_summary,
        image: ImageOutcome::Retained,
        cleanup: PreviousImageCleanup::NotNeeded,
    };

    let recorded_final = previous
        .as_ref()
        .filter(|state| state.final_image.key == final_key)
        .map(|state| state.final_image.image_id.as_str());
    if let Some(id) = recorded_final.filter(|id| current_id.as_deref() == Some(*id)) {
        // Complete no-op: the configured image already is this assembly.
        next.final_image.image_id = id.to_owned();
        commit_unchanged(&paths, &output_id, previous.as_ref(), &next)?;
        return Ok(report);
    }

    let candidate = match find_component("final", &final_key, &ctx.platform, recorded_final)? {
        Some(id) => {
            ctx.hold("final", &id)?;
            id
        }
        None => assemble::build_final(&ctx, &final_inputs, &final_key)?,
    };
    next.final_image.image_id = candidate.clone();
    if current_id.as_deref() == Some(candidate.as_str()) {
        // Only the bookkeeping was missing; the image is already current.
        commit_unchanged(&paths, &output_id, previous.as_ref(), &next)?;
        return Ok(report);
    }
    assemble::verify(&candidate, &expectations(&next, spec.extra_dnf_packages))?;
    report.cleanup = publish::publish(Publication {
        output_id: &output_id,
        paths: &paths,
        image: &image,
        previous_state: previous.as_ref(),
        previous_id: current_id,
        candidate_id: candidate.clone(),
        next,
        keep_existing: request.keep_existing,
    })?;
    report.image = ImageOutcome::Published {
        image_id: candidate,
    };
    Ok(report)
}

/// Build Rust; if the build fails because stable moved between metadata
/// resolution and installation, re-resolve and retry once.
fn rust_with_retry(
    ctx: &BuildContext,
    foundation: &mut Foundation,
    previous: Option<&RustRecord>,
    release: metadata::RustRelease,
    rustup: String,
) -> Result<(RustRecord, [String; 2]), ImageUpdateError> {
    match components::rust(ctx, foundation, previous, &release, &rustup) {
        Err(error @ ImageUpdateError::Build { .. }) => {
            let retry_release = metadata::resolve_rust(ctx.platform.rust_triple())?;
            let retry_rustup = metadata::resolve_rustup()?;
            if retry_release == release && retry_rustup == rustup {
                return Err(error);
            }
            println!("Rust stable changed during the build; retrying with the new release");
            components::rust(ctx, foundation, previous, &retry_release, &retry_rustup)
        }
        result => result,
    }
}

/// Record the (possibly refreshed) component state when the configured image
/// stays as it is.
fn commit_unchanged(
    paths: &StatePaths,
    output_id: &str,
    previous: Option<&ImageState>,
    next: &ImageState,
) -> Result<(), ImageUpdateError> {
    if previous != Some(next) {
        state::save_atomic(&paths.state, next).map_err(state_error)?;
    }
    publish::promote_tags(output_id, previous, next);
    Ok(())
}

pub fn expectations(state: &ImageState, extra_dnf_packages: &[String]) -> Expectations {
    let mut rpms: Vec<String> = BASE_DNF_PACKAGES
        .iter()
        .map(|name| (*name).to_owned())
        .collect();
    rpms.extend(inputs::canonical_packages(extra_dnf_packages));
    Expectations {
        pnpm: state.pnpm.release.version.clone(),
        rustc: state.rust.release.rustc.clone(),
        rustup: state.rust.rustup.clone(),
        rpms,
        commands: state
            .vendor
            .iter()
            .map(|tool| tool.install_as.clone())
            .collect(),
    }
}

#[cfg(test)]
#[path = "pipeline_tests.rs"]
mod tests;
