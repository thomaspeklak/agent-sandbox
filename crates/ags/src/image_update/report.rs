//! Compact explanation of what an update decided, one line per component.

use super::cleanup::PreviousImageCleanup;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageOutcome {
    /// Nothing changed; the configured image was left as it was.
    Retained,
    /// A verified candidate replaced (or created) the configured image.
    Published { image_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct VendorSummary {
    pub reused: usize,
    pub changed: usize,
    pub removed: usize,
}

impl VendorSummary {
    pub fn describe(&self) -> String {
        if self.reused + self.changed + self.removed == 0 {
            return "none selected".to_owned();
        }
        let mut parts = vec![format!("{} reused", self.reused)];
        if self.changed > 0 {
            parts.push(format!("{} changed", self.changed));
        }
        if self.removed > 0 {
            parts.push(format!("{} removed", self.removed));
        }
        parts.join(", ")
    }
}

#[derive(Debug)]
pub struct UpdateReport {
    pub recovery: Option<String>,
    pub base: String,
    pub os: String,
    pub rust: String,
    pub rustup: String,
    pub pnpm: String,
    pub vendor: VendorSummary,
    pub glimpse: String,
    pub image: ImageOutcome,
    pub cleanup: PreviousImageCleanup,
}

impl UpdateReport {
    /// A report for a creation that another process completed while this one
    /// waited for the lock.
    pub fn created_elsewhere() -> Self {
        let concurrent = "created by a concurrent AGS run".to_owned();
        Self {
            recovery: None,
            base: concurrent.clone(),
            os: concurrent.clone(),
            rust: concurrent.clone(),
            rustup: concurrent.clone(),
            pnpm: concurrent.clone(),
            vendor: VendorSummary::default(),
            glimpse: concurrent,
            image: ImageOutcome::Retained,
            cleanup: PreviousImageCleanup::NotNeeded,
        }
    }

    pub fn lines(&self) -> Vec<String> {
        let image = match &self.image {
            ImageOutcome::Retained => "no changes; existing image retained".to_owned(),
            ImageOutcome::Published { image_id } => format!(
                "verified and published {}",
                super::cleanup::short_image_id(image_id)
            ),
        };
        let mut lines: Vec<String> = self
            .recovery
            .iter()
            .map(|note| format!("Recovery:   {note}"))
            .collect();
        lines.extend([
            format!("Base:       {}", self.base),
            format!("OS:         {}", self.os),
            format!("Rust:       {}", self.rust),
            format!("rustup:     {}", self.rustup),
            format!("pnpm:       {}", self.pnpm),
            format!("Vendor:     {}", self.vendor.describe()),
            format!("Glimpse:    {}", self.glimpse),
            format!("Image:      {image}"),
        ]);
        if let Some(cleanup) = self.cleanup.describe() {
            lines.push(format!("Cleanup:    {cleanup}"));
        }
        lines
    }

    pub fn print(&self) {
        for line in self.lines() {
            println!("{line}");
        }
    }
}

#[cfg(test)]
#[path = "report_tests.rs"]
mod tests;
