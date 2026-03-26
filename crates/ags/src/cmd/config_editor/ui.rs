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

include!("ui_boot.rs");
include!("ui_sections.rs");
include!("ui_edit.rs");
include!("ui_render_main.rs");
include!("ui_render_overlays.rs");
include!("ui_input.rs");
include!("ui_actions.rs");

include!("ui_form_fields.rs");
include!("ui_value_helpers.rs");

#[cfg(test)]
#[path = "ui_tests.rs"]
mod tests;
