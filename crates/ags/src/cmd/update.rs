//! `ags update-image`: check for and apply updates through the incremental
//! image pipeline, reusing every component whose inputs are unchanged.

use std::fmt;

use crate::config::ValidatedConfig;
use crate::image_update::{self, ImageSpec, ImageUpdateError, UpdateRequest};

/// Options for the update command.
#[derive(Debug, Clone, Copy, Default)]
pub struct UpdateOptions {
    pub keep_existing: bool,
    pub rebase: bool,
}

/// An update failure, stating whether the existing image was retained.
#[derive(Debug)]
pub struct UpdateError(pub ImageUpdateError);

impl fmt::Display for UpdateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)?;
        if self.0.preserved_existing_image() {
            write!(
                f,
                "\nThe existing image and its update state were retained."
            )?;
        }
        Ok(())
    }
}

impl std::error::Error for UpdateError {}

pub fn run(config: &ValidatedConfig, opts: &UpdateOptions) -> Result<(), UpdateError> {
    let sandbox = &config.sandbox;
    let spec = ImageSpec {
        image: &sandbox.image,
        extra_dnf_packages: &sandbox.extra_dnf_packages,
        tool_downloads: &sandbox.tool_downloads,
    };
    let request = UpdateRequest {
        rebase: opts.rebase,
        keep_existing: opts.keep_existing,
        create_only: false,
    };
    println!("Updating {}", sandbox.image);
    let report = image_update::update(&spec, request).map_err(UpdateError)?;
    println!();
    report.print();
    println!("\nRun 'ags update-agents' to install/update agent CLIs in volumes.");
    Ok(())
}
