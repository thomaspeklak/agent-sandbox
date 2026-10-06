//! Fedora base policy: keep the numbered release, record its immutable
//! identity on first use, and change it only through `--rebase`.

use super::engine::{self, ImageInfo};
use super::error::ImageUpdateError;
use super::platform::Platform;
use super::state::BaseRecord;

pub const FEDORA_REPOSITORY: &str = "registry.fedoraproject.org/fedora";
/// Fedora release of the sandbox image. `--rebase` refreshes within it.
pub const FEDORA_RELEASE: &str = "44";

pub fn fedora_reference() -> String {
    format!("{FEDORA_REPOSITORY}:{FEDORA_RELEASE}")
}

/// How the base for this run was obtained, for the update summary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BaseOutcome {
    Retained,
    Initialized,
    Rebased { changed: bool },
}

impl BaseOutcome {
    pub fn describe(&self) -> String {
        let release = format!("Fedora {FEDORA_RELEASE}");
        match self {
            Self::Retained => format!("retained recorded {release} digest"),
            Self::Initialized => format!("recorded {release} digest"),
            Self::Rebased { changed: true } => format!("rebased to the current {release} digest"),
            Self::Rebased { changed: false } => {
                format!("rebase kept the same {release} digest; new OS lineage")
            }
        }
    }
}

/// What to do for the base, decided from the recorded identity alone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BasePlan {
    /// Pull the numbered release explicitly (`--rebase`).
    PullRelease,
    /// First use: reuse a local numbered image, pulling only if missing.
    InitializeMissingOnly,
    /// Use the recorded identity, pulling its exact digest if needed.
    Recorded(BaseRecord),
}

pub fn plan(recorded: Option<&BaseRecord>, rebase: bool) -> BasePlan {
    match recorded {
        _ if rebase => BasePlan::PullRelease,
        Some(record) if record.reference == fedora_reference() => {
            BasePlan::Recorded(record.clone())
        }
        _ => BasePlan::InitializeMissingOnly,
    }
}

pub fn resolve(
    recorded: Option<&BaseRecord>,
    rebase: bool,
    platform: &Platform,
) -> Result<(BaseRecord, BaseOutcome), ImageUpdateError> {
    let reference = fedora_reference();
    match plan(recorded, rebase) {
        BasePlan::PullRelease => {
            engine::pull(&reference).map_err(|message| ImageUpdateError::BaseUnavailable {
                reference: reference.clone(),
                message,
            })?;
            let record = record_from(&reference, required(&reference)?, platform)?;
            let changed = recorded.is_none_or(|old| old.image_id != record.image_id);
            Ok((record, BaseOutcome::Rebased { changed }))
        }
        BasePlan::InitializeMissingOnly => {
            if engine::inspect(&reference)?.is_none() {
                engine::pull(&reference).map_err(|message| ImageUpdateError::BaseUnavailable {
                    reference: reference.clone(),
                    message,
                })?;
            }
            let record = record_from(&reference, required(&reference)?, platform)?;
            Ok((record, BaseOutcome::Initialized))
        }
        BasePlan::Recorded(record) => {
            if engine::inspect(&record.image_id)?.is_none() {
                let pinned = format!("{FEDORA_REPOSITORY}@{}", record.digest);
                engine::pull(&pinned).map_err(|message| ImageUpdateError::BaseUnavailable {
                    reference: pinned.clone(),
                    message,
                })?;
            }
            let info = engine::inspect(&record.image_id)?.ok_or_else(|| {
                ImageUpdateError::BaseUnavailable {
                    reference: format!("{FEDORA_REPOSITORY}@{}", record.digest),
                    message: format!("pulling it did not produce image {}", record.image_id),
                }
            })?;
            check_platform(&info, platform)?;
            Ok((record, BaseOutcome::Retained))
        }
    }
}

fn required(reference: &str) -> Result<ImageInfo, ImageUpdateError> {
    engine::inspect(reference)?.ok_or_else(|| ImageUpdateError::BaseUnavailable {
        reference: reference.to_owned(),
        message: "image missing after pull".to_owned(),
    })
}

fn record_from(
    reference: &str,
    info: ImageInfo,
    platform: &Platform,
) -> Result<BaseRecord, ImageUpdateError> {
    check_platform(&info, platform)?;
    if !valid_digest(&info.digest) {
        return Err(ImageUpdateError::BaseUnavailable {
            reference: reference.to_owned(),
            message: format!(
                "local image has no registry digest ('{}'); remove it with `podman image rm {reference}` and retry",
                info.digest
            ),
        });
    }
    Ok(BaseRecord {
        reference: reference.to_owned(),
        digest: info.digest,
        image_id: info.id,
    })
}

pub fn valid_digest(digest: &str) -> bool {
    digest
        .strip_prefix("sha256:")
        .is_some_and(|hex| hex.len() == 64 && hex.bytes().all(|byte| byte.is_ascii_hexdigit()))
}

/// Reject a base whose platform differs from the Podman host.
pub fn check_platform(info: &ImageInfo, platform: &Platform) -> Result<(), ImageUpdateError> {
    if platform.matches_image(&info.os, &info.arch) {
        Ok(())
    } else {
        Err(ImageUpdateError::Platform(format!(
            "base image {} is {}/{}, but this Podman host builds {platform}",
            info.id, info.os, info.arch
        )))
    }
}

#[cfg(test)]
#[path = "base_tests.rs"]
mod tests;
