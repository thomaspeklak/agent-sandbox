//! Final assembly from concrete component image IDs, plus the offline
//! verification a candidate must pass before publication.

use std::fs;

use super::context::{BuildContext, BuildStep};
use super::engine;
use super::error::ImageUpdateError;
use super::inputs::FinalInputs;
use crate::podman::LayerCache;

const VENDOR_STAGES: &str = "# @ags-vendor-stages@";
const VENDOR_COPIES: &str = "# @ags-vendor-copies@";

/// Replace the vendor markers with exactly the selected tools' stages and
/// copies; `vendor` holds `(install_as, image ID)` pairs.
pub fn render_final(template: &str, vendor: &[(String, String)]) -> Result<String, String> {
    for marker in [VENDOR_STAGES, VENDOR_COPIES] {
        if template.lines().filter(|line| *line == marker).count() != 1 {
            return Err(format!("final recipe must contain '{marker}' exactly once"));
        }
    }
    let stages: Vec<String> = vendor
        .iter()
        .enumerate()
        .map(|(index, (_, id))| format!("FROM {id} AS vendor-{index}"))
        .collect();
    let copies: Vec<String> = vendor
        .iter()
        .enumerate()
        .map(|(index, (command, _))| {
            format!("COPY --from=vendor-{index} /out/{command} /usr/local/bin/{command}")
        })
        .collect();
    let mut rendered = String::with_capacity(template.len());
    for line in template.lines() {
        let replacement = match line {
            VENDOR_STAGES => &stages,
            VENDOR_COPIES => &copies,
            _ => {
                rendered.push_str(line);
                rendered.push('\n');
                continue;
            }
        };
        for generated in replacement {
            rendered.push_str(generated);
            rendered.push('\n');
        }
    }
    Ok(rendered)
}

pub fn build_final(
    ctx: &BuildContext,
    inputs: &FinalInputs,
    key: &str,
) -> Result<String, ImageUpdateError> {
    let recipe = render_final(crate::assets::CONTAINERFILE, &inputs.vendor)
        .map_err(ImageUpdateError::Assets)?;
    let path = ctx.root().join("final.Containerfile");
    fs::write(&path, recipe).map_err(|error| ImageUpdateError::Assets(error.to_string()))?;
    ctx.build(BuildStep {
        component: "final",
        label_component: "final",
        key,
        containerfile: &path,
        context_dir: ctx.root(),
        build_args: &[
            ("OS_IMAGE", inputs.os_checkpoint.clone()),
            ("RUST_IMAGE", inputs.rust.clone()),
            ("PNPM_IMAGE", inputs.pnpm.clone()),
            ("GLIMPSE_IMAGE", inputs.glimpse.clone()),
        ],
        cache: LayerCache::Reuse,
    })
}

/// Values the candidate must report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Expectations {
    pub pnpm: String,
    pub rustc: String,
    pub rustup: String,
    pub rpms: Vec<String>,
    pub commands: Vec<String>,
}

/// Offline, mount-free smoke test of a candidate; the script arrives on stdin.
pub fn verify_args(candidate: &str, expected: &Expectations) -> Vec<String> {
    let mut args: Vec<String> = ["run", "--rm", "-i", "--pull=never", "--network=none"]
        .map(str::to_owned)
        .to_vec();
    for (name, value) in [
        ("EXPECT_PNPM_VERSION", expected.pnpm.clone()),
        ("EXPECT_RUSTC_VERSION", expected.rustc.clone()),
        ("EXPECT_RUSTUP_VERSION", expected.rustup.clone()),
        ("EXPECT_RPMS", expected.rpms.join(" ")),
        ("EXPECT_COMMANDS", expected.commands.join(" ")),
    ] {
        args.extend(["-e".to_owned(), format!("{name}={value}")]);
    }
    args.extend([candidate.to_owned(), "bash".to_owned(), "-s".to_owned()]);
    args
}

pub fn verify(candidate: &str, expected: &Expectations) -> Result<(), ImageUpdateError> {
    println!("Verifying candidate image offline");
    let script = crate::assets::image_recipe("verify-image.sh");
    let output = engine::run_container(&verify_args(candidate, expected), Some(script))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr);
    let detail: Vec<&str> = stderr.lines().rev().take(8).collect();
    let detail: Vec<&str> = detail.into_iter().rev().collect();
    Err(ImageUpdateError::Verification(format!(
        "exited with {}: {}",
        output.status,
        detail.join(" | ")
    )))
}

#[cfg(test)]
#[path = "assemble_tests.rs"]
mod tests;
