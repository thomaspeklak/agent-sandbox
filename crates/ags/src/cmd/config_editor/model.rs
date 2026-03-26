// Layered config data model for the Visual Config Editor.
//
// Owns two independent toml_edit::DocumentMut instances (global + local overlay)
// and computes a merged view for display. All edits target a single selected
// document; the other is never modified.
//
// See: docs/CONFIG_EDITOR_TOML_RESEARCH.md (architecture notes)

use std::fs;
use std::path::{Path, PathBuf};

use toml_edit::{DocumentMut, Item};

use crate::config::{ADDITIVE_ARRAY_KEYS, parse_and_validate, parse_and_validate_with_overlay};

/// Which config file edits should target.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditTarget {
    /// The global config at `~/.config/ags/config.toml`.
    Global,
    /// The repo-local overlay at `.ags/config.toml`.
    Local,
}

/// Whether to display the raw single-file view or the merged effective view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewMode {
    /// Show only the current target file (editable).
    Raw,
    /// Show the merged effective config (read-only).
    Merged,
}

/// Source annotation for a value in the merged view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ValueSource {
    /// Value exists only in the global config.
    Global,
    /// Value exists only in the local overlay.
    Local,
    /// Local overlay overrides a global value (scalar).
    LocalOverridesGlobal,
}

/// Metadata for a config section shown in the sidebar.
#[derive(Debug, Clone)]
pub struct SectionInfo {
    /// Human-readable label for the sidebar.
    pub label: &'static str,
    /// Corresponding TOML key.
    pub toml_key: &'static str,
    /// Whether this section uses `[[array_of_tables]]` syntax.
    pub is_array: bool,
}

/// All config sections in sidebar order.
pub const SECTIONS: &[SectionInfo] = &[
    SectionInfo {
        label: "Sandbox",
        toml_key: "sandbox",
        is_array: false,
    },
    SectionInfo {
        label: "Agents",
        toml_key: "agent_mount",
        is_array: true,
    },
    SectionInfo {
        label: "Mounts",
        toml_key: "mount",
        is_array: true,
    },
    SectionInfo {
        label: "Tools",
        toml_key: "tool",
        is_array: true,
    },
    SectionInfo {
        label: "Secrets",
        toml_key: "secret",
        is_array: true,
    },
    SectionInfo {
        label: "Browser",
        toml_key: "browser",
        is_array: false,
    },
    SectionInfo {
        label: "Auth Proxy",
        toml_key: "auth_proxy",
        is_array: false,
    },
    SectionInfo {
        label: "Host UI",
        toml_key: "host_ui",
        is_array: false,
    },
    SectionInfo {
        label: "PSP",
        toml_key: "psp",
        is_array: false,
    },
    SectionInfo {
        label: "Update",
        toml_key: "update",
        is_array: false,
    },
];

/// The layered config editor state.
///
/// Holds two independent `DocumentMut` instances — one for the global config and
/// one for the repo-local overlay draft — plus whether the local file currently
/// exists on disk, the selected edit target, and the current view mode.
pub struct ConfigEditorState {
    /// Parsed global config document.
    pub global_doc: DocumentMut,
    /// Path to the global config file.
    pub global_path: PathBuf,
    /// Parsed repo-local overlay document. This may be an unsaved in-memory draft
    /// even when `local_exists` is false.
    pub local_doc: DocumentMut,
    /// Whether `.ags/config.toml` currently exists on disk.
    pub local_exists: bool,
    /// Path to the repo-local config file.
    pub local_path: PathBuf,
    /// Which file edits currently target.
    pub edit_target: EditTarget,
    /// Whether to show raw or merged view.
    pub view_mode: ViewMode,
    /// Whether the active document has unsaved changes.
    pub modified: bool,
}

impl ConfigEditorState {
    /// Load both config files from disk.
    ///
    /// `global_path` must exist (or an error is returned).
    /// If `local_path` does not exist, the local layer starts as an empty in-memory
    /// document and will be created on first save.
    pub fn load(
        global_path: impl Into<PathBuf>,
        local_path: impl Into<PathBuf>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let global_path = global_path.into();
        let local_path = local_path.into();

        let global_content = fs::read_to_string(&global_path)?;
        let global_doc: DocumentMut = global_content.parse()?;

        let local_exists = local_path.exists();
        let local_doc = if local_exists {
            let content = fs::read_to_string(&local_path)?;
            content.parse::<DocumentMut>()?
        } else {
            "".parse::<DocumentMut>()?
        };

        Ok(Self {
            global_doc,
            global_path,
            local_doc,
            local_exists,
            local_path,
            edit_target: EditTarget::Global,
            view_mode: ViewMode::Raw,
            modified: false,
        })
    }

    /// Return a reference to the document for the current edit target.
    pub fn active_doc(&self) -> &DocumentMut {
        match self.edit_target {
            EditTarget::Global => &self.global_doc,
            EditTarget::Local => &self.local_doc,
        }
    }

    /// Return a mutable reference to the document for the current edit target.
    pub fn active_doc_mut(&mut self) -> &mut DocumentMut {
        match self.edit_target {
            EditTarget::Global => &mut self.global_doc,
            EditTarget::Local => &mut self.local_doc,
        }
    }

    /// Path of the current edit target.
    pub fn active_path(&self) -> &Path {
        match self.edit_target {
            EditTarget::Global => &self.global_path,
            EditTarget::Local => &self.local_path,
        }
    }

    /// Whether the local config file is missing on disk for the current target.
    pub fn local_missing_on_disk(&self) -> bool {
        self.edit_target == EditTarget::Local && !self.local_exists
    }

    /// Whether the local layer contributes anything to the effective config.
    pub fn has_local_layer(&self) -> bool {
        self.local_exists || self.local_doc.iter().next().is_some()
    }

    /// Compute the merged effective config by combining global + local overlay.
    ///
    /// Merge semantics match `parse.rs:merge_toml_value()`:
    /// - Additive keys (`mount`, `agent_mount`, `tool`, `secret`): arrays concatenated.
    /// - Everything else: overlay replaces base (recursive for nested tables).
    pub fn compute_merged_view(&self) -> DocumentMut {
        if !self.has_local_layer() {
            return self.global_doc.clone();
        }

        let mut merged = self.global_doc.clone();

        for (key, overlay_item) in self.local_doc.iter() {
            if ADDITIVE_ARRAY_KEYS.contains(&key) {
                // Additive: concatenate array-of-tables entries.
                if let Some(overlay_aot) = overlay_item.as_array_of_tables() {
                    let merged_item = &mut merged[key];
                    if let Some(base_aot) = merged_item.as_array_of_tables_mut() {
                        for table in overlay_aot.iter() {
                            base_aot.push(table.clone());
                        }
                    } else {
                        // Base didn't have this key — take overlay as-is.
                        merged[key] = overlay_item.clone();
                    }
                } else {
                    // Not actually an array-of-tables in the overlay — just replace.
                    merged[key] = overlay_item.clone();
                }
            } else {
                // Non-additive: recursive table merge or scalar replace.
                merge_item(&mut merged[key], overlay_item.clone());
            }
        }

        merged
    }

    /// Determine where a scalar value comes from in the merged view.
    ///
    /// `table_key` is the top-level section (e.g. `"sandbox"`, `"browser"`).
    /// `field_key` is the field within that section (e.g. `"image"`).
    pub fn value_source(&self, table_key: &str, field_key: &str) -> ValueSource {
        // Note: `Item::is_none()` is a toml_edit concept — it returns true for
        // the `Item::None` sentinel that represents a non-existent key, which is
        // distinct from `Option::is_none()` on the outer get() chain.
        let in_global = self
            .global_doc
            .get(table_key)
            .and_then(|t| t.get(field_key))
            .is_some_and(|i| !i.is_none());

        let in_local = self.has_local_layer()
            && self
                .local_doc
                .get(table_key)
                .and_then(|d| d.get(field_key))
                .is_some_and(|i| !i.is_none());

        match (in_global, in_local) {
            (true, true) => ValueSource::LocalOverridesGlobal,
            (true, false) => ValueSource::Global,
            (false, true) => ValueSource::Local,
            // Shouldn't happen for values that exist in merged view, but default
            // to Global as the base layer.
            (false, false) => ValueSource::Global,
        }
    }

    /// Determine the source of an entry in an additive array section.
    ///
    /// `array_key` is one of the additive keys (e.g. `"mount"`).
    /// `index` is the position in the merged array.
    ///
    /// In the merged view, global entries come first, followed by local entries.
    pub fn array_entry_source(&self, array_key: &str, index: usize) -> ValueSource {
        let global_count = self
            .global_doc
            .get(array_key)
            .and_then(|i| i.as_array_of_tables())
            .map(|a| a.len())
            .unwrap_or(0);

        if index < global_count {
            ValueSource::Global
        } else {
            ValueSource::Local
        }
    }

    /// Save the active document to disk.
    ///
    /// Creates a `.bak` backup of the existing file before writing.
    /// Uses atomic write (write to temp file, then rename) to prevent corruption.
    pub fn save(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let path = self.active_path().to_owned();
        let content = self.active_doc().to_string();

        backup_file(&path)?;
        atomic_write(&path, &content)?;

        if self.edit_target == EditTarget::Local {
            self.local_exists = true;
        }
        self.modified = false;
        Ok(())
    }

    /// Validate the active target using the same semantics as runtime config loading.
    pub fn validate_active(
        &self,
    ) -> Result<crate::config::ValidatedConfig, crate::config::ConfigError> {
        match self.edit_target {
            EditTarget::Global => parse_and_validate(&self.global_path),
            EditTarget::Local => parse_and_validate_with_overlay(
                &self.global_path,
                self.local_exists.then_some(self.local_path.as_path()),
            ),
        }
    }

    /// Restore the active document from its `.bak` backup.
    ///
    /// Returns `true` if a backup was found and restored, `false` if no backup exists.
    pub fn undo(&mut self) -> Result<bool, Box<dyn std::error::Error>> {
        let path = self.active_path().to_owned();
        let backup = path.with_extension("toml.bak");

        if !backup.exists() {
            return Ok(false);
        }

        let content = fs::read_to_string(&backup)?;
        let doc: DocumentMut = content.parse()?;

        match self.edit_target {
            EditTarget::Global => self.global_doc = doc,
            EditTarget::Local => {
                self.local_doc = doc;
                self.local_exists = true;
            }
        }

        // Write the restored content back to the target file.
        atomic_write(&path, &content)?;
        self.modified = false;
        Ok(true)
    }

    /// Switch the edit target between Global and Local.
    pub fn toggle_target(&mut self) {
        self.edit_target = match self.edit_target {
            EditTarget::Global => EditTarget::Local,
            EditTarget::Local => EditTarget::Global,
        };
    }

    /// Create the repo-local config file if it doesn't exist.
    ///
    /// Writes the current in-memory local document to disk and marks the local
    /// layer as existing.
    pub fn create_local_if_missing(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if self.local_exists {
            return Ok(());
        }

        atomic_write(&self.local_path, &self.local_doc.to_string())?;
        self.local_exists = true;
        Ok(())
    }
}

/// Recursively merge an overlay item into a base item.
///
/// Tables are merged field-by-field; everything else is replaced.
///
/// Note: this duplicates the semantics of `config::parse::merge_toml_value()`,
/// but operates on `toml_edit::Item` (format-preserving) instead of
/// `toml::Value` (serde-based). The two types are not interchangeable without a
/// serialize→parse round-trip, so a separate implementation is necessary here.
fn merge_item(base: &mut Item, overlay: Item) {
    match (base.as_table_like_mut(), overlay.as_table_like()) {
        (Some(base_table), Some(overlay_table)) => {
            for (key, value) in overlay_table.iter() {
                match base_table.entry(key) {
                    toml_edit::Entry::Occupied(mut entry) => {
                        merge_item(entry.get_mut(), value.clone());
                    }
                    toml_edit::Entry::Vacant(entry) => {
                        entry.insert(value.clone());
                    }
                }
            }
        }
        _ => {
            *base = overlay;
        }
    }
}

/// Copy the file at `path` to `path.with_extension("toml.bak")`.
fn backup_file(path: &Path) -> std::io::Result<PathBuf> {
    let backup = path.with_extension("toml.bak");
    match fs::copy(path, &backup) {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }
    Ok(backup)
}

/// Write `content` atomically by writing to a temp file and renaming.
fn atomic_write(path: &Path, content: &str) -> std::io::Result<()> {
    let dir = path.parent().unwrap_or(Path::new("."));
    fs::create_dir_all(dir)?;
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    std::io::Write::write_all(&mut tmp, content.as_bytes())?;
    tmp.persist(path).map_err(|e| e.error)?;
    Ok(())
}
