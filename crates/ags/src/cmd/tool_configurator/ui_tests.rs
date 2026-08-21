mod tests {
    use super::*;
    use crate::cmd::tool_configurator::model::{
        ToolCatalog, ToolDefinition, ToolGroupDefinition, ToolSubcategoryDefinition,
    };
    use crate::config::{ToolArchiveFormat, ToolDownloadArtifact, ToolDownloadSource};
    use std::collections::BTreeMap;
    use ratatui::backend::TestBackend;

    fn definition(id: &str, default: bool) -> ToolDefinition {
        ToolDefinition {
            id: id.to_owned(),
            name: id.to_owned(),
            description: format!("Use {id} to complete work."),
            default,
            dnf_packages: vec![id.to_owned()],
            download: None,
        }
    }

    fn group(id: &str, name: &str, subcategories: &[(&str, &[&str])]) -> ToolGroupDefinition {
        ToolGroupDefinition {
            id: id.to_owned(),
            name: name.to_owned(),
            subcategories: subcategories
                .iter()
                .map(|(name, tools)| ToolSubcategoryDefinition {
                    name: (*name).to_owned(),
                    tools: tools.iter().map(|tool| (*tool).to_owned()).collect(),
                })
                .collect(),
        }
    }

    fn app() -> App {
        let catalog = ToolCatalog {
            tools: vec![definition("shared", true), definition("optional", false)],
            groups: vec![
                group(
                    "general",
                    "General",
                    &[("First", &["shared"]), ("Second", &["optional"])],
                ),
                group(
                    "software-development",
                    "Software Development",
                    &[("Development", &["shared", "optional"])],
                ),
                group(
                    "operations-devops",
                    "Operations and DevOps",
                    &[("Operations", &["shared"])],
                ),
            ],
        };
        let state = ToolSelectionState::from_catalog(catalog, &[]).unwrap();
        let mut app = App {
            config_path: PathBuf::from("config.toml"),
            legacy_cleanup_path: None,
            packages_path: PathBuf::from("catalog.json"),
            state,
            running: true,
            current_group: 0,
            selected_row: 0,
            table_state: TableState::default(),
            show_help: false,
            status_message: None,
            save_report: None,
        };
        app.normalize_selection();
        app
    }

    #[test]
    fn divider_rows_are_skipped_during_navigation() {
        let mut app = app();
        assert_eq!(app.selected_row, 1);

        app.move_tool(1);
        assert_eq!(app.selected_row, 3);

        app.move_tool(-1);
        assert_eq!(app.selected_row, 1);
    }

    #[test]
    fn shared_tool_selection_is_visible_in_every_profession() {
        let mut app = app();
        app.toggle_current_tool();
        assert!(app.state.tools[0].selected);

        app.move_group(1);
        assert_eq!(app.current_tool_index(), Some(0));
        assert!(app.state.tools[0].selected);
    }

    #[test]
    fn restore_defaults_uses_catalog_flags() {
        let mut app = app();
        app.state.tools[0].selected = false;
        app.state.tools[1].selected = true;

        app.restore_defaults();

        assert!(app.state.tools[0].selected);
        assert!(!app.state.tools[1].selected);
        assert!(app.state.tools.iter().all(|tool| tool.touched));
    }

    #[test]
    fn stateful_table_scrolls_to_keep_selected_tool_visible() {
        let tools = (0..20)
            .map(|index| definition(&format!("tool-{index}"), false))
            .collect::<Vec<_>>();
        let ids_owned = tools
            .iter()
            .map(|tool| tool.id.clone())
            .collect::<Vec<_>>();
        let ids = ids_owned
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>();
        let catalog = ToolCatalog {
            tools,
            groups: vec![
                group("general", "General", &[("Area", &ids)]),
                group(
                    "software-development",
                    "Software Development",
                    &[("Area", &ids)],
                ),
                group(
                    "operations-devops",
                    "Operations and DevOps",
                    &[("Area", &ids)],
                ),
            ],
        };
        let state = ToolSelectionState::from_catalog(catalog, &[]).unwrap();
        let mut app = App {
            config_path: PathBuf::from("config.toml"),
            legacy_cleanup_path: None,
            packages_path: PathBuf::from("catalog.json"),
            state,
            running: true,
            current_group: 0,
            selected_row: 20,
            table_state: TableState::default(),
            show_help: false,
            status_message: None,
            save_report: None,
        };
        app.normalize_selection();
        let backend = TestBackend::new(100, 20);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal.draw(|frame| app.render(frame)).unwrap();

        assert!(app.table_state.offset() > 0);
    }

    #[test]
    fn rendered_picker_shows_professions_areas_and_defaults_but_not_rpm_names() {
        let mut app = app();
        app.state.tools[0].definition.dnf_packages = vec!["internal-rpm-name".to_owned()];
        app.state.tools[0].definition.download = Some(ToolDownloadSource {
            version: "1.0.0".to_owned(),
            archive: ToolArchiveFormat::Zip,
            member: "secret-member".to_owned(),
            install_as: "secret-command".to_owned(),
            artifacts: BTreeMap::from([
                (
                    "aarch64".to_owned(),
                    ToolDownloadArtifact {
                        url: "https://downloads.example.com/secret-arm64.zip".to_owned(),
                        sha256: "a".repeat(64),
                    },
                ),
                (
                    "x86_64".to_owned(),
                    ToolDownloadArtifact {
                        url: "https://downloads.example.com/secret-amd64.zip".to_owned(),
                        sha256: "b".repeat(64),
                    },
                ),
            ]),
        });
        let backend = TestBackend::new(140, 30);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal.draw(|frame| app.render(frame)).unwrap();

        let buffer = terminal.backend().buffer();
        let row_text = |y: u16| {
            (0..140u16)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        };
        let tabs = row_text(2);
        assert!(tabs.contains("General (0/2)"));
        assert!(tabs.contains("Software Development (0/2)"));
        assert!(tabs.contains("Operations and DevOps (0/1)"));
        assert!(row_text(9).contains("── First"));
        assert!(row_text(10).contains("recommended"));

        let rendered = buffer
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(!rendered.contains("internal-rpm-name"));
        assert!(!rendered.contains("downloads.example.com"));
        assert!(!rendered.contains("secret-member"));
    }
}
