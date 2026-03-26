// TUI rendering and interaction for the Visual Config Editor.
//
// Uses ratatui + crossterm for the terminal UI. Follows the standard
// immediate-mode pattern: build frame each tick, handle key events.

use std::collections::BTreeMap;
use std::path::Path;

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyModifiers};
use ratatui::prelude::*;
use ratatui::widgets::*;
use tui_input::{Input, InputRequest};

use crate::util::capitalize_first;

use super::agents::{AgentDef, KNOWN_AGENTS};
use super::discovery::{
    discover_home_dirs, discover_path_binaries, suggest_binaries_from, suggest_paths_from,
};

/// Cached discovery data for autocomplete suggestions.
struct SuggestionCache<'a> {
    binaries: &'a [String],
    home_dirs: &'a [String],
}
use super::model::{ConfigEditorState, EditTarget, SECTIONS, SectionInfo, ValueSource, ViewMode};
use super::schema::{ScalarFieldKind, ScalarFieldSchema, scalar_field, scalar_fields};

// ---------------------------------------------------------------------------
// Form field types
// ---------------------------------------------------------------------------

#[derive(Clone, Copy)]
enum FieldKind {
    Text,
    Toggle(&'static [&'static str]),
    Checkbox,
}

struct FormField {
    label: &'static str,
    key: &'static str,
    kind: FieldKind,
    required: bool,
    input: Input,
}

// ---------------------------------------------------------------------------
// Edit mode
// ---------------------------------------------------------------------------

enum EditMode {
    None,
    Search {
        input: Input,
    },
    EditingField {
        input: Input,
        section_key: String,
        field_key: String,
        kind: ScalarFieldKind,
    },
    AddingEntry {
        fields: Vec<FormField>,
        active_field: usize,
        section_key: String,
        max_label: usize,
        edit_index: Option<usize>,
    },
    ConfirmDelete {
        index: usize,
        section_key: String,
    },
}

// ---------------------------------------------------------------------------
// UI-only types
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq)]
enum Focus {
    Sidebar,
    MainPanel,
}

/// A key-value field displayed in the main panel for scalar sections.
#[derive(Clone)]
struct FieldDisplay {
    key: String,
    value: String,
    source: Option<ValueSource>,
    present: bool,
    editable: bool,
}

/// A one-line summary for an array entry.
#[derive(Clone)]
struct ArrayEntry {
    summary: String,
    source: Option<ValueSource>,
    raw_index: usize,
}

/// Rendered content of the currently selected section.
#[derive(Clone)]
enum SectionContent {
    /// Scalar fields with pre-computed max key width for alignment.
    Scalar(Vec<FieldDisplay>, usize),
    Array(Vec<ArrayEntry>),
}

enum StatusKind {
    Info,
    Success,
    Warning,
    Error,
}

enum DialogState {
    None,
    ValidationError(String),
    QuitConfirm,
    CreateLocalPrompt,
}

/// What to do when toggling a scalar field.
enum ToggleAction {
    Bool(bool),
    Str(&'static str),
}

enum SaveOutcome {
    Saved,
    ValidationError,
    SaveFailed,
}

#[derive(Clone)]
enum HostStatus {
    Missing,
    Present,
    Partial(usize, usize),
}

impl std::fmt::Display for HostStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HostStatus::Missing => write!(f, "missing"),
            HostStatus::Present => write!(f, "exists"),
            HostStatus::Partial(count, total) => write!(f, "{count}/{total} exist"),
        }
    }
}

impl HostStatus {
    fn style(&self) -> Style {
        match self {
            HostStatus::Present => Style::default().fg(Color::Green),
            HostStatus::Missing => Style::default().fg(Color::Yellow),
            HostStatus::Partial(..) => Style::default().fg(Color::Cyan),
        }
    }
}

// ---------------------------------------------------------------------------
// App
// ---------------------------------------------------------------------------

/// Main application state for the config editor TUI.
pub struct App {
    running: bool,
    state: ConfigEditorState,

    // Navigation
    focus: Focus,
    selected_section: usize,
    selected_field: usize,

    // View state
    show_help: bool,
    search_query: String,

    // Status
    status_message: Option<(String, StatusKind)>,

    // Dialogs
    dialog: DialogState,

    // Edit mode
    edit_mode: EditMode,

    // Cached section content — invalidated on edits, target/view changes.
    content_cache: Vec<Option<SectionContent>>,
    // Cached agent enabled-state — parallel to KNOWN_AGENTS.
    agent_enabled_cache: Vec<bool>,
    // Whether repo-local editing is available in this cwd.
    repo_local_available: bool,
    // Whether a quit-confirm save is waiting for the validation dialog to resolve.
    quit_after_validation_dialog: bool,
    // Parallel to KNOWN_AGENTS, computed once. Not refreshed because the TUI
    // cannot create/delete host paths.
    agent_host_status_cache: Vec<HostStatus>,
    cached_binaries: Option<Vec<String>>,
    cached_home_dirs: Option<Vec<String>>,
    current_suggestion: Option<String>,
}

impl App {
    pub fn new(config_path: &Path) -> Result<Self, Box<dyn std::error::Error>> {
        let cwd = std::env::current_dir().ok();
        Self::new_with_cwd(config_path, cwd.as_deref())
    }

    fn new_with_cwd(
        config_path: &Path,
        cwd: Option<&Path>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let (local_path, repo_local_available) = super::resolve_local_target_path(cwd);

        let mut state = ConfigEditorState::load(config_path, &local_path)?;
        if !repo_local_available {
            state.local_doc = "".parse::<toml_edit::DocumentMut>()?;
            state.local_exists = false;
        }

        let agent_host_status_cache = KNOWN_AGENTS
            .iter()
            .map(|agent| compute_host_status(agent))
            .collect();

        Ok(Self {
            running: true,
            state,
            focus: Focus::Sidebar,
            selected_section: 0,
            selected_field: 0,
            show_help: false,
            search_query: String::new(),
            status_message: None,
            dialog: DialogState::None,
            edit_mode: EditMode::None,
            content_cache: vec![None; SECTIONS.len()],
            agent_enabled_cache: vec![false; KNOWN_AGENTS.len()],
            repo_local_available,
            quit_after_validation_dialog: false,
            agent_host_status_cache,
            cached_binaries: None,
            cached_home_dirs: None,
            current_suggestion: None,
        })
    }

    pub fn set_info_status(&mut self, message: impl Into<String>) {
        self.status_message = Some((message.into(), StatusKind::Info));
    }

    pub fn run(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        let mut terminal = ratatui::init();

        let result = self.event_loop(&mut terminal);

        ratatui::restore();
        result
    }

    fn event_loop(
        &mut self,
        terminal: &mut ratatui::DefaultTerminal,
    ) -> Result<(), Box<dyn std::error::Error>> {
        while self.running {
            self.ensure_cache();
            self.normalize_selection_for_filters();
            self.current_suggestion = self.active_suggestion();
            terminal.draw(|frame| self.render(frame))?;

            if let Event::Key(key) = event::read()? {
                self.handle_key(key);
            }
        }
        Ok(())
    }

    /// Pre-populate any missing cache entries before rendering.
    fn ensure_cache(&mut self) {
        let needs_fill = self.content_cache.iter().any(|s| s.is_none());
        if needs_fill {
            // Clone the document once for all sections that need filling.
            let is_merged = self.state.view_mode == ViewMode::Merged;
            let doc = if is_merged {
                self.state.compute_merged_view()
            } else {
                self.state.active_doc().clone()
            };

            for i in 0..SECTIONS.len() {
                if self.content_cache[i].is_none() {
                    let content = self.compute_section_content(i, &doc, is_merged);
                    self.content_cache[i] = Some(content);
                }
            }

            // Refresh agent enabled-state
            for (i, agent) in KNOWN_AGENTS.iter().enumerate() {
                self.agent_enabled_cache[i] = self.agent_is_enabled(agent);
            }
        }
    }

    // -------------------------------------------------------------------
    // Section helpers
    // -------------------------------------------------------------------

    /// Style for an item at `index` in the main panel — highlighted if selected.
    fn item_style(&self, index: usize) -> Style {
        if self.focus == Focus::MainPanel && index == self.selected_field {
            Style::default().fg(Color::Black).bg(Color::White)
        } else {
            Style::default()
        }
    }

    fn is_agents_section(&self) -> bool {
        SECTIONS[self.selected_section].toml_key == "agent_mount"
    }

    fn panel_border_color(&self, active: Focus) -> Color {
        if self.focus == active {
            Color::Cyan
        } else {
            Color::DarkGray
        }
    }

    fn current_section_info(&self) -> &SectionInfo {
        &SECTIONS[self.selected_section]
    }

    fn current_search_query(&self) -> &str {
        match &self.edit_mode {
            EditMode::Search { input } => input.value(),
            _ => self.search_query.as_str(),
        }
    }

    fn filtered_section_indices(&self) -> Vec<usize> {
        let query = self.current_search_query().trim().to_ascii_lowercase();
        if query.is_empty() {
            return (0..SECTIONS.len()).collect();
        }

        SECTIONS
            .iter()
            .enumerate()
            .filter(|(_, section)| {
                section.label.to_ascii_lowercase().contains(&query)
                    || section.toml_key.to_ascii_lowercase().contains(&query)
            })
            .map(|(index, _)| index)
            .collect()
    }

    fn filtered_agent_indices(&self) -> Vec<usize> {
        let query = self.current_search_query().trim().to_ascii_lowercase();
        if query.is_empty() {
            return (0..KNOWN_AGENTS.len()).collect();
        }

        KNOWN_AGENTS
            .iter()
            .enumerate()
            .filter(|(_, agent)| {
                agent.name.to_ascii_lowercase().contains(&query)
                    || agent
                        .mounts
                        .iter()
                        .any(|mount| mount.host.to_ascii_lowercase().contains(&query))
            })
            .map(|(index, _)| index)
            .collect()
    }

    fn filtered_section_content(&self, section_idx: usize) -> std::borrow::Cow<'_, SectionContent> {
        let query = self.current_search_query().trim().to_ascii_lowercase();
        if query.is_empty() {
            return std::borrow::Cow::Borrowed(self.section_content(section_idx));
        }

        let content = self.section_content(section_idx).clone();
        let filtered = match content {
            SectionContent::Scalar(fields, max_key) => SectionContent::Scalar(
                fields
                    .into_iter()
                    .filter(|field| {
                        field.key.to_ascii_lowercase().contains(&query)
                            || field.value.to_ascii_lowercase().contains(&query)
                    })
                    .collect(),
                max_key,
            ),
            SectionContent::Array(entries) => SectionContent::Array(
                entries
                    .into_iter()
                    .filter(|entry| entry.summary.to_ascii_lowercase().contains(&query))
                    .collect(),
            ),
        };
        std::borrow::Cow::Owned(filtered)
    }

    fn normalize_selection_for_filters(&mut self) {
        let visible_sections = self.filtered_section_indices();
        if let Some(first) = visible_sections.first().copied() {
            if !visible_sections.contains(&self.selected_section) {
                self.selected_section = first;
                self.selected_field = 0;
            }
        }

        let max = self.current_item_count();
        if max == 0 {
            self.selected_field = 0;
        } else if self.selected_field >= max {
            self.selected_field = max - 1;
        }
    }

    fn ensure_suggestion_caches(&mut self) {
        if self.cached_binaries.is_none() {
            self.cached_binaries = Some(discover_path_binaries());
        }
        if self.cached_home_dirs.is_none() {
            self.cached_home_dirs = Some(discover_home_dirs());
        }
    }

    fn suggestion_cache(&self) -> SuggestionCache<'_> {
        SuggestionCache {
            binaries: self.cached_binaries.as_deref().unwrap_or(&[]),
            home_dirs: self.cached_home_dirs.as_deref().unwrap_or(&[]),
        }
    }

    fn active_suggestion(&mut self) -> Option<String> {
        match &self.edit_mode {
            EditMode::EditingField { .. } | EditMode::AddingEntry { .. } => {}
            _ => return None,
        }
        self.ensure_suggestion_caches();
        let cache = self.suggestion_cache();
        match &self.edit_mode {
            EditMode::EditingField {
                input, field_key, ..
            } => suggestion_for_field(field_key, input.value(), &cache),
            EditMode::AddingEntry {
                fields,
                active_field,
                ..
            } => fields
                .get(*active_field)
                .and_then(|field| match field.kind {
                    FieldKind::Text => suggestion_for_field(field.key, field.input.value(), &cache),
                    _ => None,
                }),
            _ => None,
        }
    }

    /// Accept the current top-ranked suggestion into the active input field.
    fn accept_suggestion(&mut self) {
        self.ensure_suggestion_caches();
        let binaries = self.cached_binaries.as_deref().unwrap_or(&[]);
        let home_dirs = self.cached_home_dirs.as_deref().unwrap_or(&[]);
        let cache = SuggestionCache { binaries, home_dirs };
        match &mut self.edit_mode {
            EditMode::EditingField {
                input, field_key, ..
            } => {
                if let Some(suggestion) = suggestion_for_field(field_key, input.value(), &cache) {
                    *input = Input::new(suggestion);
                }
            }
            EditMode::AddingEntry {
                fields,
                active_field,
                ..
            } => {
                let field = &mut fields[*active_field];
                if let Some(suggestion) =
                    suggestion_for_field(field.key, field.input.value(), &cache)
                {
                    field.input = Input::new(suggestion);
                }
            }
            _ => {}
        }
    }

    // -------------------------------------------------------------------
    // Data extraction from real config (with caching)
    // -------------------------------------------------------------------

    /// Invalidate all cached section content. Call after any edit, save, undo,
    /// target toggle, or view mode change.
    fn invalidate_cache(&mut self) {
        for slot in &mut self.content_cache {
            *slot = None;
        }
    }

    /// Get cached section content. Call `ensure_cache()` before rendering.
    fn section_content(&self, section_idx: usize) -> &SectionContent {
        self.content_cache[section_idx]
            .as_ref()
            .expect("cache must be populated before rendering")
    }

    /// Build section content for display from the provided document.
    fn compute_section_content(
        &self,
        section_idx: usize,
        doc: &toml_edit::DocumentMut,
        is_merged: bool,
    ) -> SectionContent {
        let info = &SECTIONS[section_idx];

        if info.is_array {
            self.extract_array_content(doc, info, is_merged)
        } else {
            self.extract_scalar_content(doc, info, is_merged)
        }
    }

    fn extract_scalar_content(
        &self,
        doc: &toml_edit::DocumentMut,
        info: &SectionInfo,
        is_merged: bool,
    ) -> SectionContent {
        let mut fields = Vec::new();
        let table = doc.get(info.toml_key).and_then(|i| i.as_table_like());

        for schema in scalar_fields(info.toml_key) {
            let item = table.and_then(|table| table.get(schema.key));
            let present = item.is_some_and(|i| !i.is_none());
            let value = item
                .map(format_toml_value)
                .unwrap_or_else(|| missing_scalar_value(schema));
            let source = if is_merged && present {
                Some(self.state.value_source(info.toml_key, schema.key))
            } else {
                None
            };
            fields.push(FieldDisplay {
                key: schema.key.to_string(),
                value,
                source,
                present,
                editable: true,
            });
        }

        if let Some(table) = table {
            for (key, item) in table.iter() {
                if scalar_field(info.toml_key, key).is_some() {
                    continue;
                }
                let source = if is_merged {
                    Some(self.state.value_source(info.toml_key, key))
                } else {
                    None
                };
                fields.push(FieldDisplay {
                    key: key.to_string(),
                    value: format!("[unknown] {}", format_toml_value(item)),
                    source,
                    present: true,
                    editable: false,
                });
            }
        }

        let max_key = fields.iter().map(|f| f.key.len()).max().unwrap_or(0);
        SectionContent::Scalar(fields, max_key)
    }

    fn extract_array_content(
        &self,
        doc: &toml_edit::DocumentMut,
        info: &SectionInfo,
        is_merged: bool,
    ) -> SectionContent {
        let mut entries = Vec::new();

        if let Some(aot) = doc.get(info.toml_key).and_then(|i| i.as_array_of_tables()) {
            for (idx, table) in aot.iter().enumerate() {
                let summary = summarize_array_entry(info.toml_key, table);
                let source = if is_merged {
                    Some(self.state.array_entry_source(info.toml_key, idx))
                } else {
                    None
                };
                entries.push(ArrayEntry {
                    summary,
                    source,
                    raw_index: idx,
                });
            }
        }

        SectionContent::Array(entries)
    }

    /// Returns `true` (and sets a warning) if we're in merged view and edits
    /// should be blocked.
    fn reject_if_merged(&mut self) -> bool {
        if self.state.view_mode == ViewMode::Merged {
            self.status_message = Some((
                "Press Enter to jump to the originating Raw value, or Ctrl-V to switch views."
                    .into(),
                StatusKind::Warning,
            ));
            true
        } else {
            false
        }
    }

    // -------------------------------------------------------------------
    // Agent helpers
    // -------------------------------------------------------------------

    fn agent_mount_present(
        aot: &toml_edit::ArrayOfTables,
        mount: &super::agents::AgentMountDef,
    ) -> bool {
        aot.iter().any(|table| {
            table.get("host").and_then(|v| v.as_str()) == Some(mount.host)
                && table.get("container").and_then(|v| v.as_str()) == Some(mount.container)
        })
    }

    fn agent_is_enabled(&self, agent: &AgentDef) -> bool {
        let doc = self.state.active_doc();
        let aot = match doc.get("agent_mount").and_then(|i| i.as_array_of_tables()) {
            Some(aot) => aot,
            None => return false,
        };

        agent
            .mounts
            .iter()
            .all(|mount| Self::agent_mount_present(aot, mount))
    }

    fn agent_host_status(&self, agent_idx: usize) -> &HostStatus {
        &self.agent_host_status_cache[agent_idx]
    }

    fn toggle_agent(&mut self, agent_idx: usize) {
        if self.reject_if_merged() {
            return;
        }

        if agent_idx >= KNOWN_AGENTS.len() {
            return;
        }

        let agent = &KNOWN_AGENTS[agent_idx];

        // Check current state (immutable borrow scoped in block)
        let enabled = self.agent_is_enabled(agent);

        let doc = self.state.active_doc_mut();
        let mut changed = false;

        if enabled {
            // Remove matching mounts (reverse order to preserve indices)
            if let Some(aot) = doc["agent_mount"].as_array_of_tables_mut() {
                let mut to_remove = Vec::new();
                for (i, table) in aot.iter().enumerate() {
                    for m in agent.mounts {
                        if table.get("host").and_then(|v| v.as_str()) == Some(m.host)
                            && table.get("container").and_then(|v| v.as_str()) == Some(m.container)
                        {
                            to_remove.push(i);
                        }
                    }
                }
                to_remove.sort_unstable();
                to_remove.dedup();
                changed = !to_remove.is_empty();
                for i in to_remove.into_iter().rev() {
                    aot.remove(i);
                }
            }
        } else {
            // Add only mounts that are still missing for this agent.
            if doc.get("agent_mount").is_none() || doc["agent_mount"].as_array_of_tables().is_none()
            {
                doc["agent_mount"] =
                    toml_edit::Item::ArrayOfTables(toml_edit::ArrayOfTables::new());
            }
            if let Some(aot) = doc["agent_mount"].as_array_of_tables_mut() {
                for m in agent.mounts {
                    if Self::agent_mount_present(aot, m) {
                        continue;
                    }
                    let mut entry = toml_edit::Table::new();
                    entry["host"] = toml_edit::value(m.host);
                    entry["container"] = toml_edit::value(m.container);
                    entry["kind"] = toml_edit::value(m.kind.to_string());
                    aot.push(entry);
                    changed = true;
                }
            }
        }

        if changed {
            self.state.modified = true;
            self.invalidate_cache();
        }
    }

    // -------------------------------------------------------------------
    // Scalar field editing
    // -------------------------------------------------------------------

    /// Resolve the currently selected scalar field for editing. Returns the
    /// section index, field key, and schema, or sets a status message and returns `None`.
    fn resolve_editable_scalar(
        &mut self,
    ) -> Option<(usize, String, &'static ScalarFieldSchema)> {
        if self.reject_if_merged() {
            return None;
        }
        let info = &SECTIONS[self.selected_section];
        if info.is_array {
            return None;
        }
        let content = self.filtered_section_content(self.selected_section);
        let fields = match &*content {
            SectionContent::Scalar(f, _) => f,
            _ => return None,
        };
        if self.selected_field >= fields.len() {
            return None;
        }
        let field = &fields[self.selected_field];
        if !field.editable {
            self.status_message = Some((
                "Unknown fields are preserved but not editable in the structured editor.".into(),
                StatusKind::Warning,
            ));
            return None;
        }
        let field_key = field.key.clone();
        let schema = scalar_field(info.toml_key, &field_key)?;
        Some((self.selected_section, field_key, schema))
    }

    fn start_edit_field(&mut self) {
        let Some((section_idx, field_key, schema)) = self.resolve_editable_scalar() else {
            return;
        };
        let info = &SECTIONS[section_idx];

        let raw_value = {
            let doc = self.state.active_doc();
            doc.get(info.toml_key)
                .and_then(|t| t.get(&field_key))
                .map(|item| item_to_editor_text(item, schema))
                .unwrap_or_else(|| schema.default_input.to_string())
        };

        let mut input = Input::new(raw_value);
        input.handle(InputRequest::GoToEnd);

        self.edit_mode = EditMode::EditingField {
            input,
            section_key: info.toml_key.to_string(),
            field_key,
            kind: schema.kind,
        };
    }

    fn confirm_edit_field(&mut self) {
        let (new_text, section_key, field_key, kind) = match &self.edit_mode {
            EditMode::EditingField {
                input,
                section_key,
                field_key,
                kind,
            } => (
                input.value().to_string(),
                section_key.clone(),
                field_key.clone(),
                *kind,
            ),
            _ => return,
        };

        if let Err(message) = apply_scalar_value(
            self.state.active_doc_mut(),
            &section_key,
            &field_key,
            kind,
            &new_text,
        ) {
            self.status_message = Some((message, StatusKind::Error));
            return;
        }

        self.state.modified = true;
        self.invalidate_cache();
        self.edit_mode = EditMode::None;
    }

    fn toggle_scalar_field(&mut self) {
        let Some((section_idx, field_key, schema)) = self.resolve_editable_scalar() else {
            return;
        };
        let info = &SECTIONS[section_idx];

        let action = next_toggle_action(self.state.active_doc(), info.toml_key, &field_key, schema);

        if let Some(action) = action {
            let doc = self.state.active_doc_mut();
            ensure_table(doc, info.toml_key);
            match action {
                ToggleAction::Bool(b) => {
                    doc[info.toml_key][&field_key] = toml_edit::value(b);
                }
                ToggleAction::Str(s) => {
                    doc[info.toml_key][&field_key] = toml_edit::value(s);
                }
            }
            self.state.modified = true;
            self.invalidate_cache();
        }
    }

    // -------------------------------------------------------------------
    // Add / delete array entries
    // -------------------------------------------------------------------

    /// Returns the current section index if it's an editable (non-agents) array section,
    /// or `None` (with status message) if edits should be blocked.
    fn require_editable_array(&mut self) -> Option<usize> {
        if self.reject_if_merged() {
            return None;
        }
        let info = &SECTIONS[self.selected_section];
        if !info.is_array || self.is_agents_section() {
            return None;
        }
        Some(self.selected_section)
    }

    fn start_add_entry(&mut self) {
        let Some(section_idx) = self.require_editable_array() else {
            return;
        };
        let info = &SECTIONS[section_idx];

        let fields = build_entry_form_fields(info.toml_key, None);
        if fields.is_empty() {
            return;
        }

        let max_label = fields.iter().map(|f| f.label.len()).max().unwrap_or(0);
        self.edit_mode = EditMode::AddingEntry {
            fields,
            active_field: 0,
            section_key: info.toml_key.to_string(),
            max_label,
            edit_index: None,
        };
    }

    fn start_edit_entry(&mut self) {
        let Some(section_idx) = self.require_editable_array() else {
            return;
        };
        let info = &SECTIONS[section_idx];

        let content = self.filtered_section_content(self.selected_section);
        let entries = match &*content {
            SectionContent::Array(entries) => entries,
            _ => return,
        };
        let Some(entry) = entries.get(self.selected_field) else {
            return;
        };

        let fields = {
            let doc = self.state.active_doc();
            let table = doc
                .get(info.toml_key)
                .and_then(|item| item.as_array_of_tables())
                .and_then(|entries| entries.get(entry.raw_index));
            build_entry_form_fields(info.toml_key, table)
        };
        if fields.is_empty() {
            return;
        }

        let max_label = fields.iter().map(|f| f.label.len()).max().unwrap_or(0);
        self.edit_mode = EditMode::AddingEntry {
            fields,
            active_field: 0,
            section_key: info.toml_key.to_string(),
            max_label,
            edit_index: Some(entry.raw_index),
        };
    }

    fn confirm_add_entry(&mut self) {
        let (field_values, section_key, edit_index) = match &self.edit_mode {
            EditMode::AddingEntry {
                fields,
                section_key,
                edit_index,
                ..
            } => {
                for field in fields {
                    if field.required && field.input.value().is_empty() {
                        self.status_message =
                            Some((format!("{} is required", field.key), StatusKind::Error));
                        return;
                    }
                }
                let values = fields
                    .iter()
                    .map(|field| (field.key, field.kind, field.input.value().to_string()))
                    .collect::<Vec<_>>();
                (values, section_key.clone(), *edit_index)
            }
            _ => return,
        };

        let doc = self.state.active_doc_mut();
        if doc.get(&section_key).is_none() || doc[&section_key].as_array_of_tables().is_none() {
            doc[&section_key] = toml_edit::Item::ArrayOfTables(toml_edit::ArrayOfTables::new());
        }

        let Some(aot) = doc[&section_key].as_array_of_tables_mut() else {
            return;
        };

        if let Some(index) = edit_index {
            let Some(entry) = aot.get_mut(index) else {
                self.status_message = Some(("Entry no longer exists.".into(), StatusKind::Error));
                return;
            };
            if let Err(error) = apply_entry_form(section_key.as_str(), entry, &field_values) {
                self.status_message = Some((error, StatusKind::Error));
                return;
            }
        } else {
            let mut entry = toml_edit::Table::new();
            if let Err(error) = apply_entry_form(section_key.as_str(), &mut entry, &field_values) {
                self.status_message = Some((error, StatusKind::Error));
                return;
            }
            aot.push(entry);
        }

        self.state.modified = true;
        self.invalidate_cache();
        self.edit_mode = EditMode::None;
        self.status_message = Some((
            if edit_index.is_some() {
                "Entry updated.".into()
            } else {
                "Entry added.".into()
            },
            StatusKind::Success,
        ));
    }

    fn start_delete_entry(&mut self) {
        let Some(section_idx) = self.require_editable_array() else {
            return;
        };
        let info = &SECTIONS[section_idx];

        let content = self.filtered_section_content(self.selected_section);
        let entries = match &*content {
            SectionContent::Array(entries) => entries,
            _ => return,
        };

        if entries.is_empty() || self.selected_field >= entries.len() {
            return;
        }

        self.edit_mode = EditMode::ConfirmDelete {
            index: entries[self.selected_field].raw_index,
            section_key: info.toml_key.to_string(),
        };
        self.status_message = Some((
            format!("Delete entry {}? [y/N]", self.selected_field + 1),
            StatusKind::Warning,
        ));
    }

    fn confirm_delete(&mut self) {
        let (index, section_key) = match &self.edit_mode {
            EditMode::ConfirmDelete { index, section_key } => (*index, section_key.clone()),
            _ => return,
        };

        let (removed, new_len) = {
            let doc = self.state.active_doc_mut();
            if let Some(aot) = doc[&section_key].as_array_of_tables_mut() {
                if index < aot.len() {
                    aot.remove(index);
                    (true, aot.len())
                } else {
                    (false, aot.len())
                }
            } else {
                (false, 0)
            }
        };

        if removed {
            self.state.modified = true;
            self.invalidate_cache();
            self.status_message = Some(("Entry deleted.".into(), StatusKind::Success));
            if self.selected_field >= new_len && new_len > 0 {
                self.selected_field = new_len - 1;
            }
        }
        self.edit_mode = EditMode::None;
    }

    fn jump_to_origin(&mut self) {
        if self.state.view_mode != ViewMode::Merged || self.focus != Focus::MainPanel {
            return;
        }

        let info = self.current_section_info();
        let content = self.filtered_section_content(self.selected_section);

        let (target, selected_field, message) = if info.is_array {
            let SectionContent::Array(entries) = &*content else {
                return;
            };
            let Some(entry) = entries.get(self.selected_field) else {
                return;
            };
            let Some(source) = entry.source else {
                return;
            };
            let target = match source {
                ValueSource::Global => EditTarget::Global,
                ValueSource::Local | ValueSource::LocalOverridesGlobal => EditTarget::Local,
            };
            let selected_field = match source {
                ValueSource::Global => entry.raw_index,
                ValueSource::Local | ValueSource::LocalOverridesGlobal => entry
                    .raw_index
                    .saturating_sub(global_array_len(&self.state.global_doc, info.toml_key)),
            };
            (
                target,
                selected_field,
                format!("Jumped to {} raw value.", edit_target_label(target)),
            )
        } else {
            let SectionContent::Scalar(fields, _) = &*content else {
                return;
            };
            let Some(field) = fields.get(self.selected_field) else {
                return;
            };
            let Some(source) = field.source else {
                return;
            };
            let target = match source {
                ValueSource::Global => EditTarget::Global,
                ValueSource::Local | ValueSource::LocalOverridesGlobal => EditTarget::Local,
            };
            let selected_field =
                scalar_field_index_for_target(&self.state, target, info.toml_key, &field.key)
                    .unwrap_or(self.selected_field);
            (
                target,
                selected_field,
                format!("Jumped to {} raw value.", edit_target_label(target)),
            )
        };

        self.state.edit_target = target;
        self.state.view_mode = ViewMode::Raw;
        self.selected_field = selected_field;
        self.invalidate_cache();
        self.status_message = Some((message, StatusKind::Info));
    }

    // -------------------------------------------------------------------
    // Rendering
    // -------------------------------------------------------------------

    fn render(&self, frame: &mut Frame) {
        let area = frame.area();

        // Layout: top bar (3) | middle (fill) | bottom bar (1)
        let outer = Layout::vertical([
            Constraint::Length(3),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(area);

        self.render_top_bar(frame, outer[0]);

        if self.show_help {
            self.render_help_overlay(frame, outer[1]);
        } else {
            self.render_middle(frame, outer[1]);
        }

        // Render dialog overlay on top of content if active
        match &self.dialog {
            DialogState::ValidationError(msg) => {
                self.render_validation_dialog(frame, outer[1], msg);
            }
            DialogState::QuitConfirm => {
                self.render_quit_dialog(frame, outer[1]);
            }
            DialogState::CreateLocalPrompt => {
                self.render_create_local_dialog(frame, outer[1]);
            }
            DialogState::None => {}
        }

        self.render_bottom_bar(frame, outer[2]);
    }

    fn render_top_bar(&self, frame: &mut Frame, area: Rect) {
        let target_label = match self.state.edit_target {
            EditTarget::Global => "Global (~/.config/ags/config.toml)",
            EditTarget::Local if self.state.local_exists => "Local (.ags/config.toml)",
            EditTarget::Local => "Local (.ags/config.toml, creates on save)",
        };
        let view_label = match self.state.view_mode {
            ViewMode::Raw => "Raw",
            ViewMode::Merged => "Merged",
        };
        let mut spans = vec![Span::raw(format!(
            " Target: {}    View: {}",
            target_label, view_label
        ))];
        if !self.repo_local_available {
            spans.push(Span::styled(
                "    Repo-local: disabled (not in a git repo)",
                Style::default().fg(Color::DarkGray),
            ));
        }
        if self.state.modified {
            spans.push(Span::styled(
                "  [modified]",
                Style::default().fg(Color::Yellow).bold(),
            ));
        }

        let block = Block::bordered()
            .title(" AGS Config Editor ")
            .title_alignment(Alignment::Center)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::Cyan));

        let text = Paragraph::new(Line::from(spans)).block(block);
        frame.render_widget(text, area);
    }

    fn render_middle(&self, frame: &mut Frame, area: Rect) {
        // Horizontal split: sidebar (22 cols) | main panel (rest)
        let chunks = Layout::horizontal([Constraint::Length(22), Constraint::Min(1)]).split(area);

        self.render_sidebar(frame, chunks[0]);
        self.render_main_panel(frame, chunks[1]);
    }

    fn render_sidebar(&self, frame: &mut Frame, area: Rect) {
        let items: Vec<ListItem> = self
            .filtered_section_indices()
            .into_iter()
            .map(|i| {
                let info = &SECTIONS[i];
                let count_suffix = if info.is_array {
                    if info.toml_key == "agent_mount" {
                        // Show count of enabled agents (from cache)
                        let enabled = self.agent_enabled_cache.iter().filter(|&&e| e).count();
                        format!(" ({}/{})", enabled, KNOWN_AGENTS.len())
                    } else {
                        match self.section_content(i) {
                            SectionContent::Array(entries) => {
                                format!(" ({})", entries.len())
                            }
                            _ => String::new(),
                        }
                    }
                } else {
                    String::new()
                };

                let label = format!("  {}{}", info.label, count_suffix);
                let style = if i == self.selected_section {
                    if self.focus == Focus::Sidebar {
                        Style::default().fg(Color::Black).bg(Color::Cyan).bold()
                    } else {
                        Style::default().fg(Color::Cyan).bold()
                    }
                } else {
                    Style::default()
                };
                ListItem::new(label).style(style)
            })
            .collect();

        let sidebar = List::new(items).block(
            Block::bordered()
                .title(" Sections ")
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(self.panel_border_color(Focus::Sidebar))),
        );
        frame.render_widget(sidebar, area);
    }

    fn render_main_panel(&self, frame: &mut Frame, area: Rect) {
        let info = &SECTIONS[self.selected_section];

        let block = Block::bordered()
            .title(format!(" {} ", info.label))
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(self.panel_border_color(Focus::MainPanel)));

        let inner = block.inner(area);
        frame.render_widget(block, area);

        // Check if we're in add mode for this section
        let in_add_mode = matches!(
            &self.edit_mode,
            EditMode::AddingEntry { section_key, .. } if section_key == info.toml_key
        );

        if in_add_mode {
            let form_height = match &self.edit_mode {
                EditMode::AddingEntry { fields, .. } => fields.len() as u16 + 5,
                _ => 0,
            };
            let chunks = Layout::vertical([Constraint::Min(1), Constraint::Length(form_height)])
                .split(inner);

            self.render_section_content(frame, chunks[0]);
            self.render_add_form(frame, chunks[1]);
        } else {
            self.render_section_content(frame, inner);
        }
    }

    fn render_section_content(&self, frame: &mut Frame, area: Rect) {
        if self.state.local_missing_on_disk()
            && self.state.view_mode == ViewMode::Raw
            && !self.state.has_local_layer()
        {
            let hint = Paragraph::new(
                "  Repo-local config does not exist yet.\n\n  Any edits here stay local and will create .ags/config.toml on save.\n\n  Press Ctrl-S to create it now, or Ctrl-T to switch back to Global.",
            )
            .style(Style::default().fg(Color::DarkGray));
            frame.render_widget(hint, area);
            return;
        }

        if self.is_agents_section() {
            self.render_agents_section(frame, area);
        } else {
            let content = self.filtered_section_content(self.selected_section);
            match &*content {
                SectionContent::Scalar(fields, max_key) => {
                    self.render_scalar_fields(frame, area, fields, *max_key)
                }
                SectionContent::Array(entries) => self.render_array_entries(frame, area, entries),
            }
        }
    }

    fn render_agents_section(&self, frame: &mut Frame, area: Rect) {
        let items: Vec<ListItem> = self
            .filtered_agent_indices()
            .into_iter()
            .enumerate()
            .map(|(visible_index, i)| {
                let agent = &KNOWN_AGENTS[i];
                let enabled = self.agent_enabled_cache[i];
                let checkbox = if enabled { "[x]" } else { "[ ]" };

                let mut mounts_desc = String::new();
                for (j, m) in agent.mounts.iter().enumerate() {
                    if j > 0 {
                        mounts_desc.push_str(", ");
                    }
                    mounts_desc.push_str(m.host);
                }
                let host_status = self.agent_host_status(i);

                let style = self.item_style(visible_index);

                let checkbox_style = if enabled {
                    Style::default().fg(Color::Green)
                } else {
                    Style::default().fg(Color::DarkGray)
                };

                ListItem::new(Line::from(vec![
                    Span::raw("  "),
                    Span::styled(checkbox, checkbox_style),
                    Span::raw(format!(" {:<10} {}", agent.name, mounts_desc)),
                    Span::styled(format!("  [{}]", host_status), host_status.style()),
                ]))
                .style(style)
            })
            .collect();

        let list = List::new(items);
        frame.render_widget(list, area);
    }

    fn render_scalar_fields(
        &self,
        frame: &mut Frame,
        area: Rect,
        fields: &[FieldDisplay],
        max_key: usize,
    ) {
        if fields.is_empty() {
            let empty = Paragraph::new("  (no fields)").style(Style::default().fg(Color::DarkGray));
            frame.render_widget(empty, area);
            return;
        }

        let rows: Vec<Row> = fields
            .iter()
            .enumerate()
            .map(|(i, field)| {
                let style = self.item_style(i);
                let key_style = if field.editable {
                    Style::default().fg(Color::Yellow)
                } else {
                    Style::default().fg(Color::DarkGray)
                };
                let value_style = if field.present {
                    Style::default()
                } else {
                    Style::default().fg(Color::DarkGray).italic()
                };

                let source_marker = source_suffix(field.source);

                Row::new(vec![
                    Cell::from(format!("  {:<width$}", field.key, width = max_key))
                        .style(key_style),
                    Cell::from(" = "),
                    Cell::from(field.value.clone()).style(value_style),
                    Cell::from(source_marker.0).style(source_marker.1),
                ])
                .style(style)
            })
            .collect();

        let widths = [
            Constraint::Length((max_key + 2) as u16),
            Constraint::Length(3),
            Constraint::Min(1),
            Constraint::Length(6),
        ];

        let table = Table::new(rows, widths).column_spacing(0);
        frame.render_widget(table, area);

        // Overlay input widget if editing a field in this section
        if let EditMode::EditingField {
            input,
            section_key,
            field_key,
            ..
        } = &self.edit_mode
        {
            let info = &SECTIONS[self.selected_section];
            if section_key == info.toml_key {
                if let Some(i) = fields.iter().position(|f| f.key == *field_key) {
                    let y = area.y + i as u16;
                    let value_x = area.x + (max_key + 2) as u16 + 3;
                    let value_width = area.width.saturating_sub((max_key + 2) as u16 + 3 + 6);
                    let input_area = Rect::new(value_x, y, value_width, 1);

                    let scroll = input.visual_scroll(value_width.saturating_sub(1) as usize);
                    let display = Paragraph::new(input.value())
                        .scroll((0, scroll as u16))
                        .style(Style::default().fg(Color::White).bg(Color::DarkGray));
                    frame.render_widget(display, input_area);

                    let cursor_x = value_x + (input.visual_cursor().saturating_sub(scroll)) as u16;
                    frame.set_cursor_position((cursor_x, y));
                }
            }
        }
    }

    fn render_array_entries(&self, frame: &mut Frame, area: Rect, entries: &[ArrayEntry]) {
        if entries.is_empty() {
            let empty = Paragraph::new("  (no entries)\n\n  Press 'a' to add")
                .style(Style::default().fg(Color::DarkGray));
            frame.render_widget(empty, area);
            return;
        }

        let items: Vec<ListItem> = entries
            .iter()
            .enumerate()
            .map(|(i, entry)| {
                let style = self.item_style(i);

                let marker = source_suffix(entry.source);

                ListItem::new(Line::from(vec![
                    Span::raw(format!("  {}. {}", i + 1, entry.summary)),
                    if !marker.0.is_empty() {
                        Span::styled(format!("  {}", marker.0), marker.1)
                    } else {
                        Span::raw("")
                    },
                ]))
                .style(style)
            })
            .collect();

        let list = List::new(items);
        frame.render_widget(list, area);
    }

    fn render_add_form(&self, frame: &mut Frame, area: Rect) {
        let (fields, active_field, section_key, max_label, edit_index) = match &self.edit_mode {
            EditMode::AddingEntry {
                fields,
                active_field,
                section_key,
                max_label,
                edit_index,
            } => (
                fields,
                *active_field,
                section_key.as_str(),
                *max_label,
                *edit_index,
            ),
            _ => return,
        };

        let title = if edit_index.is_some() {
            format!(" Edit {} ", capitalize_first(section_key))
        } else {
            format!(" Add {} ", capitalize_first(section_key))
        };
        let block = Block::bordered()
            .title(title)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::Cyan));

        let inner = block.inner(area);
        frame.render_widget(block, area);

        for (i, field) in fields.iter().enumerate() {
            if i as u16 >= inner.height.saturating_sub(1) {
                break;
            }

            let y = inner.y + i as u16;
            let is_active = i == active_field;

            // Label
            let label_width = (max_label + 4) as u16;
            let label_area = Rect::new(inner.x, y, label_width.min(inner.width), 1);
            let label_style = if is_active {
                Style::default().fg(Color::Yellow).bold()
            } else {
                Style::default().fg(Color::DarkGray)
            };
            frame.render_widget(
                Paragraph::new(format!("  {:<width$}:", field.label, width = max_label))
                    .style(label_style),
                label_area,
            );

            // Value area
            let value_x = inner.x + label_width;
            let value_width = inner.width.saturating_sub(label_width + 1);
            if value_width == 0 {
                continue;
            }
            let value_area = Rect::new(value_x, y, value_width, 1);

            match &field.kind {
                FieldKind::Text => {
                    let scroll = field
                        .input
                        .visual_scroll(value_width.saturating_sub(1) as usize);
                    let style = if is_active {
                        Style::default().fg(Color::White).bg(Color::DarkGray)
                    } else {
                        Style::default().fg(Color::White)
                    };
                    let display = Paragraph::new(field.input.value())
                        .scroll((0, scroll as u16))
                        .style(style);
                    frame.render_widget(display, value_area);

                    if is_active {
                        let cursor_x =
                            value_x + (field.input.visual_cursor().saturating_sub(scroll)) as u16;
                        frame.set_cursor_position((cursor_x, y));
                    }
                }
                FieldKind::Toggle(options) => {
                    let current = field.input.value();
                    let mut spans = vec![Span::raw(" ")];
                    for (j, opt) in options.iter().enumerate() {
                        if j > 0 {
                            spans.push(Span::styled(" | ", Style::default().fg(Color::DarkGray)));
                        }
                        if *opt == current {
                            spans.push(Span::styled(
                                format!(" {} ", opt),
                                Style::default().fg(Color::Black).bg(Color::Cyan).bold(),
                            ));
                        } else {
                            spans.push(Span::styled(
                                format!(" {} ", opt),
                                Style::default().fg(Color::DarkGray),
                            ));
                        }
                    }
                    frame.render_widget(Paragraph::new(Line::from(spans)), value_area);
                }
                FieldKind::Checkbox => {
                    let checked = field.input.value() == "true";
                    let display = if checked { "[x]" } else { "[ ]" };
                    let style = if checked {
                        Style::default().fg(Color::Green)
                    } else {
                        Style::default().fg(Color::DarkGray)
                    };
                    frame.render_widget(
                        Paragraph::new(format!(" {}", display)).style(style),
                        value_area,
                    );
                }
            }
        }

        // Footer hint
        let footer_y = inner.y + inner.height.saturating_sub(1);
        if footer_y > inner.y + fields.len() as u16 {
            let footer =
                Paragraph::new("  Enter: Confirm  Esc: Cancel  Tab: Next  Shift-Tab: Prev")
                    .style(Style::default().fg(Color::DarkGray));
            frame.render_widget(footer, Rect::new(inner.x, footer_y, inner.width, 1));
        }
    }

    fn render_bottom_bar(&self, frame: &mut Frame, area: Rect) {
        if let Some((ref msg, ref kind)) = self.status_message {
            let fg = match kind {
                StatusKind::Info => Color::White,
                StatusKind::Success => Color::Green,
                StatusKind::Warning => Color::Yellow,
                StatusKind::Error => Color::Red,
            };
            let bar = Paragraph::new(format!(" {msg}"))
                .style(Style::default().fg(fg).bg(Color::DarkGray));
            frame.render_widget(bar, area);
            return;
        }

        if let EditMode::Search { input } = &self.edit_mode {
            let bar = Paragraph::new(format!(
                " / Search: {}  Enter: Apply  Esc: Clear",
                input.value()
            ))
            .style(Style::default().fg(Color::White).bg(Color::DarkGray));
            frame.render_widget(bar, area);
            return;
        }

        let mut shortcuts = match &self.edit_mode {
            EditMode::EditingField { .. } => "Enter: Confirm  Esc: Cancel".to_string(),
            EditMode::AddingEntry { .. } => {
                "Enter: Confirm  Esc: Cancel  Tab: Next  Shift-Tab: Prev".to_string()
            }
            EditMode::ConfirmDelete { .. } => "y: Delete  Any key: Cancel".to_string(),
            EditMode::Search { .. } => unreachable!(),
            EditMode::None => match self.focus {
                Focus::Sidebar => {
                    "^S Save  ^Z Undo  ^T Target  ^V View  Tab Panel  ? Help  q Quit".to_string()
                }
                Focus::MainPanel => {
                    let info = &SECTIONS[self.selected_section];
                    if self.state.view_mode == ViewMode::Merged {
                        "Enter Jump to raw  ^V Raw view  Tab Sidebar  Esc Back  ? Help  q Quit"
                            .to_string()
                    } else if self.is_agents_section() {
                        "Space Toggle  ^S Save  ^Z Undo  Tab Sidebar  Esc Back  ? Help  q Quit"
                            .to_string()
                    } else if info.is_array {
                        "Enter Edit  a Add  d Delete  ^S Save  ^Z Undo  Tab Sidebar  Esc Back  ? Help  q Quit"
                            .to_string()
                    } else {
                        "Enter Edit  Space Toggle  ^S Save  ^Z Undo  Tab Sidebar  Esc Back  ? Help  q Quit"
                            .to_string()
                    }
                }
            },
        };

        if let Some(suggestion) = &self.current_suggestion {
            shortcuts.push_str(&format!("  Ctrl-G Suggest: {}", suggestion));
        }

        let bar = Paragraph::new(format!(" {shortcuts}"))
            .style(Style::default().fg(Color::White).bg(Color::DarkGray));
        frame.render_widget(bar, area);
    }

    fn render_help_overlay(&self, frame: &mut Frame, area: Rect) {
        let help_text = vec![
            Line::from("Navigation").style(Style::default().bold().fg(Color::Cyan)),
            Line::from("  j / Down      Move down"),
            Line::from("  k / Up        Move up"),
            Line::from("  Tab           Move focus to main panel"),
            Line::from("  Shift-Tab     Move focus to sidebar"),
            Line::from("  Enter         Select / edit / jump to raw in merged view"),
            Line::from("  Escape        Back to sidebar / cancel edit"),
            Line::from(""),
            Line::from("Actions").style(Style::default().bold().fg(Color::Cyan)),
            Line::from("  a             Add entry (array sections)"),
            Line::from("  d             Delete entry (array sections)"),
            Line::from("  Space         Toggle (agents / booleans)"),
            Line::from("  Ctrl-G        Accept top discovery suggestion (text inputs)"),
            Line::from(""),
            Line::from("Global").style(Style::default().bold().fg(Color::Cyan)),
            Line::from("  Ctrl-S        Save current target"),
            Line::from("  Ctrl-Z        Undo (restore from backup)"),
            Line::from("  Ctrl-T        Switch target (Global/Local)"),
            Line::from("  Ctrl-V        Switch view (Raw/Merged)"),
            Line::from("  ?             Toggle this help"),
            Line::from("  q             Quit (prompts if unsaved)"),
        ];

        let block = Block::bordered()
            .title(" Help ")
            .title_alignment(Alignment::Center)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::Cyan));

        let help = Paragraph::new(help_text).block(block);
        frame.render_widget(help, area);
    }

    /// Render a centered dialog box and return its inner rect.
    fn render_centered_dialog(
        frame: &mut Frame,
        area: Rect,
        max_width: u16,
        title: &str,
        border_color: Color,
    ) -> Rect {
        let dialog_width = (area.width.saturating_sub(8)).min(max_width);
        let dialog_height = 7_u16;
        let x = area.x + (area.width.saturating_sub(dialog_width)) / 2;
        let y = area.y + (area.height.saturating_sub(dialog_height)) / 2;
        let dialog_area = Rect::new(x, y, dialog_width, dialog_height);

        frame.render_widget(Clear, dialog_area);

        let block = Block::bordered()
            .title(title)
            .title_alignment(Alignment::Center)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(border_color));

        let inner = block.inner(dialog_area);
        frame.render_widget(block, dialog_area);
        inner
    }

    fn render_validation_dialog(&self, frame: &mut Frame, area: Rect, error_msg: &str) {
        let inner = Self::render_centered_dialog(frame, area, 60, " Validation Error ", Color::Red);

        // Truncate error message to fit
        let max_msg_len = inner.width.saturating_sub(2) as usize;
        let truncated = if error_msg.len() > max_msg_len {
            format!("{}...", &error_msg[..max_msg_len.saturating_sub(3)])
        } else {
            error_msg.to_string()
        };

        let text = vec![
            Line::from(""),
            Line::from(Span::styled(
                format!("  {truncated}"),
                Style::default().fg(Color::Red),
            )),
            Line::from(""),
            Line::from(vec![
                Span::styled("[K]", Style::default().fg(Color::Yellow).bold()),
                Span::raw(" Keep  "),
                Span::styled("[R]", Style::default().fg(Color::Yellow).bold()),
                Span::raw(" Restore backup"),
            ]),
        ];

        frame.render_widget(Paragraph::new(text), inner);
    }

    fn render_quit_dialog(&self, frame: &mut Frame, area: Rect) {
        let inner =
            Self::render_centered_dialog(frame, area, 56, " Unsaved Changes ", Color::Yellow);

        let text = vec![
            Line::from(""),
            Line::from("  Save before quitting?"),
            Line::from(""),
            Line::from(vec![
                Span::raw("  "),
                Span::styled("[Y]", Style::default().fg(Color::Green).bold()),
                Span::raw(" Save & quit  "),
                Span::styled("[N]", Style::default().fg(Color::Red).bold()),
                Span::raw(" Discard & quit  "),
                Span::styled("[C]", Style::default().fg(Color::Yellow).bold()),
                Span::raw(" Cancel"),
            ]),
        ];

        frame.render_widget(Paragraph::new(text), inner);
    }

    fn render_create_local_dialog(&self, frame: &mut Frame, area: Rect) {
        let inner = Self::render_centered_dialog(
            frame,
            area,
            64,
            " Create repo-local config ",
            Color::Cyan,
        );

        let text = vec![
            Line::from(""),
            Line::from("  Create .ags/config.toml now for this repo?"),
            Line::from(""),
            Line::from(vec![
                Span::raw("  "),
                Span::styled("[C]", Style::default().fg(Color::Green).bold()),
                Span::raw(" Create now  "),
                Span::styled("[L]", Style::default().fg(Color::Yellow).bold()),
                Span::raw(" Later (create on save)  "),
                Span::styled("[Esc]", Style::default().fg(Color::Red).bold()),
                Span::raw(" Cancel"),
            ]),
        ];

        frame.render_widget(Paragraph::new(text), inner);
    }

    // -------------------------------------------------------------------
    // Key handling
    // -------------------------------------------------------------------

    fn handle_key(&mut self, key: KeyEvent) {
        // Edit mode takes top priority
        match &self.edit_mode {
            EditMode::Search { .. } => {
                self.handle_key_search(key);
                return;
            }
            EditMode::EditingField { .. } => {
                self.handle_key_editing_field(key);
                return;
            }
            EditMode::AddingEntry { .. } => {
                self.handle_key_adding_entry(key);
                return;
            }
            EditMode::ConfirmDelete { .. } => {
                self.handle_key_confirm_delete(key);
                return;
            }
            EditMode::None => {}
        }

        // Dialog handling takes priority
        match &self.dialog {
            DialogState::QuitConfirm => {
                match key.code {
                    KeyCode::Char('y') | KeyCode::Char('Y') => {
                        self.dialog = DialogState::None;
                        self.quit_after_validation_dialog = false;
                        match self.handle_save() {
                            SaveOutcome::Saved => self.running = false,
                            SaveOutcome::ValidationError => {
                                self.quit_after_validation_dialog = true;
                            }
                            SaveOutcome::SaveFailed => {}
                        }
                    }
                    KeyCode::Char('n') | KeyCode::Char('N') => {
                        self.dialog = DialogState::None;
                        self.quit_after_validation_dialog = false;
                        self.running = false;
                    }
                    KeyCode::Char('c') | KeyCode::Char('C') | KeyCode::Esc => {
                        self.dialog = DialogState::None;
                        self.quit_after_validation_dialog = false;
                        self.status_message = None;
                    }
                    _ => {}
                }
                return;
            }
            DialogState::ValidationError(_) => {
                match key.code {
                    KeyCode::Char('k') => {
                        // Keep the saved file as-is
                        self.status_message = Some((
                            "Saved (validation errors kept).".into(),
                            StatusKind::Warning,
                        ));
                        self.dialog = DialogState::None;
                        if self.quit_after_validation_dialog {
                            self.quit_after_validation_dialog = false;
                            self.running = false;
                        }
                    }
                    KeyCode::Char('r') => {
                        // Restore from backup
                        self.handle_undo();
                        self.dialog = DialogState::None;
                        if self.quit_after_validation_dialog {
                            self.quit_after_validation_dialog = false;
                            self.running = false;
                        }
                    }
                    _ => {}
                }
                return;
            }
            DialogState::CreateLocalPrompt => {
                match key.code {
                    KeyCode::Char('c') | KeyCode::Char('C') | KeyCode::Enter => {
                        match self.state.create_local_if_missing() {
                            Ok(()) => {
                                self.dialog = DialogState::None;
                                self.invalidate_cache();
                                self.status_message = Some((
                                    "Created .ags/config.toml for this repo.".into(),
                                    StatusKind::Success,
                                ));
                            }
                            Err(error) => {
                                self.dialog = DialogState::None;
                                self.status_message = Some((
                                    format!("Failed to create local config: {error}"),
                                    StatusKind::Error,
                                ));
                            }
                        }
                    }
                    KeyCode::Char('l') | KeyCode::Char('L') => {
                        self.dialog = DialogState::None;
                        self.status_message = Some((
                            "Local draft selected. Save later to create .ags/config.toml.".into(),
                            StatusKind::Info,
                        ));
                    }
                    KeyCode::Esc => {
                        self.dialog = DialogState::None;
                        self.state.edit_target = EditTarget::Global;
                        self.status_message =
                            Some(("Stayed on Global config.".into(), StatusKind::Info));
                        self.invalidate_cache();
                    }
                    _ => {}
                }
                return;
            }
            DialogState::None => {}
        }

        // Global shortcuts first
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('t') => {
                    if !self.repo_local_available {
                        self.status_message = Some((
                            "Repo-local target is disabled because this directory is not in a git repo."
                                .into(),
                            StatusKind::Warning,
                        ));
                        return;
                    }

                    self.state.toggle_target();
                    if self.state.local_missing_on_disk() {
                        self.dialog = DialogState::CreateLocalPrompt;
                        self.status_message = Some((
                            "Repo-local config is missing. Create it now or keep a local draft until save."
                                .into(),
                            StatusKind::Warning,
                        ));
                    } else {
                        let label = match self.state.edit_target {
                            EditTarget::Global => "Global (~/.config/ags/config.toml)",
                            EditTarget::Local => "Local (.ags/config.toml)",
                        };
                        self.status_message =
                            Some((format!("Target: {}", label), StatusKind::Info));
                    }
                    self.selected_field = 0;
                    self.invalidate_cache();
                    return;
                }
                KeyCode::Char('v') => {
                    self.state.view_mode = match self.state.view_mode {
                        ViewMode::Raw => ViewMode::Merged,
                        ViewMode::Merged => ViewMode::Raw,
                    };
                    let label = match self.state.view_mode {
                        ViewMode::Raw => "Raw",
                        ViewMode::Merged => "Merged",
                    };
                    self.status_message = Some((format!("View: {}", label), StatusKind::Info));
                    self.selected_field = 0;
                    self.invalidate_cache();
                    return;
                }
                KeyCode::Char('s') => {
                    let _ = self.handle_save();
                    return;
                }
                KeyCode::Char('z') => {
                    self.handle_undo();
                    return;
                }
                _ => {}
            }
        }

        // Help toggle
        if key.code == KeyCode::Char('?') {
            self.show_help = !self.show_help;
            return;
        }
        if self.show_help {
            // Any key besides ? closes help
            self.show_help = false;
            return;
        }

        match key.code {
            KeyCode::Char('/') => {
                let mut input = Input::new(self.search_query.clone());
                input.handle(InputRequest::GoToEnd);
                self.edit_mode = EditMode::Search { input };
            }
            KeyCode::Char('q') => {
                if self.state.modified {
                    self.dialog = DialogState::QuitConfirm;
                    self.status_message = Some((
                        "Unsaved changes. Save before quitting? [Y]es / [N]o / [C]ancel".into(),
                        StatusKind::Warning,
                    ));
                } else {
                    self.running = false;
                }
            }
            KeyCode::Tab => {
                self.focus = match self.focus {
                    Focus::Sidebar => Focus::MainPanel,
                    Focus::MainPanel => Focus::Sidebar,
                };
                self.selected_field = 0;
            }
            KeyCode::BackTab => {
                // Shift-Tab always goes to sidebar
                if self.focus == Focus::MainPanel {
                    self.focus = Focus::Sidebar;
                    self.selected_field = 0;
                }
            }
            KeyCode::Esc => {
                self.focus = Focus::Sidebar;
                self.selected_field = 0;
                self.status_message = None;
            }
            KeyCode::Char('j') | KeyCode::Down => self.move_down(),
            KeyCode::Char('k') | KeyCode::Up => self.move_up(),
            KeyCode::Enter => {
                if self.focus == Focus::Sidebar {
                    self.focus = Focus::MainPanel;
                    self.selected_field = 0;
                } else if self.focus == Focus::MainPanel {
                    if self.state.view_mode == ViewMode::Merged {
                        self.jump_to_origin();
                    } else {
                        let info = &SECTIONS[self.selected_section];
                        if info.is_array && !self.is_agents_section() {
                            self.start_edit_entry();
                        } else if !info.is_array && !self.is_agents_section() {
                            self.start_edit_field();
                        }
                    }
                }
            }
            KeyCode::Char(' ') => {
                if self.focus == Focus::MainPanel {
                    if self.is_agents_section() {
                        if let Some(idx) = self
                            .filtered_agent_indices()
                            .get(self.selected_field)
                            .copied()
                        {
                            self.toggle_agent(idx);
                        }
                    } else {
                        self.toggle_scalar_field();
                    }
                }
            }
            KeyCode::Char('a') => {
                if self.focus == Focus::MainPanel {
                    self.start_add_entry();
                }
            }
            KeyCode::Char('d') => {
                if self.focus == Focus::MainPanel {
                    self.start_delete_entry();
                }
            }
            _ => {}
        }
    }

    fn handle_key_editing_field(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('g')) {
            self.accept_suggestion();
            return;
        }

        match key.code {
            KeyCode::Enter => {
                self.confirm_edit_field();
                return;
            }
            KeyCode::Esc => {
                self.edit_mode = EditMode::None;
                return;
            }
            _ => {}
        }

        if let Some(request) = key_to_input_request(key.code, true) {
            if let EditMode::EditingField { input, .. } = &mut self.edit_mode {
                input.handle(request);
            }
        }
    }

    fn handle_key_adding_entry(&mut self, key: KeyEvent) {
        let kind_is_text = match &self.edit_mode {
            EditMode::AddingEntry {
                fields,
                active_field,
                ..
            } => matches!(fields[*active_field].kind, FieldKind::Text),
            _ => return,
        };

        if key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('g')) {
            self.accept_suggestion();
            return;
        }

        match key.code {
            KeyCode::Esc => {
                self.edit_mode = EditMode::None;
                self.status_message = None;
                return;
            }
            KeyCode::Tab => {
                if let EditMode::AddingEntry {
                    active_field,
                    fields,
                    ..
                } = &mut self.edit_mode
                {
                    if *active_field + 1 < fields.len() {
                        *active_field += 1;
                    }
                }
                return;
            }
            KeyCode::BackTab => {
                if let EditMode::AddingEntry { active_field, .. } = &mut self.edit_mode {
                    *active_field = active_field.saturating_sub(1);
                }
                return;
            }
            KeyCode::Enter => {
                let is_last = match &self.edit_mode {
                    EditMode::AddingEntry {
                        active_field,
                        fields,
                        ..
                    } => *active_field + 1 >= fields.len(),
                    _ => false,
                };
                if is_last {
                    self.confirm_add_entry();
                } else if let EditMode::AddingEntry { active_field, .. } = &mut self.edit_mode {
                    *active_field += 1;
                }
                return;
            }
            KeyCode::Char(' ') if !kind_is_text => {
                if let EditMode::AddingEntry {
                    fields,
                    active_field,
                    ..
                } = &mut self.edit_mode
                {
                    let field = &mut fields[*active_field];
                    match &field.kind {
                        FieldKind::Toggle(options) => {
                            let current = field.input.value().to_string();
                            let idx = options.iter().position(|&o| o == current).unwrap_or(0);
                            let next = options[(idx + 1) % options.len()];
                            field.input = Input::new(next.to_string());
                        }
                        FieldKind::Checkbox => {
                            let next = if field.input.value() == "true" {
                                "false"
                            } else {
                                "true"
                            };
                            field.input = Input::new(next.to_string());
                        }
                        FieldKind::Text => {}
                    }
                }
                return;
            }
            _ => {}
        }

        // Forward text-editing keys to the active field's input
        if kind_is_text {
            if let Some(request) = key_to_input_request(key.code, false) {
                if let EditMode::AddingEntry {
                    fields,
                    active_field,
                    ..
                } = &mut self.edit_mode
                {
                    fields[*active_field].input.handle(request);
                }
            }
        }
    }

    fn handle_key_confirm_delete(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('y') | KeyCode::Char('Y') => {
                self.confirm_delete();
            }
            _ => {
                self.edit_mode = EditMode::None;
                self.status_message = None;
            }
        }
    }

    fn handle_key_search(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Enter => {
                if let EditMode::Search { input } = &self.edit_mode {
                    self.search_query = input.value().to_string();
                }
                self.edit_mode = EditMode::None;
                self.selected_field = 0;
                self.normalize_selection_for_filters();
                self.status_message = if self.search_query.trim().is_empty() {
                    Some(("Search cleared.".into(), StatusKind::Info))
                } else {
                    Some((format!("Filter: {}", self.search_query), StatusKind::Info))
                };
                return;
            }
            KeyCode::Esc => {
                self.search_query.clear();
                self.edit_mode = EditMode::None;
                self.selected_field = 0;
                self.normalize_selection_for_filters();
                self.status_message = Some(("Search cleared.".into(), StatusKind::Info));
                return;
            }
            _ => {}
        }

        if let Some(request) = key_to_input_request(key.code, true) {
            if let EditMode::Search { input } = &mut self.edit_mode {
                input.handle(request);
            }
        }
    }

    // -------------------------------------------------------------------
    // Save / Undo
    // -------------------------------------------------------------------

    fn set_doctor_status(&mut self, prefix: &str, doctor: crate::cmd::doctor::DoctorSummary) {
        if doctor.all_clear() {
            self.status_message = Some((
                format!("{prefix} Doctor: all checks passed."),
                StatusKind::Success,
            ));
        } else if doctor.fail_count == 0 {
            self.status_message = Some((
                format!("{prefix} Doctor: {} warnings.", doctor.warn_count),
                StatusKind::Warning,
            ));
        } else {
            self.status_message = Some((
                format!(
                    "{prefix} Doctor: {} warnings, {} failures.",
                    doctor.warn_count, doctor.fail_count
                ),
                StatusKind::Warning,
            ));
        }
    }

    fn handle_save(&mut self) -> SaveOutcome {
        let creating_local = self.state.local_missing_on_disk();

        if let Err(e) = self.state.save() {
            self.status_message = Some((format!("Save failed: {e}"), StatusKind::Error));
            return SaveOutcome::SaveFailed;
        }

        let outcome = match self.state.validate_active() {
            Ok(config) => {
                let doctor = crate::cmd::doctor::summarize(&config);
                let prefix = match self.state.edit_target {
                    EditTarget::Global => "Saved. Validation passed.",
                    EditTarget::Local if creating_local => {
                        "Saved. Created .ags/config.toml and layered validation passed."
                    }
                    EditTarget::Local => "Saved. Layered validation passed.",
                };
                self.set_doctor_status(prefix, doctor);
                SaveOutcome::Saved
            }
            Err(e) => {
                let detail = e.to_string();
                self.dialog = DialogState::ValidationError(detail.clone());
                let message = match self.state.edit_target {
                    EditTarget::Global => {
                        format!("Saved with validation error: {detail}")
                    }
                    EditTarget::Local => {
                        format!("Saved with layered validation error: {detail}")
                    }
                };
                self.status_message = Some((message, StatusKind::Error));
                SaveOutcome::ValidationError
            }
        };
        self.invalidate_cache();
        outcome
    }

    fn handle_undo(&mut self) {
        match self.state.undo() {
            Ok(true) => {
                self.selected_field = 0;
                self.invalidate_cache();
                match self.state.validate_active() {
                    Ok(config) => {
                        let doctor = crate::cmd::doctor::summarize(&config);
                        let prefix = match self.state.edit_target {
                            EditTarget::Global => "Restored from backup. Validation passed.",
                            EditTarget::Local => {
                                "Restored from backup. Layered validation passed."
                            }
                        };
                        self.set_doctor_status(prefix, doctor);
                    }
                    Err(e) => {
                        self.status_message = Some((
                            format!("Restored from backup, but validation still fails: {e}"),
                            StatusKind::Warning,
                        ));
                    }
                }
            }
            Ok(false) => {
                self.status_message = Some(("No backup available.".into(), StatusKind::Warning));
            }
            Err(e) => {
                self.status_message = Some((format!("Undo failed: {e}"), StatusKind::Error));
            }
        }
    }

    // -------------------------------------------------------------------
    // Navigation
    // -------------------------------------------------------------------

    fn move_down(&mut self) {
        match self.focus {
            Focus::Sidebar => {
                let visible = self.filtered_section_indices();
                if let Some(pos) = visible
                    .iter()
                    .position(|index| *index == self.selected_section)
                {
                    if let Some(next) = visible.get(pos + 1).copied() {
                        self.selected_section = next;
                        self.selected_field = 0;
                    }
                } else if let Some(first) = visible.first().copied() {
                    self.selected_section = first;
                    self.selected_field = 0;
                }
            }
            Focus::MainPanel => {
                let max = self.current_item_count();
                if max > 0 && self.selected_field + 1 < max {
                    self.selected_field += 1;
                }
            }
        }
    }

    fn move_up(&mut self) {
        match self.focus {
            Focus::Sidebar => {
                let visible = self.filtered_section_indices();
                if let Some(pos) = visible
                    .iter()
                    .position(|index| *index == self.selected_section)
                {
                    if pos > 0 {
                        self.selected_section = visible[pos - 1];
                        self.selected_field = 0;
                    }
                } else if let Some(first) = visible.first().copied() {
                    self.selected_section = first;
                    self.selected_field = 0;
                }
            }
            Focus::MainPanel => {
                self.selected_field = self.selected_field.saturating_sub(1);
            }
        }
    }

    fn current_item_count(&self) -> usize {
        if self.is_agents_section() {
            return self.filtered_agent_indices().len();
        }
        let content = self.filtered_section_content(self.selected_section);
        match &*content {
            SectionContent::Scalar(fields, _) => fields.len(),
            SectionContent::Array(entries) => entries.len(),
        }
    }
}

// ---------------------------------------------------------------------------
// Form field builders
// ---------------------------------------------------------------------------

fn form_field(
    label: &'static str,
    key: &'static str,
    kind: FieldKind,
    required: bool,
    value: String,
) -> FormField {
    FormField {
        label,
        key,
        kind,
        required,
        input: Input::new(value),
    }
}

fn bool_form_value(table: Option<&toml_edit::Table>, key: &str, default: bool) -> String {
    table
        .and_then(|table| table.get(key))
        .and_then(|item| item.as_bool())
        .unwrap_or(default)
        .to_string()
}

fn string_form_value(table: Option<&toml_edit::Table>, key: &str, default: &str) -> String {
    table
        .and_then(|table| table.get(key))
        .and_then(|item| item.as_str())
        .unwrap_or(default)
        .to_string()
}

fn kv_table_form_value(table: Option<&toml_edit::Table>, key: &str) -> String {
    table
        .and_then(|table| table.get(key))
        .and_then(|item| item.as_inline_table())
        .map(|inline| {
            inline
                .iter()
                .map(|(k, v)| format!("{}={}", k, v.as_str().unwrap_or(&v.to_string())))
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default()
}

fn serialize_nested_mount_entries(table: Option<&toml_edit::Table>) -> String {
    table
        .and_then(|table| table.get("directory"))
        .and_then(|item| item.as_array_of_tables())
        .map(|entries| {
            entries
                .iter()
                .map(|entry| {
                    serialize_flattened_table_entry(
                        entry,
                        &[
                            "host",
                            "container",
                            "mode",
                            "kind",
                            "when",
                            "create",
                            "optional",
                            "source",
                        ],
                        &[],
                    )
                })
                .collect::<Vec<_>>()
                .join("; ")
        })
        .unwrap_or_default()
}

fn serialize_nested_secret_entries(table: Option<&toml_edit::Table>) -> String {
    table
        .and_then(|table| table.get("secret"))
        .and_then(|item| item.as_array_of_tables())
        .map(|entries| {
            entries
                .iter()
                .map(|entry| {
                    serialize_flattened_table_entry(
                        entry,
                        &["env", "from_env", "provider", "var"],
                        &["secret_store", "attributes"],
                    )
                })
                .collect::<Vec<_>>()
                .join("; ")
        })
        .unwrap_or_default()
}

fn serialize_flattened_table_entry(
    table: &toml_edit::Table,
    preferred_scalar_keys: &[&str],
    preferred_inline_keys: &[&str],
) -> String {
    let mut parts = Vec::new();

    for key in preferred_scalar_keys {
        if let Some(item) = table.get(*key) {
            if let Some(part) = serialize_flattened_scalar_part(key, item) {
                parts.push(part);
            }
        }
    }

    for (key, item) in table.iter() {
        if preferred_scalar_keys.contains(&key) || preferred_inline_keys.contains(&key) {
            continue;
        }
        if let Some(part) = serialize_flattened_scalar_part(key, item) {
            parts.push(part);
        }
    }

    for key in preferred_inline_keys {
        if let Some(inline) = table.get(*key).and_then(|item| item.as_inline_table()) {
            parts.extend(serialize_inline_table_parts(key, inline));
        }
    }

    for (key, item) in table.iter() {
        if preferred_inline_keys.contains(&key) {
            continue;
        }
        if let Some(inline) = item.as_inline_table() {
            parts.extend(serialize_inline_table_parts(key, inline));
        }
    }

    parts.join(", ")
}

fn serialize_flattened_scalar_part(key: &str, item: &toml_edit::Item) -> Option<String> {
    let value = item.as_value()?;
    if matches!(value, toml_edit::Value::InlineTable(_)) {
        return None;
    }
    Some(format!("{key}={}", serialize_flattened_value(value)))
}

fn serialize_inline_table_parts(prefix: &str, inline: &toml_edit::InlineTable) -> Vec<String> {
    inline
        .iter()
        .map(|(key, value)| format!("{}.{}={}", prefix, key, serialize_flattened_value(value)))
        .collect()
}

fn serialize_flattened_value(value: &toml_edit::Value) -> String {
    value
        .as_str()
        .map(ToOwned::to_owned)
        .unwrap_or_else(|| value.to_string())
}

/// Shared fields for `[[mount]]` and `[[agent_mount]]`: host, container, kind.
fn base_mount_fields(table: Option<&toml_edit::Table>) -> Vec<FormField> {
    vec![
        form_field(
            "host",
            "host",
            FieldKind::Text,
            true,
            string_form_value(table, "host", ""),
        ),
        form_field(
            "container",
            "container",
            FieldKind::Text,
            true,
            string_form_value(table, "container", ""),
        ),
        form_field(
            "kind",
            "kind",
            FieldKind::Toggle(&["dir", "file"]),
            false,
            string_form_value(table, "kind", "dir"),
        ),
    ]
}

fn build_entry_form_fields(section_key: &str, table: Option<&toml_edit::Table>) -> Vec<FormField> {
    match section_key {
        "mount" => {
            let mut fields = base_mount_fields(table);
            fields.insert(
                2,
                form_field(
                    "mode",
                    "mode",
                    FieldKind::Toggle(&["ro", "rw"]),
                    false,
                    string_form_value(table, "mode", "ro"),
                ),
            );
            fields.insert(
                4,
                form_field(
                    "when",
                    "when",
                    FieldKind::Toggle(&["always", "browser"]),
                    false,
                    string_form_value(table, "when", "always"),
                ),
            );
            fields.push(form_field(
                "source",
                "source",
                FieldKind::Text,
                false,
                string_form_value(table, "source", "config"),
            ));
            fields.push(form_field(
                "create",
                "create",
                FieldKind::Checkbox,
                false,
                bool_form_value(table, "create", false),
            ));
            fields.push(form_field(
                "optional",
                "optional",
                FieldKind::Checkbox,
                false,
                bool_form_value(table, "optional", false),
            ));
            fields
        }
        "agent_mount" => base_mount_fields(table),
        "secret" => vec![
            form_field(
                "env",
                "env",
                FieldKind::Text,
                true,
                string_form_value(table, "env", ""),
            ),
            form_field(
                "from_env",
                "from_env",
                FieldKind::Text,
                false,
                string_form_value(table, "from_env", ""),
            ),
            form_field(
                "secret_store",
                "secret_store",
                FieldKind::Text,
                false,
                kv_table_form_value(table, "secret_store"),
            ),
            form_field(
                "provider",
                "provider",
                FieldKind::Text,
                false,
                string_form_value(table, "provider", ""),
            ),
            form_field(
                "var",
                "var",
                FieldKind::Text,
                false,
                string_form_value(table, "var", ""),
            ),
            form_field(
                "attributes",
                "attributes",
                FieldKind::Text,
                false,
                kv_table_form_value(table, "attributes"),
            ),
        ],
        "tool" => vec![
            form_field(
                "name",
                "name",
                FieldKind::Text,
                true,
                string_form_value(table, "name", ""),
            ),
            form_field(
                "path",
                "path",
                FieldKind::Text,
                true,
                string_form_value(table, "path", ""),
            ),
            form_field(
                "container_path",
                "container_path",
                FieldKind::Text,
                true,
                string_form_value(table, "container_path", ""),
            ),
            form_field(
                "mode",
                "mode",
                FieldKind::Toggle(&["ro", "rw"]),
                false,
                string_form_value(table, "mode", "ro"),
            ),
            form_field(
                "when",
                "when",
                FieldKind::Toggle(&["always", "browser"]),
                false,
                string_form_value(table, "when", "always"),
            ),
            form_field(
                "optional",
                "optional",
                FieldKind::Checkbox,
                false,
                bool_form_value(table, "optional", false),
            ),
            form_field(
                "directories",
                "directories",
                FieldKind::Text,
                false,
                serialize_nested_mount_entries(table),
            ),
            form_field(
                "secrets",
                "secrets",
                FieldKind::Text,
                false,
                serialize_nested_secret_entries(table),
            ),
        ],
        _ => vec![],
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn suggestion_for_field(field_key: &str, input: &str, cache: &SuggestionCache<'_>) -> Option<String> {
    let query = input.trim();
    if query.is_empty() {
        return None;
    }

    match field_key {
        "command" | "binary" => suggest_binaries_from(query, cache.binaries)
            .into_iter()
            .next()
            .map(|s| s.value),
        "host" | "path" | "containerfile" | "cache_dir" | "gitconfig_path" | "auth_key"
        | "sign_key" | "profile_dir" | "renderer_bin" => {
            suggest_paths_from(query, cache.home_dirs)
                .into_iter()
                .next()
                .map(|s| s.value)
        }
        _ => None,
    }
}

fn ensure_table(doc: &mut toml_edit::DocumentMut, section_key: &str) {
    if doc
        .get(section_key)
        .and_then(|item| item.as_table_like())
        .is_none()
    {
        doc[section_key] = toml_edit::Item::Table(toml_edit::Table::new());
    }
}

fn missing_scalar_value(schema: &ScalarFieldSchema) -> String {
    if schema.default_input.is_empty() {
        "<unset>".to_string()
    } else {
        match schema.kind {
            ScalarFieldKind::StringList => format!("<default: [{}]>", schema.default_input),
            _ => format!("<default: {}>", schema.default_input),
        }
    }
}

fn item_to_editor_text(item: &toml_edit::Item, schema: &ScalarFieldSchema) -> String {
    match schema.kind {
        ScalarFieldKind::Text | ScalarFieldKind::Enum(_) => item
            .as_str()
            .map(ToOwned::to_owned)
            .unwrap_or_else(|| format_toml_value(item)),
        ScalarFieldKind::Bool => item
            .as_bool()
            .map(|value| value.to_string())
            .unwrap_or_else(|| schema.default_input.to_string()),
        ScalarFieldKind::Number { .. } => item
            .as_integer()
            .map(|value| value.to_string())
            .or_else(|| item.as_float().map(|value| value.to_string()))
            .unwrap_or_else(|| schema.default_input.to_string()),
        ScalarFieldKind::StringList => item
            .as_array()
            .map(|array| {
                array
                    .iter()
                    .filter_map(|value| value.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_else(|| schema.default_input.to_string()),
    }
}

fn parse_string_list(input: &str) -> Vec<String> {
    input
        .split(',')
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn parse_key_value_pairs(input: &str) -> Result<Vec<(String, String)>, String> {
    let mut pairs = Vec::new();
    for pair in input
        .split(',')
        .map(str::trim)
        .filter(|pair| !pair.is_empty())
    {
        let Some((key, value)) = pair.split_once('=') else {
            return Err(format!("expected key=value pair, got '{pair}'"));
        };
        let key = key.trim();
        let value = value.trim();
        if key.is_empty() || value.is_empty() {
            return Err(format!("expected non-empty key=value pair, got '{pair}'"));
        }
        pairs.push((key.to_string(), value.to_string()));
    }
    Ok(pairs)
}

fn set_inline_table(entry: &mut toml_edit::Table, key: &str, input: &str) -> Result<(), String> {
    if input.trim().is_empty() {
        entry.remove(key);
        return Ok(());
    }

    let pairs = parse_key_value_pairs(input)?;
    let mut inline = toml_edit::InlineTable::new();
    for (attr_key, attr_value) in pairs {
        inline.insert(
            attr_key.as_str(),
            toml_edit::Value::from(attr_value.as_str()),
        );
    }
    entry[key] = toml_edit::Item::Value(toml_edit::Value::InlineTable(inline));
    Ok(())
}

struct FlattenedEntryParts {
    scalars: Vec<(String, String)>,
    inline_tables: BTreeMap<String, Vec<(String, String)>>,
}

fn parse_flattened_entries(input: &str) -> Result<Vec<FlattenedEntryParts>, String> {
    input
        .split(';')
        .map(str::trim)
        .filter(|entry| !entry.is_empty())
        .map(|entry| {
            let mut parsed = FlattenedEntryParts {
                scalars: Vec::new(),
                inline_tables: BTreeMap::new(),
            };
            for (key, value) in parse_key_value_pairs(entry)? {
                if let Some((prefix, subkey)) = key.split_once('.') {
                    parsed
                        .inline_tables
                        .entry(prefix.to_string())
                        .or_default()
                        .push((subkey.to_string(), value));
                } else {
                    parsed.scalars.push((key, value));
                }
            }
            Ok(parsed)
        })
        .collect()
}

fn flattened_scalar_item(raw: &str) -> toml_edit::Item {
    if let Ok(value) = raw.parse::<bool>() {
        return toml_edit::value(value);
    }
    if let Ok(value) = raw.parse::<i64>() {
        return toml_edit::value(value);
    }
    if let Ok(value) = raw.parse::<f64>() {
        return toml_edit::value(value);
    }
    toml_edit::value(raw)
}

fn set_flattened_nested_entries(
    entry: &mut toml_edit::Table,
    nested_key: &str,
    input: &str,
) -> Result<(), String> {
    if input.trim().is_empty() {
        entry.remove(nested_key);
        return Ok(());
    }

    let mut nested_entries = toml_edit::ArrayOfTables::new();
    for parsed in parse_flattened_entries(input)? {
        let mut nested_entry = toml_edit::Table::new();
        for (key, value) in parsed.scalars {
            nested_entry[key.as_str()] = flattened_scalar_item(&value);
        }
        for (inline_key, pairs) in parsed.inline_tables {
            let mut inline = toml_edit::InlineTable::new();
            for (key, value) in pairs {
                let item = flattened_scalar_item(&value);
                if let Some(value) = item.as_value() {
                    inline.insert(key.as_str(), value.clone());
                }
            }
            nested_entry[inline_key.as_str()] =
                toml_edit::Item::Value(toml_edit::Value::InlineTable(inline));
        }
        nested_entries.push(nested_entry);
    }

    entry[nested_key] = toml_edit::Item::ArrayOfTables(nested_entries);
    Ok(())
}

fn set_nested_mount_entries(entry: &mut toml_edit::Table, input: &str) -> Result<(), String> {
    set_flattened_nested_entries(entry, "directory", input)
}

fn set_nested_secret_entries(entry: &mut toml_edit::Table, input: &str) -> Result<(), String> {
    set_flattened_nested_entries(entry, "secret", input)
}

fn apply_entry_form(
    section_key: &str,
    entry: &mut toml_edit::Table,
    field_values: &[(&'static str, FieldKind, String)],
) -> Result<(), String> {
    for &(key, kind, ref value) in field_values {
        match (key, kind) {
            ("secret_store", _) | ("attributes", _) => set_inline_table(entry, key, value)?,
            ("directories", _) => set_nested_mount_entries(entry, value)?,
            ("secrets", _) => set_nested_secret_entries(entry, value)?,
            (_, FieldKind::Checkbox) => {
                entry[key] = toml_edit::value(value == "true");
            }
            (_, FieldKind::Toggle(_)) => {
                entry[key] = toml_edit::value(value.as_str());
            }
            (_, FieldKind::Text) => {
                if value.trim().is_empty() {
                    entry.remove(key);
                } else {
                    entry[key] = toml_edit::value(value.as_str());
                }
            }
        }
    }

    if section_key == "secret" {
        let env = entry
            .get("env")
            .and_then(|item| item.as_str())
            .unwrap_or_default()
            .to_string();
        let has_from_env = entry
            .get("from_env")
            .and_then(|item| item.as_str())
            .is_some();
        let has_secret_store = entry.get("secret_store").is_some();
        let has_provider = entry
            .get("provider")
            .and_then(|item| item.as_str())
            .is_some();
        if !env.is_empty() && !has_from_env && !has_secret_store && !has_provider {
            entry["from_env"] = toml_edit::value(env);
        }
    }

    Ok(())
}

fn apply_scalar_value(
    doc: &mut toml_edit::DocumentMut,
    section_key: &str,
    field_key: &str,
    kind: ScalarFieldKind,
    new_text: &str,
) -> Result<(), String> {
    ensure_table(doc, section_key);

    match kind {
        ScalarFieldKind::Text => {
            doc[section_key][field_key] = toml_edit::value(new_text);
        }
        ScalarFieldKind::Enum(options) => {
            if !options.iter().any(|option| *option == new_text) {
                return Err(format!(
                    "{section_key}.{field_key} must be one of: {}",
                    options.join(", ")
                ));
            }
            doc[section_key][field_key] = toml_edit::value(new_text);
        }
        ScalarFieldKind::Bool => {
            let value = new_text
                .parse::<bool>()
                .map_err(|_| format!("{section_key}.{field_key} must be 'true' or 'false'"))?;
            doc[section_key][field_key] = toml_edit::value(value);
        }
        ScalarFieldKind::Number { min, max } => {
            let value = new_text
                .parse::<u64>()
                .map_err(|_| format!("{section_key}.{field_key} must be a whole number"))?;
            if !(min..=max).contains(&value) {
                return Err(format!(
                    "{section_key}.{field_key} must be between {min} and {max}"
                ));
            }
            let value = i64::try_from(value)
                .map_err(|_| format!("{section_key}.{field_key} is too large to store"))?;
            doc[section_key][field_key] = toml_edit::value(value);
        }
        ScalarFieldKind::StringList => {
            let values = parse_string_list(new_text);
            let array = toml_edit::Array::from_iter(values.iter().map(|value| value.as_str()));
            doc[section_key][field_key] = toml_edit::Item::Value(toml_edit::Value::Array(array));
        }
    }

    Ok(())
}

fn next_toggle_action(
    doc: &toml_edit::DocumentMut,
    section_key: &str,
    field_key: &str,
    schema: &ScalarFieldSchema,
) -> Option<ToggleAction> {
    let item = doc.get(section_key).and_then(|table| table.get(field_key));

    match schema.kind {
        ScalarFieldKind::Bool => {
            let current = item
                .and_then(|item| item.as_bool())
                .unwrap_or(schema.default_input == "true");
            Some(ToggleAction::Bool(!current))
        }
        ScalarFieldKind::Enum(options) => {
            let current = item
                .and_then(|item| item.as_str())
                .unwrap_or(schema.default_input);
            let index = options
                .iter()
                .position(|option| *option == current)
                .unwrap_or(0);
            Some(ToggleAction::Str(options[(index + 1) % options.len()]))
        }
        _ => None,
    }
}

fn global_array_len(doc: &toml_edit::DocumentMut, section_key: &str) -> usize {
    doc.get(section_key)
        .and_then(|item| item.as_array_of_tables())
        .map(|entries| entries.len())
        .unwrap_or(0)
}

fn scalar_field_index_for_target(
    state: &ConfigEditorState,
    target: EditTarget,
    section_key: &str,
    field_key: &str,
) -> Option<usize> {
    let doc = match target {
        EditTarget::Global => &state.global_doc,
        EditTarget::Local => &state.local_doc,
    };

    let content = if let Some(section_idx) = SECTIONS
        .iter()
        .position(|section| section.toml_key == section_key)
    {
        let info = &SECTIONS[section_idx];
        let mut fields = Vec::new();
        let table = doc.get(info.toml_key).and_then(|item| item.as_table_like());
        for schema in scalar_fields(info.toml_key) {
            let present = table
                .and_then(|table| table.get(schema.key))
                .is_some_and(|item| !item.is_none());
            fields.push((schema.key, present));
        }
        if let Some(table) = table {
            for (key, _) in table.iter() {
                if scalar_field(info.toml_key, key).is_none() {
                    fields.push((key, true));
                }
            }
        }
        fields
    } else {
        Vec::new()
    };

    content.iter().position(|(key, _)| *key == field_key)
}

fn edit_target_label(target: EditTarget) -> &'static str {
    match target {
        EditTarget::Global => "global",
        EditTarget::Local => "local",
    }
}

fn expand_home_path(path: &str) -> std::path::PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = dirs::home_dir() {
            return home.join(rest);
        }
    }
    std::path::PathBuf::from(path)
}

fn compute_host_status(agent: &AgentDef) -> HostStatus {
    let total = agent.mounts.len();
    if total == 0 {
        return HostStatus::Present;
    }
    let existing = agent
        .mounts
        .iter()
        .filter(|mount| expand_home_path(mount.host).exists())
        .count();
    if existing == 0 {
        HostStatus::Missing
    } else if existing == total {
        HostStatus::Present
    } else {
        HostStatus::Partial(existing, total)
    }
}

/// Format a TOML item as a display string.
fn format_toml_value(item: &toml_edit::Item) -> String {
    match item {
        toml_edit::Item::Value(v) => {
            let s = v.to_string();
            let trimmed = s.trim();
            if trimmed.len() == s.len() {
                s
            } else {
                trimmed.to_owned()
            }
        }
        toml_edit::Item::Table(t) => {
            // Nested table -- show as inline summary
            let keys: Vec<&str> = t.iter().map(|(k, _)| k).collect();
            format!("{{ {} }}", keys.join(", "))
        }
        _ => "(...)".to_string(),
    }
}

/// Map a key code to a tui-input request. `home_end` enables Home/End navigation.
fn key_to_input_request(code: KeyCode, home_end: bool) -> Option<InputRequest> {
    match code {
        KeyCode::Char(c) => Some(InputRequest::InsertChar(c)),
        KeyCode::Backspace => Some(InputRequest::DeletePrevChar),
        KeyCode::Delete => Some(InputRequest::DeleteNextChar),
        KeyCode::Left => Some(InputRequest::GoToPrevChar),
        KeyCode::Right => Some(InputRequest::GoToNextChar),
        KeyCode::Home if home_end => Some(InputRequest::GoToStart),
        KeyCode::End if home_end => Some(InputRequest::GoToEnd),
        _ => None,
    }
}

/// Get a string value from a TOML table, returning a default if missing or non-string.
fn get_str<'a>(table: &'a toml_edit::Table, key: &str, default: &'a str) -> &'a str {
    table.get(key).and_then(|v| v.as_str()).unwrap_or(default)
}

/// Build a one-line summary for an array-of-tables entry based on its section type.
fn summarize_array_entry(toml_key: &str, table: &toml_edit::Table) -> String {
    match toml_key {
        "mount" | "agent_mount" => {
            let host = get_str(table, "host", "?");
            let container = get_str(table, "container", "?");
            let mode = get_str(table, "mode", "ro");
            let kind = get_str(table, "kind", "dir");

            let mut parts = format!("{} -> {}  [{}] [{}]", host, container, mode, kind);

            if table
                .get("optional")
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
            {
                parts.push_str(" [optional]");
            }

            parts
        }
        "tool" => {
            let name = get_str(table, "name", "?");
            let path = get_str(table, "path", "?");
            let container_path = get_str(table, "container_path", "?");
            let mode = get_str(table, "mode", "ro");

            format!("{}  {} -> {}  [{}]", name, path, container_path, mode)
        }
        "secret" => {
            let env = get_str(table, "env", "?");

            if let Some(from_env) = table.get("from_env").and_then(|v| v.as_str()) {
                format!("{}  from_env={}", env, from_env)
            } else if table.get("secret_store").is_some() {
                format!("{}  secret_store", env)
            } else {
                env.to_string()
            }
        }
        _ => {
            // Generic fallback: show all key=value pairs
            let pairs: Vec<String> = table
                .iter()
                .map(|(k, v)| format!("{}={}", k, v.as_str().unwrap_or("...")))
                .collect();
            pairs.join("  ")
        }
    }
}

/// Return (marker_text, style) for a ValueSource, or empty if None (raw view).
fn source_suffix(source: Option<ValueSource>) -> (&'static str, Style) {
    match source {
        None => ("", Style::default()),
        Some(ValueSource::Global) => (" [G]", Style::default().fg(Color::DarkGray)),
        Some(ValueSource::Local) => (" [L]", Style::default().fg(Color::Cyan)),
        Some(ValueSource::LocalOverridesGlobal) => {
            (" [L>G]", Style::default().fg(Color::Yellow).bold())
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::process::Command;

    use crossterm::event::KeyCode;
    use tempfile::TempDir;
    use tui_input::Input;

    use super::{App, Focus, SECTIONS, SectionContent, apply_scalar_value};
    use crate::cmd::config_editor::model::{ConfigEditorState, ViewMode};
    use crate::cmd::config_editor::schema::ScalarFieldKind;

    #[test]
    fn resolve_local_target_path_disables_repo_local_outside_git_repo() {
        let dir = TempDir::new().unwrap();

        let (path, available) = super::super::resolve_local_target_path(Some(dir.path()));

        assert!(!available);
        assert_eq!(path, dir.path().join(".ags/config.toml"));
    }

    #[test]
    fn resolve_local_target_path_uses_repo_root_inside_git_repo() {
        let dir = TempDir::new().unwrap();
        let status = Command::new("git")
            .args(["init", "-q"])
            .current_dir(dir.path())
            .status()
            .unwrap();
        assert!(status.success());

        let nested = dir.path().join("nested/worktree");
        std::fs::create_dir_all(&nested).unwrap();

        let (path, available) = super::super::resolve_local_target_path(Some(&nested));

        assert!(available);
        assert_eq!(path, dir.path().join(".ags/config.toml"));
    }

    #[test]
    fn app_ignores_repo_local_overlay_outside_git_repo() {
        let dir = TempDir::new().unwrap();
        let global = dir.path().join("config.toml");
        fs::write(
            &global,
            r#"[sandbox]
image = "global"
containerfile = "/tmp/Containerfile"
cache_dir = "/tmp/cache"
gitconfig_path = "/tmp/gitconfig"
auth_key = "/tmp/auth"
sign_key = "/tmp/sign"
"#,
        )
        .unwrap();
        let local = dir.path().join(".ags/config.toml");
        fs::create_dir_all(local.parent().unwrap()).unwrap();
        fs::write(&local, "[sandbox]\nimage = \"local\"\n").unwrap();

        let app = App::new_with_cwd(&global, Some(dir.path())).unwrap();

        assert!(!app.repo_local_available);
        assert!(!app.state.local_exists);
        assert!(app.state.local_doc.iter().next().is_none());
        assert_eq!(
            app.state.compute_merged_view()["sandbox"]["image"].as_str(),
            Some("global")
        );
    }

    fn make_test_app(global_toml: &str) -> (TempDir, App) {
        make_test_app_with_local(global_toml, None)
    }

    fn make_test_app_with_local(global_toml: &str, local_toml: Option<&str>) -> (TempDir, App) {
        let dir = TempDir::new().unwrap();
        let global = dir.path().join("config.toml");
        fs::write(&global, global_toml).unwrap();
        let local = dir.path().join(".ags/config.toml");
        if let Some(local_toml) = local_toml {
            fs::create_dir_all(local.parent().unwrap()).unwrap();
            fs::write(&local, local_toml).unwrap();
        }
        let state = ConfigEditorState::load(&global, &local).unwrap();
        let app = App {
            running: true,
            state,
            focus: Focus::Sidebar,
            selected_section: 0,
            selected_field: 0,
            show_help: false,
            search_query: String::new(),
            status_message: None,
            dialog: super::DialogState::None,
            edit_mode: super::EditMode::None,
            content_cache: vec![None; SECTIONS.len()],
            agent_enabled_cache: vec![false; super::KNOWN_AGENTS.len()],
            repo_local_available: local_toml.is_some(),
            quit_after_validation_dialog: false,
            agent_host_status_cache: super::KNOWN_AGENTS
                .iter()
                .map(|a| super::compute_host_status(a))
                .collect(),
            cached_binaries: Some(Vec::new()),
            cached_home_dirs: Some(Vec::new()),
            current_suggestion: None,
        };
        (dir, app)
    }

    #[test]
    fn schema_driven_scalar_sections_show_known_missing_fields() {
        let (_dir, app) = make_test_app(
            r#"[sandbox]
image = "base"
containerfile = "/tmp/Containerfile"
cache_dir = "/tmp/cache"
gitconfig_path = "/tmp/gitconfig"
auth_key = "/tmp/auth"
sign_key = "/tmp/sign"
"#,
        );

        let expectations = [
            (
                "browser",
                vec![
                    "enabled",
                    "command",
                    "profile_dir",
                    "debug_port",
                    "pi_skill_path",
                    "command_args",
                ],
            ),
            ("auth_proxy", vec!["auto_allow_domains"]),
            (
                "host_ui",
                vec![
                    "enabled",
                    "binary",
                    "renderer",
                    "renderer_bin",
                    "idle_timeout_ms",
                    "log_level",
                ],
            ),
            ("psp", vec!["binary"]),
            ("update", vec!["pi_spec", "minimum_release_age"]),
        ];

        for (section_key, expected_keys) in expectations {
            let section_idx = SECTIONS
                .iter()
                .position(|section| section.toml_key == section_key)
                .unwrap();

            let content = app.compute_section_content(section_idx, app.state.active_doc(), false);
            let SectionContent::Scalar(fields, _) = content else {
                panic!("expected scalar content for {section_key}");
            };

            let keys: Vec<_> = fields.iter().map(|field| field.key.as_str()).collect();
            assert_eq!(keys, expected_keys, "section={section_key}");
            assert!(
                fields.iter().all(|field| !field.present),
                "section={section_key}"
            );
        }
    }

    #[test]
    fn apply_scalar_value_creates_typed_list_values() {
        let mut doc = "".parse::<toml_edit::DocumentMut>().unwrap();

        apply_scalar_value(
            &mut doc,
            "sandbox",
            "bootstrap_files",
            ScalarFieldKind::StringList,
            "auth.json, models.json",
        )
        .unwrap();

        let values: Vec<_> = doc["sandbox"]["bootstrap_files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap())
            .collect();
        assert_eq!(values, vec!["auth.json", "models.json"]);
    }

    #[test]
    fn apply_scalar_value_rejects_invalid_numbers() {
        let mut doc = "".parse::<toml_edit::DocumentMut>().unwrap();

        let err = apply_scalar_value(
            &mut doc,
            "browser",
            "debug_port",
            ScalarFieldKind::Number {
                min: 0,
                max: u16::MAX as u64,
            },
            "70000",
        )
        .unwrap_err();

        assert!(err.contains("between 0 and 65535"), "got: {err}");
    }

    #[test]
    fn apply_scalar_value_creates_missing_bool_fields() {
        let mut doc = "".parse::<toml_edit::DocumentMut>().unwrap();

        apply_scalar_value(
            &mut doc,
            "host_ui",
            "enabled",
            ScalarFieldKind::Bool,
            "true",
        )
        .unwrap();

        assert_eq!(doc["host_ui"]["enabled"].as_bool(), Some(true));
    }

    #[test]
    fn apply_scalar_value_supports_enum_fields() {
        let mut doc = "".parse::<toml_edit::DocumentMut>().unwrap();

        apply_scalar_value(
            &mut doc,
            "host_ui",
            "log_level",
            ScalarFieldKind::Enum(&["trace", "debug", "info", "warn", "error"]),
            "warn",
        )
        .unwrap();

        assert_eq!(doc["host_ui"]["log_level"].as_str(), Some("warn"));
    }

    #[test]
    fn merged_scalar_enter_jumps_to_local_raw_origin() {
        let (_dir, mut app) = make_test_app_with_local(
            r#"[sandbox]
image = "global"
containerfile = "/tmp/Containerfile"
cache_dir = "/tmp/cache"
gitconfig_path = "/tmp/gitconfig"
auth_key = "/tmp/auth"
sign_key = "/tmp/sign"
"#,
            Some(
                r#"[sandbox]
image = "local"
"#,
            ),
        );
        app.state.view_mode = ViewMode::Merged;
        app.focus = Focus::MainPanel;
        app.selected_section = SECTIONS
            .iter()
            .position(|section| section.toml_key == "sandbox")
            .unwrap();
        app.ensure_cache();
        app.selected_field = 0;

        app.jump_to_origin();

        assert_eq!(app.state.view_mode, ViewMode::Raw);
        assert_eq!(app.state.edit_target, super::EditTarget::Local);
    }

    #[test]
    fn merged_array_enter_jumps_to_local_raw_origin() {
        let (_dir, mut app) = make_test_app_with_local(
            r#"[sandbox]
image = "global"
containerfile = "/tmp/Containerfile"
cache_dir = "/tmp/cache"
gitconfig_path = "/tmp/gitconfig"
auth_key = "/tmp/auth"
sign_key = "/tmp/sign"

[[mount]]
host = "/global"
container = "/mnt/global"
mode = "ro"
kind = "dir"
"#,
            Some(
                r#"[[mount]]
host = "/local"
container = "/mnt/local"
mode = "rw"
kind = "dir"
"#,
            ),
        );
        app.state.view_mode = ViewMode::Merged;
        app.focus = Focus::MainPanel;
        app.selected_section = SECTIONS
            .iter()
            .position(|section| section.toml_key == "mount")
            .unwrap();
        app.ensure_cache();
        app.selected_field = 1;

        app.jump_to_origin();

        assert_eq!(app.state.view_mode, ViewMode::Raw);
        assert_eq!(app.state.edit_target, super::EditTarget::Local);
        assert_eq!(app.selected_field, 0);
    }

    #[test]
    fn agent_host_status_reports_missing_mounts() {
        let agent = super::AgentDef {
            name: "Test",
            mounts: &[super::super::agents::AgentMountDef {
                host: "/definitely/missing/path",
                container: "/container",
                kind: crate::config::MountKind::Dir,
            }],
        };

        assert!(
            matches!(super::compute_host_status(&agent), super::HostStatus::Missing)
        );
    }

    #[test]
    fn toggle_agent_only_adds_missing_mounts_from_partial_state() {
        let agent = &super::KNOWN_AGENTS[0];
        let seed_mount = &agent.mounts[0];
        let (_dir, mut app) = make_test_app(&format!(
            r#"[sandbox]
image = "base"
containerfile = "/tmp/Containerfile"
cache_dir = "/tmp/cache"
gitconfig_path = "/tmp/gitconfig"
auth_key = "/tmp/auth"
sign_key = "/tmp/sign"

[[agent_mount]]
host = "{}"
container = "{}"
kind = "{}"
"#,
            seed_mount.host, seed_mount.container, seed_mount.kind
        ));

        app.toggle_agent(0);

        let mounts = app.state.global_doc["agent_mount"]
            .as_array_of_tables()
            .unwrap();
        assert_eq!(mounts.len(), agent.mounts.len());
        for mount in agent.mounts {
            let matches = mounts
                .iter()
                .filter(|table| {
                    table.get("host").and_then(|value| value.as_str()) == Some(mount.host)
                        && table.get("container").and_then(|value| value.as_str())
                            == Some(mount.container)
                })
                .count();
            assert_eq!(
                matches, 1,
                "mount should appear exactly once: {}",
                mount.host
            );
        }
    }

    #[test]
    fn search_filters_sidebar_and_current_section() {
        let (_dir, mut app) = make_test_app(
            r#"[sandbox]
image = "base"
containerfile = "/tmp/Containerfile"
cache_dir = "/tmp/cache"
gitconfig_path = "/tmp/gitconfig"
auth_key = "/tmp/auth"
sign_key = "/tmp/sign"
"#,
        );
        app.ensure_cache();
        app.search_query = "browser".to_string();

        let visible_sections = app.filtered_section_indices();
        assert_eq!(visible_sections.len(), 1);
        assert_eq!(SECTIONS[visible_sections[0]].toml_key, "browser");

        app.search_query = "command".to_string();
        let browser_idx = SECTIONS
            .iter()
            .position(|section| section.toml_key == "browser")
            .unwrap();
        let content = app.filtered_section_content(browser_idx);
        let SectionContent::Scalar(fields, _) = &*content else {
            panic!("expected scalar content");
        };
        assert!(fields.iter().any(|field| field.key == "command"));
    }

    #[test]
    fn search_enter_applies_filter_and_escape_clears_it() {
        let (_dir, mut app) = make_test_app(
            r#"[sandbox]
image = "base"
containerfile = "/tmp/Containerfile"
cache_dir = "/tmp/cache"
gitconfig_path = "/tmp/gitconfig"
auth_key = "/tmp/auth"
sign_key = "/tmp/sign"
"#,
        );
        app.ensure_cache();
        app.edit_mode = super::EditMode::Search {
            input: Input::new("browser".to_string()),
        };

        app.handle_key_search(crossterm::event::KeyEvent::from(KeyCode::Enter));
        assert_eq!(app.search_query, "browser");

        app.edit_mode = super::EditMode::Search {
            input: Input::new(app.search_query.clone()),
        };
        app.handle_key_search(crossterm::event::KeyEvent::from(KeyCode::Esc));
        assert!(app.search_query.is_empty());
    }

    #[test]
    fn quit_confirm_save_waits_for_validation_dialog_resolution() {
        let (_dir, mut app) = make_test_app(
            r#"[sandbox]
image = "base"
containerfile = "/tmp/Containerfile"
cache_dir = "/tmp/cache"
gitconfig_path = "/tmp/gitconfig"
auth_key = "/tmp/auth"
sign_key = "/tmp/sign"
"#,
        );
        app.state.global_doc["sandbox"]["auth_key"] = toml_edit::value("");
        app.state.modified = true;
        app.dialog = super::DialogState::QuitConfirm;

        app.handle_key(crossterm::event::KeyEvent::from(KeyCode::Char('y')));

        assert!(app.running, "app should stay open for validation recovery");
        assert!(matches!(app.dialog, super::DialogState::ValidationError(_)));
        assert!(app.quit_after_validation_dialog);

        app.handle_key(crossterm::event::KeyEvent::from(KeyCode::Char('k')));

        assert!(
            !app.running,
            "keeping the invalid file should finish the pending quit"
        );
        assert!(matches!(app.dialog, super::DialogState::None));
    }

    #[test]
    fn apply_entry_form_covers_full_mount_fields() {
        let mut entry = toml_edit::Table::new();
        super::apply_entry_form(
            "mount",
            &mut entry,
            &[
                ("host", super::FieldKind::Text, "/host".to_string()),
                (
                    "container",
                    super::FieldKind::Text,
                    "/container".to_string(),
                ),
                (
                    "mode",
                    super::FieldKind::Toggle(&["ro", "rw"]),
                    "rw".to_string(),
                ),
                (
                    "kind",
                    super::FieldKind::Toggle(&["dir", "file"]),
                    "file".to_string(),
                ),
                (
                    "when",
                    super::FieldKind::Toggle(&["always", "browser"]),
                    "browser".to_string(),
                ),
                ("source", super::FieldKind::Text, "custom".to_string()),
                ("create", super::FieldKind::Checkbox, "true".to_string()),
                ("optional", super::FieldKind::Checkbox, "true".to_string()),
            ],
        )
        .unwrap();

        assert_eq!(entry["host"].as_str(), Some("/host"));
        assert_eq!(entry["container"].as_str(), Some("/container"));
        assert_eq!(entry["mode"].as_str(), Some("rw"));
        assert_eq!(entry["kind"].as_str(), Some("file"));
        assert_eq!(entry["when"].as_str(), Some("browser"));
        assert_eq!(entry["source"].as_str(), Some("custom"));
        assert_eq!(entry["create"].as_bool(), Some(true));
        assert_eq!(entry["optional"].as_bool(), Some(true));
    }

    #[test]
    fn apply_entry_form_preserves_existing_tool_nested_content() {
        let mut entry = toml_edit::Table::new();
        entry["name"] = toml_edit::value("gh");
        entry["path"] = toml_edit::value("/usr/bin/gh");
        entry["container_path"] = toml_edit::value("/usr/local/bin/gh");
        entry["custom"] = toml_edit::value("keep");

        let mut directories = toml_edit::ArrayOfTables::new();
        let mut dir = toml_edit::Table::new();
        dir["host"] = toml_edit::value("/dir");
        dir["container"] = toml_edit::value("/mnt/dir");
        dir["label"] = toml_edit::value("keep-me");
        dir["priority"] = toml_edit::value(7);
        directories.push(dir);
        entry["directory"] = toml_edit::Item::ArrayOfTables(directories);

        let mut secrets = toml_edit::ArrayOfTables::new();
        let mut secret = toml_edit::Table::new();
        secret["env"] = toml_edit::value("TOKEN");
        secret["from_env"] = toml_edit::value("HOST_TOKEN");
        secret["namespace"] = toml_edit::value("ops");
        let mut metadata = toml_edit::InlineTable::new();
        metadata.insert("team", toml_edit::Value::from("platform"));
        secret["metadata"] = toml_edit::Item::Value(toml_edit::Value::InlineTable(metadata));
        secrets.push(secret);
        entry["secret"] = toml_edit::Item::ArrayOfTables(secrets);

        let mut field_values: Vec<_> = super::build_entry_form_fields("tool", Some(&entry))
            .into_iter()
            .map(|field| (field.key, field.kind, field.input.value().to_string()))
            .collect();
        field_values
            .iter_mut()
            .find(|(key, _, _)| *key == "path")
            .unwrap()
            .2 = "/opt/gh".to_string();

        super::apply_entry_form("tool", &mut entry, &field_values).unwrap();

        assert_eq!(entry["path"].as_str(), Some("/opt/gh"));
        assert_eq!(entry["custom"].as_str(), Some("keep"));
        let directory = entry["directory"]
            .as_array_of_tables()
            .unwrap()
            .iter()
            .next()
            .unwrap();
        assert_eq!(directory["label"].as_str(), Some("keep-me"));
        assert_eq!(directory["priority"].as_integer(), Some(7));
        let secret = entry["secret"]
            .as_array_of_tables()
            .unwrap()
            .iter()
            .next()
            .unwrap();
        assert_eq!(secret["namespace"].as_str(), Some("ops"));
        assert_eq!(
            secret["metadata"]
                .as_inline_table()
                .unwrap()
                .get("team")
                .map(|value| value.as_str().unwrap()),
            Some("platform")
        );
    }

    #[test]
    fn apply_entry_form_supports_secret_store_and_legacy_fields() {
        let mut entry = toml_edit::Table::new();
        super::apply_entry_form(
            "secret",
            &mut entry,
            &[
                ("env", super::FieldKind::Text, "TOKEN".to_string()),
                ("from_env", super::FieldKind::Text, "".to_string()),
                (
                    "secret_store",
                    super::FieldKind::Text,
                    "service=github, username=tom".to_string(),
                ),
                (
                    "provider",
                    super::FieldKind::Text,
                    "secret-tool".to_string(),
                ),
                ("var", super::FieldKind::Text, "TOKEN".to_string()),
                (
                    "attributes",
                    super::FieldKind::Text,
                    "service=github, username=tom".to_string(),
                ),
            ],
        )
        .unwrap();

        assert!(entry["secret_store"].is_value());
        assert_eq!(entry["provider"].as_str(), Some("secret-tool"));
        assert_eq!(entry["var"].as_str(), Some("TOKEN"));
        assert!(entry["attributes"].is_value());
    }

    #[test]
    fn apply_entry_form_supports_nested_tool_directories_and_secrets() {
        let mut entry = toml_edit::Table::new();
        super::apply_entry_form(
            "tool",
            &mut entry,
            &[
                ("name", super::FieldKind::Text, "gh".to_string()),
                ("path", super::FieldKind::Text, "/usr/bin/gh".to_string()),
                (
                    "container_path",
                    super::FieldKind::Text,
                    "/usr/local/bin/gh".to_string(),
                ),
                ("mode", super::FieldKind::Toggle(&["ro", "rw"]), "ro".to_string()),
                (
                    "when",
                    super::FieldKind::Toggle(&["always", "browser"]),
                    "always".to_string(),
                ),
                ("optional", super::FieldKind::Checkbox, "false".to_string()),
                (
                    "directories",
                    super::FieldKind::Text,
                    "host=/dir, container=/mnt/dir, mode=rw, kind=dir".to_string(),
                ),
                (
                    "secrets",
                    super::FieldKind::Text,
                    "env=TOKEN, from_env=HOST_TOKEN; env=API_KEY, secret_store.service=github, secret_store.username=tom"
                        .to_string(),
                ),
            ],
        )
        .unwrap();

        assert_eq!(entry["directory"].as_array_of_tables().unwrap().len(), 1);
        assert_eq!(entry["secret"].as_array_of_tables().unwrap().len(), 2);
    }

    #[test]
    fn suggestion_for_binary_field_uses_discovery_presets() {
        assert_eq!(
            super::suggestion_for_field(
                "command",
                "gh",
                &super::SuggestionCache { binaries: &[], home_dirs: &[] },
            ),
            Some("gh".to_string())
        );
    }

    #[test]
    fn suggestion_for_path_field_uses_discovery_presets() {
        assert_eq!(
            super::suggestion_for_field(
                "host",
                "gitconfig",
                &super::SuggestionCache { binaries: &[], home_dirs: &[] },
            ),
            Some("~/.gitconfig".to_string())
        );
    }
}
