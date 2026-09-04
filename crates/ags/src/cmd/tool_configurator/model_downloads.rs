use std::collections::BTreeSet;

use crate::config::{GitHubReleaseSource, LockedToolDownload, ToolDownloadSource};

use super::{ToolConfigError, ToolSelectionState, ToolState};

impl ToolSelectionState {
    pub(super) fn selected_downloads(
        &self,
        minimum_release_age: u32,
    ) -> Result<Vec<LockedToolDownload>, ToolConfigError> {
        let catalog_ids = self
            .tools
            .iter()
            .map(|tool| tool.definition.id.as_str())
            .collect::<BTreeSet<_>>();
        let selected_commands = self
            .tools
            .iter()
            .filter(|tool| tool.selected)
            .filter_map(|tool| {
                tool.definition
                    .download
                    .as_ref()
                    .map(|download| download.install_as.as_str())
                    .or_else(|| {
                        tool.definition
                            .github_release
                            .as_ref()
                            .map(|source| source.install_as.as_str())
                    })
            })
            .collect::<BTreeSet<_>>();
        let mut downloads = Vec::new();
        for tool in self.tools.iter().filter(|tool| tool.selected) {
            let download = if let Some(download) = &tool.definition.download {
                download.clone()
            } else if let Some(source) = &tool.definition.github_release {
                self.locked_or_resolved_release(tool, source, minimum_release_age)?
            } else {
                continue;
            };
            downloads.push(LockedToolDownload {
                id: tool.definition.id.clone(),
                download,
            });
        }
        downloads.extend(
            self.configured_downloads
                .iter()
                .filter(|tool| {
                    !catalog_ids.contains(tool.id.as_str())
                        && !selected_commands.contains(tool.download.install_as.as_str())
                })
                .cloned(),
        );
        Ok(downloads)
    }

    /// Untouched tools keep their locked release so an unrelated save works
    /// offline and never silently bumps a pinned version.
    fn locked_or_resolved_release(
        &self,
        tool: &ToolState,
        source: &GitHubReleaseSource,
        minimum_release_age: u32,
    ) -> Result<ToolDownloadSource, ToolConfigError> {
        if !tool.touched
            && let Some(locked) = self.configured_downloads.iter().find(|entry| {
                entry.id == tool.definition.id && entry.download.install_as == source.install_as
            })
        {
            return Ok(locked.download.clone());
        }
        crate::github_release::resolve_github_release_source(source, minimum_release_age).map_err(
            |error| ToolConfigError::ReleaseResolve {
                item: format!("tool '{}'", tool.definition.id),
                message: error.to_string(),
            },
        )
    }
}
