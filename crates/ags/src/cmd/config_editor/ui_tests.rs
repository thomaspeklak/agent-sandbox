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

    assert!(matches!(
        super::compute_host_status(&agent),
        super::HostStatus::Missing
    ));
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
            &super::SuggestionCache {
                binaries: &[],
                home_dirs: &[]
            },
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
            &super::SuggestionCache {
                binaries: &[],
                home_dirs: &[]
            },
        ),
        Some("~/.gitconfig".to_string())
    );
}
