# TOML Research Artifact: AGS Visual Config Editor

Research for the config-editor TUI. This document covers library selection, round-trip
guarantees, layered config strategy, edit approach, save/undo, and TUI framework.

---

## 1. TOML Library Recommendation: `toml_edit`

**Recommendation: Use `toml_edit = "0.25"` for the config editor.**

### Why `toml_edit`

`toml_edit` is the only viable Rust crate for format-preserving TOML editing. It parses
TOML into a `DocumentMut` that retains comments, whitespace, key ordering, and formatting.
You modify the document tree programmatically, then serialize back to a string that
preserves everything you didn't touch.

### Key API Surface

```rust
use toml_edit::{DocumentMut, Item, Table, Array, ArrayOfTables, Value};

// Parse
let mut doc: DocumentMut = content.parse()?;

// Read a scalar
let image = doc["sandbox"]["image"].as_str();

// Modify a scalar (preserves surrounding formatting)
doc["sandbox"]["image"] = toml_edit::value("new-image:latest");

// Access array of tables
let mounts: &ArrayOfTables = doc["mount"].as_array_of_tables().unwrap();

// Add a new [[mount]] entry
let mut new_mount = toml_edit::Table::new();
new_mount["host"] = toml_edit::value("~/new/path");
new_mount["container"] = toml_edit::value("/home/dev/new");
new_mount["mode"] = toml_edit::value("ro");
doc["mount"].as_array_of_tables_mut().unwrap().push(new_mount);

// Remove an array-of-tables entry by index
doc["mount"].as_array_of_tables_mut().unwrap().remove(2);

// Insert a new section
doc["new_section"] = toml_edit::Item::Table(toml_edit::Table::new());

// Serialize back to string (comments and formatting preserved)
let output = doc.to_string();
```

### Coexistence with `toml` (serde) crate

The `toml` crate (currently `0.8` in ags, latest is `1.0.7`) and `toml_edit` share the
same underlying parser — `toml` depends on `toml_edit` internally. They coexist cleanly:

- **Existing code** (`parse.rs`): Keep using `toml` for serde deserialization and
  validation. This is the read-and-validate path used at launch time. No changes needed.
- **Config editor**: Use `toml_edit` for loading, displaying, editing, and writing back
  config files. The editor never needs serde — it works at the document level.
- **Validation after save**: After the editor writes a file, run the existing
  `parse_and_validate()` function to confirm the output is valid AGS config.

There is no conflict. Both crates can be in the same dependency tree. The `toml_edit`
dependency adds ~0 extra compile cost since `toml` already pulls it in transitively.

### Version Pinning

```toml
# In the config-editor crate's Cargo.toml
toml_edit = "0.25"
toml = "0.8"  # for re-validation via existing parse.rs
```

---

## 2. Round-Trip Guarantees and Limitations

### What Gets Preserved

When you parse TOML into `DocumentMut`, modify specific nodes, and serialize back:

| Element | Preserved? | Notes |
|---------|-----------|-------|
| Comments (above keys, inline) | Yes | Attached to the nearest following key |
| Blank lines between sections | Yes | Part of the document's whitespace trivia |
| Key ordering within tables | Yes | Insertion order maintained |
| Section ordering | Yes | `[[mount]]` blocks stay in original order |
| Quoting style (bare vs quoted keys) | Yes | |
| String quoting style | Yes | `"double"` vs `'single'` vs `"""multiline"""` |
| Integer formatting (hex, oct, bin) | Yes | |
| Trailing commas in inline arrays | Yes | |

### Known Edge Cases

| Edge Case | Behavior | Risk for AGS? |
|-----------|----------|---------------|
| Inline tables `{ a = 1, b = 2 }` | Preserved, but cannot be mutated (read-only in toml_edit) | **Low** — AGS config doesn't use inline tables except `secret_store = { ... }`. For editing secrets, we can replace the entire value. |
| Dotted keys `sandbox.image = "x"` | Preserved on round-trip, but the API accesses them as nested tables | **None** — AGS config uses `[sandbox]` + `image = ...` style |
| Mixed `[table]` and `[[array]]` | Fully supported | N/A |
| Unicode in comments | Preserved | N/A |
| Trailing newline at EOF | Preserved if present | N/A |

### Inline Table Mutation Workaround

`toml_edit` treats inline tables as immutable (by TOML spec, inline tables are
single-line and their formatting is fragile). If the editor needs to modify a
`secret_store = { service = "...", username = "..." }` value:

```rust
// Replace the entire inline table value
let mut inline = toml_edit::InlineTable::new();
inline.insert("service", "new-service".into());
inline.insert("username", "new-user".into());
doc["secret"][idx]["secret_store"] = toml_edit::value(inline);
```

This replaces the whole value but is fine — the key and surrounding structure are preserved.

### Practical Assessment for AGS Config

AGS config is **simple, standard TOML**:
- `[table]` sections with scalar key-value pairs
- `[[array_of_tables]]` for mounts, tools, secrets
- One inline table usage (`secret_store = { ... }`)
- No dotted keys, no multiline strings, no exotic features

**Round-trip fidelity will be excellent.** The edge cases that trip up `toml_edit` don't
apply to this config format.

---

## 3. Layered Config Strategy

### Architecture

The editor manages two independent `toml_edit::DocumentMut` instances:

```
┌─────────────────────────────────────┐
│  Global Config (DocumentMut)        │  ~/.config/ags/config.toml
│  - Full config, always present      │
└──────────────┬──────────────────────┘
               │
┌──────────────▼──────────────────────┐
│  Local Overlay (Option<DocumentMut>)│  .ags/config.toml (may not exist)
│  - Partial config, overrides only   │
└──────────────┬──────────────────────┘
               │
┌──────────────▼──────────────────────┐
│  Merged View (read-only, computed)  │  What `ags` actually uses at runtime
│  - Scalars: overlay wins            │
│  - Arrays: additive (mount, etc.)   │
└─────────────────────────────────────┘
```

### Loading

```rust
struct ConfigEditorState {
    global_doc: DocumentMut,
    global_path: PathBuf,
    local_doc: Option<DocumentMut>,
    local_path: PathBuf,          // .ags/config.toml (may not exist yet)
    edit_target: EditTarget,      // Which file the user is editing
}

enum EditTarget {
    Global,
    Local,
}
```

Load each file independently with `toml_edit`:

```rust
let global_content = fs::read_to_string(&global_path)?;
let global_doc: DocumentMut = global_content.parse()?;

let local_doc = if local_path.exists() {
    let content = fs::read_to_string(&local_path)?;
    Some(content.parse::<DocumentMut>()?)
} else {
    None
};
```

### Computing the Merged View

Build a merged view for display **without mutating either source document**. The merged
view is a read-only snapshot used only for rendering the "effective config" pane.

```rust
fn compute_merged_view(global: &DocumentMut, local: Option<&DocumentMut>) -> DocumentMut {
    let mut merged: DocumentMut = global.to_string().parse().unwrap();

    if let Some(overlay) = local {
        // For each key in the overlay:
        for (key, item) in overlay.as_table().iter() {
            if is_additive_key(key) {
                // Append overlay's array-of-tables entries to merged
                if let (Some(base_aot), Some(overlay_aot)) = (
                    merged[key].as_array_of_tables_mut(),
                    item.as_array_of_tables(),
                ) {
                    for entry in overlay_aot.iter() {
                        base_aot.push(entry.clone());
                    }
                }
            } else {
                // Scalar/table override: overlay wins
                merged[key] = item.clone();
            }
        }
    }
    merged
}

fn is_additive_key(key: &str) -> bool {
    matches!(key, "mount" | "agent_mount" | "tool" | "secret")
}
```

This mirrors the exact merge logic in `parse.rs:merge_toml_value()` (lines 72-108).

### Write-Back Semantics

When the user saves, **only write the selected target document**:

- If `edit_target == Global`: serialize `global_doc` and write to `global_path`
- If `edit_target == Local`: serialize `local_doc` and write to `local_path`

The other file is never modified. The merged view is recomputed after save for display.

### Handling Additive Arrays in the UI

The merged view shows all entries from both layers. The UI should annotate each entry
with its source:

```
[[mount]]                          [global]
  host = "~/shared/skills"
  container = "/home/dev/.claude/skills"
  mode = "ro"

[[mount]]                          [local]
  host = "~/project/extra"
  container = "/home/dev/extra"
  mode = "rw"
```

When the user adds/removes/edits an array entry, the edit applies to the selected target
document only. The user should switch `edit_target` to control where changes land.

---

## 4. Schema-Driven Regen vs. Document Patching

### Option A: Document Patching (in-place edits on `DocumentMut`)

- Parse file into `DocumentMut`
- Apply user edits directly to the document tree
- Serialize back to string
- **Preserves**: comments, formatting, key order, whitespace
- **Complexity**: Low for AGS config (flat structure, no deeply nested paths)

### Option B: Schema-Driven Regen (deserialize → modify typed struct → re-serialize)

- Parse file with serde into `RawConfig`
- Modify the Rust struct
- Serialize back with `toml::to_string_pretty()`
- **Destroys**: all comments, custom formatting, key order, blank lines

### Recommendation: Document Patching (Option A)

This is not a close call. The entire point of a config editor is to work with the user's
actual config file, not a sanitized regeneration. Users put comments in their config for a
reason. Destroying them on every save would make the tool hostile.

Document patching with `toml_edit` is also simpler for our use case:
- No need to round-trip through serde types
- No need to handle serde defaults (which would inject values the user didn't set)
- Direct correspondence between what the user sees and what's in the file

**Schema-driven regen has exactly one use**: creating a brand-new config from scratch
(e.g., `ags init`). For the editor, always patch.

---

## 5. Save / Backup / Undo Strategy

### Backup Before Write

Before every save, copy the target file to a backup:

```rust
fn backup_config(path: &Path) -> io::Result<PathBuf> {
    let backup = path.with_extension("toml.bak");
    fs::copy(path, &backup)?;
    Ok(backup)
}
```

Use a single `.bak` file (not timestamped). Rationale: this is a TUI editor, not a
version control system. One level of backup is sufficient. Users who want history should
use git (and the session protocol already commits config changes).

### Atomic Write

Never write directly to the config file. Use write-to-temp + rename:

```rust
use std::io::Write;

fn atomic_write(path: &Path, content: &str) -> io::Result<()> {
    let dir = path.parent().unwrap();
    let mut tmp = tempfile::NamedTempFile::new_in(dir)?;
    tmp.write_all(content.as_bytes())?;
    tmp.persist(path)?;
    Ok(())
}
```

This ensures the config file is never half-written if the process crashes.

### Post-Save Validation

After writing, run the existing validation pipeline to catch semantic errors:

```rust
fn validate_after_save(path: &Path) -> Result<(), ConfigError> {
    parse_and_validate(path)?;
    Ok(())
}
```

If validation fails, the editor should:
1. Show the error to the user
2. Offer to revert to the backup
3. Keep the editor open so the user can fix the issue

### Undo (Revert to Backup)

```rust
fn revert_from_backup(path: &Path) -> io::Result<()> {
    let backup = path.with_extension("toml.bak");
    if backup.exists() {
        fs::copy(&backup, path)?;
    }
    Ok(())
}
```

Reload the `DocumentMut` from the restored file after reverting.

### Invalid Config Recovery

If the source file is already broken TOML when the editor opens:

1. Attempt `toml_edit` parse. If it fails:
2. Show the parse error with line/column info
3. Offer options:
   - **Edit as raw text** — open in `$EDITOR` or a basic text input
   - **Restore from backup** — if `.bak` exists
   - **Start fresh** — generate a new config from the example template

Do not silently discard broken configs. The user needs to know what happened.

### In-Memory Undo Stack

For undo within an editing session (before save), maintain a stack of `DocumentMut`
snapshots. Since AGS configs are small (typically <200 lines), cloning the document string
is cheap:

```rust
struct UndoStack {
    states: Vec<String>,  // serialized document strings
    cursor: usize,
}
```

Push a snapshot before each edit operation. Undo pops and re-parses. This is simple,
correct, and fast enough for config files.

---

## 6. TUI Framework

### Core Stack: `ratatui` + `crossterm`

**`ratatui = "0.30"`** is the standard Rust TUI framework. It provides:
- Immediate-mode rendering (you build the frame each tick)
- Rich widget library: `List`, `Table`, `Paragraph`, `Block`, `Tabs`, `Popup`
- Layout system: `Layout::horizontal()`, `Layout::vertical()`, constraints
- Styling: colors, bold, underline, etc.

**`crossterm`** is the terminal backend (already the default for ratatui). Handles raw
mode, key events, mouse events, alternate screen.

```toml
ratatui = "0.30"
crossterm = "0.28"
```

### Relevant Widget Crates

| Crate | Purpose | Recommendation |
|-------|---------|----------------|
| `tui-input` | Single-line text input widget | Use for editing scalar values |
| `tui-textarea` | Multi-line text editor | Use for raw text editing / broken config recovery |
| `tui-tree-widget` | Tree view widget | Consider for nested config sections, but a flat list with indentation may be simpler |
| `tui-scrollview` | Scrollable viewport | Useful if config is long |

### Recommended Approach

Don't over-invest in widget crates for v1. The AGS config has a flat-ish structure that
maps well to ratatui's built-in `List` and `Table` widgets:

- **Left pane**: Section list (`[sandbox]`, `[[mount]]`, `[[tool]]`, etc.) using `List`
- **Right pane**: Key-value editor for selected section using `Table` or custom layout
- **Bottom bar**: Status line, keybindings help, current edit target (global/local)
- **Modal**: `tui-input` for editing individual values; confirm dialogs for save/revert

The config is small enough that full-document rendering in a scrollable list is fine —
no need for virtual scrolling or lazy loading.

### Architecture Pattern

Use the standard ratatui app pattern:

```rust
struct App {
    config_state: ConfigEditorState,  // from section 3
    undo_stack: UndoStack,
    ui_state: UiState,                // selected section, cursor, mode
    running: bool,
}

fn main() -> Result<()> {
    let terminal = ratatui::init();
    let mut app = App::new()?;

    while app.running {
        terminal.draw(|frame| app.render(frame))?;
        if let Event::Key(key) = crossterm::event::read()? {
            app.handle_key(key);
        }
    }

    ratatui::restore();
    Ok(())
}
```

---

## 7. Summary of Recommendations

| Decision | Choice | Rationale |
|----------|--------|-----------|
| TOML editing library | `toml_edit = "0.25"` | Only viable option for comment-preserving round-trip |
| Coexistence with `toml` | Both in dep tree | Zero cost — `toml` already depends on `toml_edit` |
| Edit approach | Document patching | Preserves user's comments and formatting |
| Layered config | Two independent `DocumentMut` + computed merged view | Matches existing merge semantics exactly |
| Write target | Only the selected file (global or local) | Never corrupt the other layer |
| Backup strategy | Single `.bak` file, atomic write via tempfile | Simple, sufficient, no data loss |
| Post-save validation | Run existing `parse_and_validate()` | Reuse existing code, catch semantic errors |
| TUI framework | `ratatui = "0.30"` + `crossterm = "0.28"` | Industry standard, excellent docs |
| Text input widget | `tui-input` for scalar editing | Lightweight, well-maintained |
| In-memory undo | Stack of serialized document strings | Simple, correct, fast for small configs |

### New Dependencies for Config Editor Crate

```toml
[dependencies]
toml_edit = "0.25"
toml = "0.8"          # for validation via existing parse_and_validate
ratatui = "0.30"
crossterm = "0.28"
tui-input = "0.11"
tempfile = "3"
```
