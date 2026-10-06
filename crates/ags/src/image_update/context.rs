//! Per-run build context: a private snapshot of the embedded recipes, the
//! AGS-owned tag namespace of one output, and component image lookup.

use std::path::{Path, PathBuf};

use super::engine::{self, LABEL_COMPONENT, LABEL_KEY};
use super::error::ImageUpdateError;
use super::platform::Platform;
use crate::podman::{ImageBuild, LayerCache, PullPolicy};

pub struct BuildContext {
    snapshot: tempfile::TempDir,
    output_id: String,
    pub platform: Platform,
}

impl BuildContext {
    /// Snapshot the embedded recipes into a private directory. Launch, doctor,
    /// and install only write the reference copy next to the configured
    /// Containerfile, so they cannot change this run's build inputs.
    pub fn new(output_id: String, platform: Platform) -> Result<Self, ImageUpdateError> {
        let assets = |error: std::io::Error| ImageUpdateError::Assets(error.to_string());
        let parent = crate::util::ags_cache_root()
            .map_err(assets)?
            .join("image-build");
        crate::util::ensure_private_dir(&parent).map_err(assets)?;
        let snapshot = tempfile::Builder::new()
            .prefix("run-")
            .tempdir_in(&parent)
            .map_err(assets)?;
        crate::assets::write_image_build_snapshot(snapshot.path()).map_err(assets)?;
        Ok(Self {
            snapshot,
            output_id,
            platform,
        })
    }

    pub fn root(&self) -> &Path {
        self.snapshot.path()
    }

    pub fn recipe_dir(&self) -> PathBuf {
        self.root().join("image")
    }

    pub fn recipe(&self, name: &str) -> PathBuf {
        self.recipe_dir().join(name)
    }

    /// Private name keeping a component of this output from being pruned.
    pub fn tag(&self, component: &str, which: TagKind) -> String {
        component_tag(&self.output_id, component, which)
    }

    /// Build one component under its private candidate tag and return its ID.
    pub fn build(&self, step: BuildStep<'_>) -> Result<String, ImageUpdateError> {
        let tag = self.tag(step.component, TagKind::Candidate);
        let iidfile = self.root().join(format!(".iid-{}", step.component));
        let labels = [
            (LABEL_COMPONENT, step.label_component.to_owned()),
            (LABEL_KEY, step.key.to_owned()),
        ];
        println!("Building {} image component", step.component);
        engine::build(
            step.component,
            &ImageBuild {
                containerfile: step.containerfile,
                context_dir: step.context_dir,
                tag: &tag,
                iidfile: &iidfile,
                // Every AGS build starts from already-resolved local images.
                pull: PullPolicy::Never,
                cache: step.cache,
                build_args: step.build_args,
                labels: &labels,
            },
        )
    }

    /// Tag a reused component as part of this run's candidate set.
    pub fn hold(&self, component: &str, id: &str) -> Result<(), ImageUpdateError> {
        engine::tag(id, &self.tag(component, TagKind::Candidate))
    }
}

pub struct BuildStep<'a> {
    /// Tag namespace entry, e.g. `rust` or `vendor-br`.
    pub component: &'a str,
    /// Value of the component label, e.g. `vendor`.
    pub label_component: &'a str,
    pub key: &'a str,
    pub containerfile: &'a Path,
    pub context_dir: &'a Path,
    pub build_args: &'a [(&'a str, String)],
    pub cache: LayerCache,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TagKind {
    /// Built or selected by the current (or last failed) run.
    Candidate,
    /// Committed component of the published image.
    Current,
    /// Previous final image, held until publication is committed.
    Previous,
}

pub fn component_tag(output_id: &str, component: &str, which: TagKind) -> String {
    let suffix = match which {
        TagKind::Candidate => "candidate",
        TagKind::Current => "current",
        TagKind::Previous => "previous",
    };
    format!("localhost/ags-build-{output_id}/{component}:{suffix}")
}

/// Find a valid local image for `component`/`key`, preferring the recorded
/// ID. Recorded IDs are re-inspected: state alone never proves an image exists.
pub fn find_component(
    component: &str,
    key: &str,
    platform: &Platform,
    recorded: Option<&str>,
) -> Result<Option<String>, ImageUpdateError> {
    if let Some(id) = recorded.filter(|id| !id.is_empty())
        && let Some(info) = engine::inspect(id)?
        && info.is_component(component, key, platform)
    {
        return Ok(Some(info.id));
    }
    for id in engine::images_with_key(key)? {
        if let Some(info) = engine::inspect(&id)?
            && info.is_component(component, key, platform)
        {
            return Ok(Some(info.id));
        }
    }
    Ok(None)
}
