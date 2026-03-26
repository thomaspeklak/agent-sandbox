use std::fs;
use std::path::PathBuf;

use tempfile::TempDir;
use toml_edit::DocumentMut;

use ags::cmd::config_editor::model::*;

// ---------------------------------------------------------------------------
// Test helpers
// ---------------------------------------------------------------------------

fn setup_global(dir: &TempDir, content: &str) -> PathBuf {
    let path = dir.path().join("config.toml");
    fs::write(&path, content).unwrap();
    path
}

fn setup_local(dir: &TempDir, content: &str) -> PathBuf {
    let ags_dir = dir.path().join(".ags");
    fs::create_dir_all(&ags_dir).unwrap();
    let path = ags_dir.join("config.toml");
    fs::write(&path, content).unwrap();
    path
}

const GLOBAL_TOML: &str = r#"
[sandbox]
image = "localhost/agent-sandbox:latest"
containerfile = "/home/user/.config/ags/Containerfile"
cache_dir = "/home/user/.cache/ags"
gitconfig_path = "/home/user/.config/ags/gitconfig-agent"
auth_key = "/home/user/.ssh/ags-agent-auth"
sign_key = "/home/user/.ssh/ags-agent-signing"

[[mount]]
host = "/home/user/.ssh/known_hosts"
container = "/home/dev/.ssh/known_hosts"
mode = "ro"
kind = "file"
optional = true
"#;

const LOCAL_TOML: &str = r#"
[sandbox]
image = "custom-sandbox:dev"

[[mount]]
host = "/home/user/project/extra"
container = "/home/dev/extra"
mode = "rw"
kind = "dir"
"#;

// ---------------------------------------------------------------------------
// 1. Loading
// ---------------------------------------------------------------------------

#[test]
fn load_global_only() {
    let dir = TempDir::new().unwrap();
    let global = setup_global(&dir, GLOBAL_TOML);
    let local = dir.path().join(".ags/config.toml"); // does not exist

    let state = ConfigEditorState::load(&global, &local).unwrap();

    assert_eq!(state.global_path, global);
    assert_eq!(state.local_path, local);
    assert!(!state.local_exists);
    assert!(state.local_doc.iter().next().is_none());
    assert_eq!(state.edit_target, EditTarget::Global);
    assert!(!state.modified);
}

#[test]
fn load_global_and_local() {
    let dir = TempDir::new().unwrap();
    let global = setup_global(&dir, GLOBAL_TOML);
    let local = setup_local(&dir, LOCAL_TOML);

    let state = ConfigEditorState::load(&global, &local).unwrap();

    assert!(state.local_exists);
    let local_doc = &state.local_doc;
    assert_eq!(
        local_doc["sandbox"]["image"].as_str().unwrap(),
        "custom-sandbox:dev"
    );
}

#[test]
fn load_missing_local_is_none() {
    let dir = TempDir::new().unwrap();
    let global = setup_global(&dir, GLOBAL_TOML);
    let local = dir.path().join("nonexistent/config.toml");

    let state = ConfigEditorState::load(&global, &local).unwrap();
    assert!(!state.local_exists);
    assert!(state.local_doc.iter().next().is_none());
}

// ---------------------------------------------------------------------------
// 2. Merged view computation
// ---------------------------------------------------------------------------

#[test]
fn merged_scalar_override() {
    let dir = TempDir::new().unwrap();
    let global = setup_global(&dir, GLOBAL_TOML);
    let local = setup_local(&dir, LOCAL_TOML);

    let state = ConfigEditorState::load(&global, &local).unwrap();
    let merged = state.compute_merged_view();

    // Local image should override global
    assert_eq!(
        merged["sandbox"]["image"].as_str().unwrap(),
        "custom-sandbox:dev"
    );
    // Global-only field should pass through
    assert_eq!(
        merged["sandbox"]["cache_dir"].as_str().unwrap(),
        "/home/user/.cache/ags"
    );
}

#[test]
fn merged_table_merge_preserves_global_fields() {
    let dir = TempDir::new().unwrap();
    let global = setup_global(&dir, GLOBAL_TOML);
    let local = setup_local(&dir, LOCAL_TOML);

    let state = ConfigEditorState::load(&global, &local).unwrap();
    let merged = state.compute_merged_view();

    // All global sandbox fields should still be present
    assert!(merged["sandbox"]["containerfile"].as_str().is_some());
    assert!(merged["sandbox"]["auth_key"].as_str().is_some());
    assert!(merged["sandbox"]["sign_key"].as_str().is_some());
}

#[test]
fn merged_additive_arrays_concatenated() {
    let dir = TempDir::new().unwrap();
    let global = setup_global(&dir, GLOBAL_TOML);
    let local = setup_local(&dir, LOCAL_TOML);

    let state = ConfigEditorState::load(&global, &local).unwrap();
    let merged = state.compute_merged_view();

    let mounts = merged["mount"].as_array_of_tables().unwrap();
    assert_eq!(mounts.len(), 2);
    let entries: Vec<_> = mounts.iter().collect();
    // First entry from global
    assert_eq!(
        entries[0]["host"].as_str().unwrap(),
        "/home/user/.ssh/known_hosts"
    );
    // Second entry from local
    assert_eq!(
        entries[1]["host"].as_str().unwrap(),
        "/home/user/project/extra"
    );
}

#[test]
fn merged_global_only_sections_pass_through() {
    let global_with_browser = r#"
[sandbox]
image = "test"

[browser]
enabled = false
"#;
    let dir = TempDir::new().unwrap();
    let global = setup_global(&dir, global_with_browser);
    let local = setup_local(&dir, "[sandbox]\nimage = \"override\"\n");

    let state = ConfigEditorState::load(&global, &local).unwrap();
    let merged = state.compute_merged_view();

    // browser section from global should be in merged
    assert!(merged.get("browser").is_some());
}

#[test]
fn merged_local_only_sections_added() {
    let dir = TempDir::new().unwrap();
    let global = setup_global(&dir, "[sandbox]\nimage = \"base\"\n");
    let local_with_extra = r#"
[browser]
enabled = true
"#;
    let local = setup_local(&dir, local_with_extra);

    let state = ConfigEditorState::load(&global, &local).unwrap();
    let merged = state.compute_merged_view();

    // browser section only in local should appear in merged
    assert!(merged.get("browser").is_some());
}

#[test]
fn merged_no_local_returns_global_clone() {
    let dir = TempDir::new().unwrap();
    let global = setup_global(&dir, GLOBAL_TOML);
    let local = dir.path().join(".ags/config.toml"); // nonexistent

    let state = ConfigEditorState::load(&global, &local).unwrap();
    let merged = state.compute_merged_view();

    assert_eq!(
        merged["sandbox"]["image"].as_str().unwrap(),
        "localhost/agent-sandbox:latest"
    );
}

// ---------------------------------------------------------------------------
// 3. Value source detection
// ---------------------------------------------------------------------------

#[test]
fn value_source_global_only() {
    let dir = TempDir::new().unwrap();
    let global = setup_global(&dir, GLOBAL_TOML);
    let local = setup_local(&dir, LOCAL_TOML);

    let state = ConfigEditorState::load(&global, &local).unwrap();

    // cache_dir only in global
    assert_eq!(
        state.value_source("sandbox", "cache_dir"),
        ValueSource::Global
    );
}

#[test]
fn value_source_local_only() {
    let dir = TempDir::new().unwrap();
    let global = setup_global(&dir, "[sandbox]\nimage = \"base\"\n");
    let local_with_extra = "[sandbox]\ncache_dir = \"/tmp/cache\"\n";
    let local = setup_local(&dir, local_with_extra);

    let state = ConfigEditorState::load(&global, &local).unwrap();

    assert_eq!(
        state.value_source("sandbox", "cache_dir"),
        ValueSource::Local
    );
}

#[test]
fn value_source_local_overrides_global() {
    let dir = TempDir::new().unwrap();
    let global = setup_global(&dir, GLOBAL_TOML);
    let local = setup_local(&dir, LOCAL_TOML);

    let state = ConfigEditorState::load(&global, &local).unwrap();

    // image exists in both
    assert_eq!(
        state.value_source("sandbox", "image"),
        ValueSource::LocalOverridesGlobal
    );
}

#[test]
fn array_entry_source_by_index() {
    let dir = TempDir::new().unwrap();
    let global = setup_global(&dir, GLOBAL_TOML);
    let local = setup_local(&dir, LOCAL_TOML);

    let state = ConfigEditorState::load(&global, &local).unwrap();

    // Global has 1 mount, so index 0 = Global, index 1 = Local
    assert_eq!(state.array_entry_source("mount", 0), ValueSource::Global);
    assert_eq!(state.array_entry_source("mount", 1), ValueSource::Local);
}

// ---------------------------------------------------------------------------
// 4. Save pipeline
// ---------------------------------------------------------------------------

#[test]
fn save_creates_backup_file() {
    let dir = TempDir::new().unwrap();
    let global = setup_global(&dir, GLOBAL_TOML);
    let local = dir.path().join(".ags/config.toml");

    let mut state = ConfigEditorState::load(&global, &local).unwrap();
    state.save().unwrap();

    let backup = global.with_extension("toml.bak");
    assert!(backup.exists());
    assert_eq!(fs::read_to_string(&backup).unwrap(), GLOBAL_TOML);
}

#[test]
fn save_writes_correct_content() {
    let dir = TempDir::new().unwrap();
    let global = setup_global(&dir, GLOBAL_TOML);
    let local = dir.path().join(".ags/config.toml");

    let mut state = ConfigEditorState::load(&global, &local).unwrap();
    state.save().unwrap();

    let saved = fs::read_to_string(&global).unwrap();
    let reparsed: DocumentMut = saved.parse().unwrap();
    assert_eq!(
        reparsed["sandbox"]["image"].as_str().unwrap(),
        "localhost/agent-sandbox:latest"
    );
}

#[test]
fn save_roundtrip_stability() {
    let dir = TempDir::new().unwrap();
    let global = setup_global(&dir, GLOBAL_TOML);
    let local = dir.path().join(".ags/config.toml");

    let mut state = ConfigEditorState::load(&global, &local).unwrap();
    let before = state.global_doc.to_string();
    state.save().unwrap();

    // Reload and compare
    let reloaded = ConfigEditorState::load(&global, &local).unwrap();
    let after = reloaded.global_doc.to_string();
    assert_eq!(before, after);
}

#[test]
fn save_clears_modified_flag() {
    let dir = TempDir::new().unwrap();
    let global = setup_global(&dir, GLOBAL_TOML);
    let local = dir.path().join(".ags/config.toml");

    let mut state = ConfigEditorState::load(&global, &local).unwrap();
    state.modified = true;
    state.save().unwrap();
    assert!(!state.modified);
}

// ---------------------------------------------------------------------------
// 5. Undo
// ---------------------------------------------------------------------------

#[test]
fn undo_restores_from_backup() {
    let dir = TempDir::new().unwrap();
    let global = setup_global(&dir, GLOBAL_TOML);
    let local = dir.path().join(".ags/config.toml");

    let mut state = ConfigEditorState::load(&global, &local).unwrap();

    // Save to create backup
    state.save().unwrap();

    // Mutate the doc
    state.global_doc["sandbox"]["image"] = toml_edit::value("mutated:latest");
    state.modified = true;
    state.save().unwrap();

    // Now undo should restore the first backup (which was overwritten by second save)
    // Actually, let's set up a more predictable scenario:
    // Write a known backup manually
    let backup_path = global.with_extension("toml.bak");
    fs::write(&backup_path, GLOBAL_TOML).unwrap();

    let result = state.undo().unwrap();
    assert!(result);
    assert_eq!(
        state.global_doc["sandbox"]["image"].as_str().unwrap(),
        "localhost/agent-sandbox:latest"
    );
}

#[test]
fn undo_returns_false_when_no_backup() {
    let dir = TempDir::new().unwrap();
    let global = setup_global(&dir, GLOBAL_TOML);
    let local = dir.path().join(".ags/config.toml");

    let mut state = ConfigEditorState::load(&global, &local).unwrap();

    // No backup exists
    let result = state.undo().unwrap();
    assert!(!result);
}

#[test]
fn undo_clears_modified_flag() {
    let dir = TempDir::new().unwrap();
    let global = setup_global(&dir, GLOBAL_TOML);
    let local = dir.path().join(".ags/config.toml");

    let mut state = ConfigEditorState::load(&global, &local).unwrap();
    state.save().unwrap(); // create backup

    state.modified = true;
    let result = state.undo().unwrap();
    assert!(result);
    assert!(!state.modified);
}

// ---------------------------------------------------------------------------
// 6. Target management
// ---------------------------------------------------------------------------

#[test]
fn toggle_target_switches_between_global_and_local() {
    let dir = TempDir::new().unwrap();
    let global = setup_global(&dir, GLOBAL_TOML);
    let local = dir.path().join(".ags/config.toml");

    let mut state = ConfigEditorState::load(&global, &local).unwrap();
    assert_eq!(state.edit_target, EditTarget::Global);

    state.toggle_target();
    assert_eq!(state.edit_target, EditTarget::Local);

    state.toggle_target();
    assert_eq!(state.edit_target, EditTarget::Global);
}

#[test]
fn active_doc_returns_correct_doc_per_target() {
    let dir = TempDir::new().unwrap();
    let global = setup_global(&dir, GLOBAL_TOML);
    let local = setup_local(&dir, LOCAL_TOML);

    let mut state = ConfigEditorState::load(&global, &local).unwrap();

    // Global target -> global doc
    assert_eq!(
        state.active_doc()["sandbox"]["image"].as_str().unwrap(),
        "localhost/agent-sandbox:latest"
    );

    state.toggle_target();

    // Local target -> local doc
    assert_eq!(
        state.active_doc()["sandbox"]["image"].as_str().unwrap(),
        "custom-sandbox:dev"
    );
}

#[test]
fn active_path_returns_correct_path_per_target() {
    let dir = TempDir::new().unwrap();
    let global = setup_global(&dir, GLOBAL_TOML);
    let local = setup_local(&dir, LOCAL_TOML);

    let mut state = ConfigEditorState::load(&global, &local).unwrap();

    assert_eq!(state.active_path(), &global);

    state.toggle_target();
    assert_eq!(state.active_path(), &local);
}

#[test]
fn create_local_if_missing_creates_file_and_sets_doc() {
    let dir = TempDir::new().unwrap();
    let global = setup_global(&dir, GLOBAL_TOML);
    let local = dir.path().join(".ags/config.toml");

    let mut state = ConfigEditorState::load(&global, &local).unwrap();
    assert!(!state.local_exists);

    state.create_local_if_missing().unwrap();

    assert!(state.local_exists);
    assert!(local.exists());
}

#[test]
fn create_local_if_missing_noop_when_exists() {
    let dir = TempDir::new().unwrap();
    let global = setup_global(&dir, GLOBAL_TOML);
    let local = setup_local(&dir, LOCAL_TOML);

    let mut state = ConfigEditorState::load(&global, &local).unwrap();

    // Should succeed without error and leave doc unchanged
    state.create_local_if_missing().unwrap();
    assert_eq!(
        state.local_doc["sandbox"]["image"].as_str().unwrap(),
        "custom-sandbox:dev"
    );
}

#[test]
fn missing_local_target_uses_local_draft_not_global_doc() {
    let dir = TempDir::new().unwrap();
    let global = setup_global(&dir, GLOBAL_TOML);
    let local = dir.path().join(".ags/config.toml");

    let mut state = ConfigEditorState::load(&global, &local).unwrap();
    state.toggle_target();

    state.active_doc_mut()["sandbox"]["image"] = toml_edit::value("local-draft:dev");

    assert_eq!(
        state.global_doc["sandbox"]["image"].as_str().unwrap(),
        "localhost/agent-sandbox:latest"
    );
    assert_eq!(
        state.local_doc["sandbox"]["image"].as_str().unwrap(),
        "local-draft:dev"
    );
    assert!(!state.local_exists);
}

#[test]
fn saving_missing_local_target_creates_local_overlay_without_mutating_global_file() {
    let dir = TempDir::new().unwrap();
    let global = setup_global(&dir, GLOBAL_TOML);
    let local = dir.path().join(".ags/config.toml");

    let mut state = ConfigEditorState::load(&global, &local).unwrap();
    state.toggle_target();
    state.active_doc_mut()["sandbox"]["image"] = toml_edit::value("repo-only:dev");
    state.modified = true;
    state.save().unwrap();

    assert!(state.local_exists);
    assert!(local.exists());
    assert_eq!(
        state.global_doc["sandbox"]["image"].as_str().unwrap(),
        "localhost/agent-sandbox:latest"
    );
    let saved_local = fs::read_to_string(&local).unwrap();
    assert!(saved_local.contains("repo-only:dev"));
}

#[test]
fn validate_active_uses_overlay_semantics_for_local_target() {
    let dir = TempDir::new().unwrap();
    let global = setup_global(&dir, GLOBAL_TOML);
    let local = dir.path().join(".ags/config.toml");

    let mut state = ConfigEditorState::load(&global, &local).unwrap();
    state.toggle_target();
    state.active_doc_mut()["sandbox"]["image"] = toml_edit::value("repo-only:dev");
    state.modified = true;
    state.save().unwrap();

    assert!(state.validate_active().is_ok());
    assert!(ags::config::parse_and_validate(&local).is_err());
}

#[test]
fn validate_active_reports_invalid_merged_local_overlay() {
    let dir = TempDir::new().unwrap();
    let global = setup_global(&dir, GLOBAL_TOML);
    let local = dir.path().join(".ags/config.toml");

    let mut state = ConfigEditorState::load(&global, &local).unwrap();
    state.toggle_target();
    state.active_doc_mut()["sandbox"]["image"] = toml_edit::value("");
    state.modified = true;
    state.save().unwrap();

    let err = state.validate_active().unwrap_err().to_string();
    assert!(err.contains("[sandbox].image"), "got: {err}");
}

// ---------------------------------------------------------------------------
// 7. Edit operations (via DocumentMut)
// ---------------------------------------------------------------------------

#[test]
fn edit_modify_scalar_value() {
    let dir = TempDir::new().unwrap();
    let global = setup_global(&dir, GLOBAL_TOML);
    let local = dir.path().join(".ags/config.toml");

    let mut state = ConfigEditorState::load(&global, &local).unwrap();

    state.active_doc_mut()["sandbox"]["image"] = toml_edit::value("new-image:v2");
    assert_eq!(
        state.global_doc["sandbox"]["image"].as_str().unwrap(),
        "new-image:v2"
    );
}

#[test]
fn edit_add_array_of_tables_entry() {
    let dir = TempDir::new().unwrap();
    let global = setup_global(&dir, GLOBAL_TOML);
    let local = dir.path().join(".ags/config.toml");

    let mut state = ConfigEditorState::load(&global, &local).unwrap();

    let mounts_before = state.global_doc["mount"]
        .as_array_of_tables()
        .unwrap()
        .len();

    // Add a new mount entry
    let mut new_table = toml_edit::Table::new();
    new_table["host"] = toml_edit::value("/home/user/new");
    new_table["container"] = toml_edit::value("/home/dev/new");
    new_table["mode"] = toml_edit::value("rw");
    new_table["kind"] = toml_edit::value("dir");

    state.global_doc["mount"]
        .as_array_of_tables_mut()
        .unwrap()
        .push(new_table);

    let mounts_after = state.global_doc["mount"]
        .as_array_of_tables()
        .unwrap()
        .len();
    assert_eq!(mounts_after, mounts_before + 1);
}

#[test]
fn edit_remove_array_of_tables_entry() {
    let dir = TempDir::new().unwrap();
    let two_mounts = r#"
[sandbox]
image = "test"

[[mount]]
host = "/first"
container = "/dev/first"
mode = "ro"
kind = "file"

[[mount]]
host = "/second"
container = "/dev/second"
mode = "rw"
kind = "dir"
"#;
    let global = setup_global(&dir, two_mounts);
    let local = dir.path().join(".ags/config.toml");

    let mut state = ConfigEditorState::load(&global, &local).unwrap();

    // Remove the first entry
    state.global_doc["mount"]
        .as_array_of_tables_mut()
        .unwrap()
        .remove(0);

    let mounts = state.global_doc["mount"].as_array_of_tables().unwrap();
    assert_eq!(mounts.len(), 1);
    assert_eq!(
        mounts.iter().next().unwrap()["host"].as_str().unwrap(),
        "/second"
    );
}

#[test]
fn modified_flag_not_auto_set_by_model() {
    let dir = TempDir::new().unwrap();
    let global = setup_global(&dir, GLOBAL_TOML);
    let local = dir.path().join(".ags/config.toml");

    let mut state = ConfigEditorState::load(&global, &local).unwrap();
    assert!(!state.modified);

    // Edit the doc directly — model does NOT auto-set modified
    state.global_doc["sandbox"]["image"] = toml_edit::value("changed");
    assert!(!state.modified);
}

// ---------------------------------------------------------------------------
// 8. Dogfood scenario tests
// ---------------------------------------------------------------------------

/// Scenario 1: Loading a missing global config returns an error (not a panic).
#[test]
fn load_missing_global_returns_error() {
    let dir = TempDir::new().unwrap();
    let global = dir.path().join("nonexistent/config.toml");
    let local = dir.path().join(".ags/config.toml");

    let result = ConfigEditorState::load(&global, &local);
    assert!(result.is_err());
}

/// Scenario 2: Load a realistic config with all section types.
#[test]
fn load_full_multi_section_config() {
    let full_config = r#"
[sandbox]
image = "localhost/agent-sandbox:latest"
containerfile = "~/.config/ags/Containerfile"
cache_dir = "~/.cache/ags"

[[mount]]
host = "~/.ssh/known_hosts"
container = "/home/dev/.ssh/known_hosts"
mode = "ro"
kind = "file"
optional = true

[[agent_mount]]
host = "~/.claude.json"
container = "/home/dev/.claude.json"
kind = "file"

[[tool]]
name = "gh"
path = "/usr/bin/gh"
container_path = "/usr/local/bin/gh"

[[secret]]
env = "ANTHROPIC_API_KEY"
from_env = "ANTHROPIC_API_KEY"

[browser]
enabled = false

[auth_proxy]
enabled = true
port = 8080

[host_ui]
enabled = false

[psp]
enabled = false

[update]
auto_check = true
"#;

    let dir = TempDir::new().unwrap();
    let global = setup_global(&dir, full_config);
    let local = dir.path().join(".ags/config.toml");

    let state = ConfigEditorState::load(&global, &local).unwrap();

    // Verify all sections are accessible
    assert!(state.global_doc.get("sandbox").is_some());
    assert!(state.global_doc.get("mount").is_some());
    assert!(state.global_doc.get("agent_mount").is_some());
    assert!(state.global_doc.get("tool").is_some());
    assert!(state.global_doc.get("secret").is_some());
    assert!(state.global_doc.get("browser").is_some());
    assert!(state.global_doc.get("auth_proxy").is_some());
    assert!(state.global_doc.get("host_ui").is_some());
    assert!(state.global_doc.get("psp").is_some());
    assert!(state.global_doc.get("update").is_some());

    // Verify array-of-tables counts
    assert_eq!(
        state.global_doc["mount"]
            .as_array_of_tables()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        state.global_doc["agent_mount"]
            .as_array_of_tables()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        state.global_doc["tool"].as_array_of_tables().unwrap().len(),
        1
    );
    assert_eq!(
        state.global_doc["secret"]
            .as_array_of_tables()
            .unwrap()
            .len(),
        1
    );
}

/// Scenario 3: Merged view with multiple additive arrays.
#[test]
fn merged_multiple_additive_arrays() {
    let global_toml = r#"
[sandbox]
image = "base-image"

[[mount]]
host = "/g/mount1"
container = "/c/mount1"
mode = "ro"

[[tool]]
name = "gh"
path = "/usr/bin/gh"

[[secret]]
env = "KEY_A"
from_env = "KEY_A"
"#;
    let local_toml = r#"
[[mount]]
host = "/l/mount2"
container = "/c/mount2"
mode = "rw"

[[tool]]
name = "jq"
path = "/usr/bin/jq"

[[secret]]
env = "KEY_B"
from_env = "KEY_B"
"#;

    let dir = TempDir::new().unwrap();
    let global = setup_global(&dir, global_toml);
    let local = setup_local(&dir, local_toml);

    let state = ConfigEditorState::load(&global, &local).unwrap();
    let merged = state.compute_merged_view();

    // Each additive array should have 2 entries (1 global + 1 local)
    assert_eq!(merged["mount"].as_array_of_tables().unwrap().len(), 2);
    assert_eq!(merged["tool"].as_array_of_tables().unwrap().len(), 2);
    assert_eq!(merged["secret"].as_array_of_tables().unwrap().len(), 2);

    // Verify ordering: global first, then local
    let tools: Vec<_> = merged["tool"]
        .as_array_of_tables()
        .unwrap()
        .iter()
        .collect();
    assert_eq!(tools[0]["name"].as_str().unwrap(), "gh");
    assert_eq!(tools[1]["name"].as_str().unwrap(), "jq");
}

/// Scenario 4: Add agent mount entries (simulating toggle_agent enable).
#[test]
fn add_agent_mount_entries() {
    let dir = TempDir::new().unwrap();
    let global = setup_global(&dir, "[sandbox]\nimage = \"test\"\n");
    let local = dir.path().join(".ags/config.toml");

    let mut state = ConfigEditorState::load(&global, &local).unwrap();
    let doc = state.active_doc_mut();

    // Simulate what toggle_agent does when enabling an agent:
    // Create agent_mount array if absent
    doc["agent_mount"] = toml_edit::Item::ArrayOfTables(toml_edit::ArrayOfTables::new());

    let aot = doc["agent_mount"].as_array_of_tables_mut().unwrap();

    // Add Claude agent mounts
    let mut entry1 = toml_edit::Table::new();
    entry1["host"] = toml_edit::value("~/.claude.json");
    entry1["container"] = toml_edit::value("/home/dev/.claude.json");
    entry1["kind"] = toml_edit::value("file");
    aot.push(entry1);

    let mut entry2 = toml_edit::Table::new();
    entry2["host"] = toml_edit::value("~/.claude");
    entry2["container"] = toml_edit::value("/home/dev/.claude");
    entry2["kind"] = toml_edit::value("dir");
    aot.push(entry2);

    // Verify TOML structure
    let aot = doc["agent_mount"].as_array_of_tables().unwrap();
    assert_eq!(aot.len(), 2);
    let entries: Vec<_> = aot.iter().collect();
    assert_eq!(entries[0]["host"].as_str().unwrap(), "~/.claude.json");
    assert_eq!(entries[0]["kind"].as_str().unwrap(), "file");
    assert_eq!(entries[1]["host"].as_str().unwrap(), "~/.claude");
    assert_eq!(entries[1]["kind"].as_str().unwrap(), "dir");

    // Verify serialized TOML is valid
    let serialized = doc.to_string();
    let reparsed: DocumentMut = serialized.parse().unwrap();
    assert_eq!(
        reparsed["agent_mount"].as_array_of_tables().unwrap().len(),
        2
    );
}

/// Scenario 4: Add tool entry with correct TOML structure.
#[test]
fn add_tool_entry_structure() {
    let dir = TempDir::new().unwrap();
    let global = setup_global(&dir, "[sandbox]\nimage = \"test\"\n");
    let local = dir.path().join(".ags/config.toml");

    let mut state = ConfigEditorState::load(&global, &local).unwrap();
    let doc = state.active_doc_mut();

    // Simulate confirm_add_entry for a tool
    doc["tool"] = toml_edit::Item::ArrayOfTables(toml_edit::ArrayOfTables::new());
    let aot = doc["tool"].as_array_of_tables_mut().unwrap();

    let mut entry = toml_edit::Table::new();
    entry["name"] = toml_edit::value("gh");
    entry["path"] = toml_edit::value("/usr/bin/gh");
    entry["container_path"] = toml_edit::value("/usr/local/bin/gh");
    aot.push(entry);

    // Roundtrip through serialization
    let serialized = doc.to_string();
    let reparsed: DocumentMut = serialized.parse().unwrap();
    let tools = reparsed["tool"].as_array_of_tables().unwrap();
    assert_eq!(tools.len(), 1);
    let t = tools.iter().next().unwrap();
    assert_eq!(t["name"].as_str().unwrap(), "gh");
    assert_eq!(t["path"].as_str().unwrap(), "/usr/bin/gh");
    assert_eq!(t["container_path"].as_str().unwrap(), "/usr/local/bin/gh");
}

/// Scenario 4: Add secret entry with from_env defaulting.
#[test]
fn add_secret_entry_structure() {
    let dir = TempDir::new().unwrap();
    let global = setup_global(&dir, "[sandbox]\nimage = \"test\"\n");
    let local = dir.path().join(".ags/config.toml");

    let mut state = ConfigEditorState::load(&global, &local).unwrap();
    let doc = state.active_doc_mut();

    doc["secret"] = toml_edit::Item::ArrayOfTables(toml_edit::ArrayOfTables::new());
    let aot = doc["secret"].as_array_of_tables_mut().unwrap();

    let mut entry = toml_edit::Table::new();
    entry["env"] = toml_edit::value("ANTHROPIC_API_KEY");
    // Simulate the default: from_env = env value when from_env is empty
    entry["from_env"] = toml_edit::value("ANTHROPIC_API_KEY");
    aot.push(entry);

    let serialized = doc.to_string();
    let reparsed: DocumentMut = serialized.parse().unwrap();
    let secrets = reparsed["secret"].as_array_of_tables().unwrap();
    let s = secrets.iter().next().unwrap();
    assert_eq!(s["env"].as_str().unwrap(), "ANTHROPIC_API_KEY");
    assert_eq!(s["from_env"].as_str().unwrap(), "ANTHROPIC_API_KEY");
}

/// Scenario 5: Save→undo restores file on disk.
#[test]
fn save_undo_restores_file_on_disk() {
    let dir = TempDir::new().unwrap();
    let global = setup_global(&dir, GLOBAL_TOML);
    let local = dir.path().join(".ags/config.toml");

    let mut state = ConfigEditorState::load(&global, &local).unwrap();

    // Save original
    state.save().unwrap();

    // Mutate and save again
    state.global_doc["sandbox"]["image"] = toml_edit::value("mutated:v2");
    state.modified = true;
    state.save().unwrap();

    // Verify file on disk has mutated value
    let on_disk = fs::read_to_string(&global).unwrap();
    assert!(on_disk.contains("mutated:v2"));

    // Undo
    let result = state.undo().unwrap();
    assert!(result);

    // Verify file on disk is restored (undo writes back to disk)
    let restored = fs::read_to_string(&global).unwrap();
    assert!(
        !restored.contains("mutated:v2"),
        "File on disk should be restored after undo"
    );
}

/// Scenario 6: Loading broken TOML returns error, not panic.
#[test]
fn load_broken_global_toml_returns_error() {
    let dir = TempDir::new().unwrap();
    let broken = "[sandbox\nimage = broken";
    let global = setup_global(&dir, broken);
    let local = dir.path().join(".ags/config.toml");

    let result = ConfigEditorState::load(&global, &local);
    assert!(result.is_err(), "Broken TOML should return Err, not panic");
}

/// Scenario 6: Loading valid global but broken local TOML returns error.
#[test]
fn load_broken_local_toml_returns_error() {
    let dir = TempDir::new().unwrap();
    let global = setup_global(&dir, GLOBAL_TOML);
    let broken_local = "[[mount\nhost = oops";
    let local = setup_local(&dir, broken_local);

    let result = ConfigEditorState::load(&global, &local);
    assert!(
        result.is_err(),
        "Broken local TOML should return Err, not panic"
    );
}

/// Scenario 6: Empty config file loads successfully (valid TOML).
#[test]
fn load_empty_global_config() {
    let dir = TempDir::new().unwrap();
    let global = setup_global(&dir, "");
    let local = dir.path().join(".ags/config.toml");

    let state = ConfigEditorState::load(&global, &local).unwrap();
    // Empty TOML is valid, no sections present
    assert!(state.global_doc.get("sandbox").is_none());
}

/// Scenario 1: DEFAULT_CONFIG from main.rs produces valid parseable TOML.
#[test]
fn default_config_is_valid_toml() {
    let default_config = r#"[sandbox]
image = "localhost/agent-sandbox:latest"
containerfile = "~/.config/ags/Containerfile"
cache_dir = "~/.cache/ags"
gitconfig_path = "~/.config/ags/gitconfig-agent"
auth_key = "~/.ssh/ags-agent-auth"
sign_key = "~/.ssh/ags-agent-signing"
bootstrap_files = ["auth.json", "models.json"]
container_boot_dirs = [
  "/home/dev/.ssh",
]
passthrough_env = [
  "ANTHROPIC_API_KEY",
  "OPENAI_API_KEY",
  "GEMINI_API_KEY",
  "OPENROUTER_API_KEY",
  "AI_GATEWAY_API_KEY",
  "OPENCODE_API_KEY",
]

[[mount]]
host = "~/.ssh/known_hosts"
container = "/home/dev/.ssh/known_hosts"
mode = "ro"
kind = "file"
optional = true
"#;

    // Verify it parses as valid TOML
    let doc: DocumentMut = default_config.parse().unwrap();
    assert!(doc.get("sandbox").is_some());
    assert!(doc.get("mount").is_some());

    // Verify it can be loaded by ConfigEditorState
    let dir = TempDir::new().unwrap();
    let global = setup_global(&dir, default_config);
    let local = dir.path().join(".ags/config.toml");
    let state = ConfigEditorState::load(&global, &local).unwrap();
    assert_eq!(
        state.global_doc["sandbox"]["image"].as_str().unwrap(),
        "localhost/agent-sandbox:latest"
    );
}

/// Scenario 3: value_source with no local doc defaults to Global.
#[test]
fn value_source_no_local_defaults_global() {
    let dir = TempDir::new().unwrap();
    let global = setup_global(&dir, GLOBAL_TOML);
    let local = dir.path().join(".ags/config.toml"); // nonexistent

    let state = ConfigEditorState::load(&global, &local).unwrap();

    // No local doc, so everything is Global
    assert_eq!(state.value_source("sandbox", "image"), ValueSource::Global);
}

/// Scenario 5: Undo on local target restores local doc.
#[test]
fn undo_local_target_restores_local_doc() {
    let dir = TempDir::new().unwrap();
    let global = setup_global(&dir, GLOBAL_TOML);
    let local = setup_local(&dir, LOCAL_TOML);

    let mut state = ConfigEditorState::load(&global, &local).unwrap();
    state.toggle_target(); // Switch to local

    // Save local to create backup
    state.save().unwrap();

    // Mutate local
    state.local_doc["sandbox"]["image"] = toml_edit::value("mutated:local");
    state.modified = true;
    state.save().unwrap();

    // Undo
    let result = state.undo().unwrap();
    assert!(result);
    assert_eq!(
        state.local_doc["sandbox"]["image"].as_str().unwrap(),
        "custom-sandbox:dev"
    );
}

#[test]
fn sections_is_array_matches_additive_array_keys() {
    use ags::config::ADDITIVE_ARRAY_KEYS;

    for section in SECTIONS {
        let expected = ADDITIVE_ARRAY_KEYS.contains(&section.toml_key);
        assert_eq!(
            section.is_array, expected,
            "SECTIONS entry {:?} has is_array={} but ADDITIVE_ARRAY_KEYS says {}",
            section.toml_key, section.is_array, expected,
        );
    }
}
