//! Publication of a verified candidate. The Podman tag and the state file
//! cannot change atomically together, so a pending record written before the
//! tag moves lets an interrupted run be reconciled under the lock.

use super::cleanup::{self, PreviousImageCleanup};
use super::context::{TagKind, component_tag};
use super::engine;
use super::error::ImageUpdateError;
use super::state::{self, ImageState, PendingPublication, STATE_SCHEMA, StatePaths};

/// How to finish an interrupted publication, given the configured image's
/// actual ID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Recovery {
    /// The verified candidate is live: commit its state.
    CommitCandidate,
    /// The tag never moved: keep the committed state.
    KeepPrevious,
    /// Something else changed the image; leave it alone.
    External(String),
}

pub fn decide_recovery(actual: Option<&str>, pending: &PendingPublication) -> Recovery {
    match actual {
        Some(id) if id == pending.candidate_id => Recovery::CommitCandidate,
        Some(id) if pending.previous_id.as_deref() == Some(id) => Recovery::KeepPrevious,
        None if pending.previous_id.is_none() => Recovery::KeepPrevious,
        Some(id) => Recovery::External(id.to_owned()),
        None => Recovery::External("no image".to_owned()),
    }
}

/// Reconcile an interrupted publication. Returns a note for the summary.
pub fn recover(
    output_id: &str,
    paths: &StatePaths,
    image: &str,
) -> Result<Option<String>, ImageUpdateError> {
    let state_error = |error: std::io::Error| ImageUpdateError::State(error.to_string());
    let pending = match state::load::<PendingPublication>(&paths.pending) {
        state::Loaded::Missing => return Ok(None),
        state::Loaded::Unusable(reason) => {
            state::remove_if_present(&paths.pending).map_err(state_error)?;
            return Ok(Some(format!(
                "discarded an unreadable interrupted-update record ({reason})"
            )));
        }
        state::Loaded::Found(pending) => pending,
    };
    let actual = engine::inspect(image)?.map(|info| info.id);
    let decision = decide_recovery(actual.as_deref(), &pending);
    if decision == Recovery::CommitCandidate {
        state::save_atomic(&paths.state, &pending.next).map_err(state_error)?;
    }
    state::remove_if_present(&paths.pending).map_err(state_error)?;
    let _ = engine::untag(&component_tag(output_id, "final", TagKind::Previous));
    let _ = engine::untag(&component_tag(output_id, "final", TagKind::Candidate));
    match decision {
        Recovery::CommitCandidate => {
            promote_tags(output_id, None, &pending.next);
            Ok(Some(format!(
                "completed an interrupted publication of {}",
                cleanup::short_image_id(&pending.candidate_id)
            )))
        }
        Recovery::KeepPrevious => Ok(Some(
            "discarded an interrupted update that had not been published".to_owned(),
        )),
        Recovery::External(found) => Err(ImageUpdateError::ExternalImageChange {
            image: image.to_owned(),
            found,
        }),
    }
}

pub struct Publication<'a> {
    pub output_id: &'a str,
    pub paths: &'a StatePaths,
    pub image: &'a str,
    pub previous_state: Option<&'a ImageState>,
    pub previous_id: Option<String>,
    pub candidate_id: String,
    pub next: ImageState,
    pub keep_existing: bool,
}

/// Move the configured tag to the verified candidate and commit its state.
pub fn publish(publication: Publication<'_>) -> Result<PreviousImageCleanup, ImageUpdateError> {
    let Publication {
        output_id,
        paths,
        image,
        previous_state,
        previous_id,
        candidate_id,
        next,
        keep_existing,
    } = publication;
    let previous_tag = component_tag(output_id, "final", TagKind::Previous);
    if let Some(previous) = &previous_id {
        engine::tag(previous, &previous_tag)?;
    }
    let pending = PendingPublication {
        schema: STATE_SCHEMA,
        previous_id: previous_id.clone(),
        candidate_id: candidate_id.clone(),
        next,
    };
    state::save_atomic(&paths.pending, &pending)
        .map_err(|error| ImageUpdateError::State(error.to_string()))?;

    if let Err(error) = engine::tag(&candidate_id, image) {
        // The configured tag did not move; the pending record is obsolete.
        let _ = state::remove_if_present(&paths.pending);
        let _ = engine::untag(&previous_tag);
        return Err(error);
    }
    let actual = engine::inspect(image)?.map(|info| info.id);
    if actual.as_deref() != Some(candidate_id.as_str()) {
        return Err(ImageUpdateError::StateAfterPublish(format!(
            "{image} does not point to the verified candidate after tagging"
        )));
    }

    let after_publish =
        |error: std::io::Error| ImageUpdateError::StateAfterPublish(error.to_string());
    state::save_atomic(&paths.state, &pending.next).map_err(after_publish)?;
    state::remove_if_present(&paths.pending).map_err(after_publish)?;

    promote_tags(output_id, previous_state, &pending.next);
    let _ = engine::untag(&component_tag(output_id, "final", TagKind::Candidate));
    let _ = engine::untag(&previous_tag);
    Ok(cleanup::remove_previous_image(
        previous_id.as_deref(),
        &candidate_id,
        keep_existing,
    ))
}

/// Components of a committed state as `(tag name, image ID)`.
pub fn component_images(state: &ImageState) -> Vec<(String, String)> {
    let mut images = vec![
        ("base".to_owned(), state.base.image_id.clone()),
        ("foundation".to_owned(), state.foundation.image_id.clone()),
        ("os-baseline".to_owned(), state.os.baseline_id.clone()),
        ("os".to_owned(), state.os.checkpoint_id.clone()),
        ("rust".to_owned(), state.rust.image_id.clone()),
        ("pnpm".to_owned(), state.pnpm.image_id.clone()),
        ("glimpse".to_owned(), state.glimpse.image_id.clone()),
    ];
    images.extend(
        state
            .vendor
            .iter()
            .map(|tool| (format!("vendor-{}", tool.install_as), tool.image_id.clone())),
    );
    images.retain(|(_, id)| !id.is_empty());
    images
}

/// Point `:current` names at the committed components and drop this run's
/// `:candidate` names. Superseded components lose their AGS name; Podman
/// keeps any still needed as parents. Failures here are harmless.
pub fn promote_tags(output_id: &str, previous: Option<&ImageState>, next: &ImageState) {
    let current = component_images(next);
    for (component, id) in &current {
        let _ = engine::tag(id, &component_tag(output_id, component, TagKind::Current));
        let _ = engine::untag(&component_tag(output_id, component, TagKind::Candidate));
    }
    for (component, _) in previous.map(component_images).unwrap_or_default() {
        if !current.iter().any(|(name, _)| *name == component) {
            let _ = engine::untag(&component_tag(output_id, &component, TagKind::Current));
        }
    }
}

#[cfg(test)]
#[path = "publish_tests.rs"]
mod tests;
