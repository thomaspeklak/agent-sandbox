//! Incremental creation and update of the sandbox image.
//!
//! The image is assembled from independently cached components (OS
//! checkpoint, Rust, pnpm, vendor tools, Glimpse), each identified by a
//! content key over its real inputs. An update reuses every component whose
//! key is unchanged, verifies the assembled candidate offline, and only then
//! moves the configured tag. See `docs/COMMANDS.md` for the invalidation table.

mod assemble;
mod base;
mod cleanup;
mod components;
mod context;
mod engine;
mod error;
mod inputs;
mod lock;
mod metadata;
mod os;
mod pipeline;
mod platform;
mod publish;
mod report;
mod state;
mod vendor;

pub use cleanup::PreviousImageCleanup;
pub use error::ImageUpdateError;
pub use pipeline::{ImageSpec, UpdateRequest, normalize_image_name};
pub use report::{ImageOutcome, UpdateReport, VendorSummary};

/// Check for and apply updates to the configured image (`ags update-image`).
pub fn update(
    spec: &ImageSpec<'_>,
    request: UpdateRequest,
) -> Result<UpdateReport, ImageUpdateError> {
    pipeline::run(
        spec,
        UpdateRequest {
            create_only: false,
            ..request
        },
    )
}

/// Create the configured image if it is missing. An existing image is used
/// as-is: launching never checks for updates.
pub fn ensure_image(spec: &ImageSpec<'_>) -> Result<(), ImageUpdateError> {
    if engine::exists(&normalize_image_name(spec.image))? {
        return Ok(());
    }
    println!("Image {} not found; creating it", spec.image);
    let report = pipeline::run(
        spec,
        UpdateRequest {
            create_only: true,
            ..UpdateRequest::default()
        },
    )?;
    report.print();
    Ok(())
}
