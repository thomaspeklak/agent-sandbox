mod tests {
    use super::*;
    use crate::cmd::tool_configurator::model::{
        AgentDefinition, ToolCatalog, ToolDefinition, ToolGroupDefinition,
        ToolSubcategoryDefinition,
    };
    use crate::config::ArchiveMemberMatch;
    use crate::config::{ToolArchiveFormat, ToolDownloadArtifact, ToolDownloadSource};
    use crossterm::event::{KeyEventKind, KeyModifiers};
    use std::collections::BTreeMap;
    use ratatui::backend::TestBackend;

    fn agent_definitions() -> Vec<AgentDefinition> {
        serde_json::from_str::<Vec<crate::config::LockedAgentProvider>>(
            crate::assets::DEFAULT_AGENT_PROVIDERS_LOCK,
        )
        .unwrap()
        .into_iter()
        .map(|entry| AgentDefinition {
            id: entry.agent,
            name: entry.agent.display_name().to_owned(),
            description: entry.agent.description().to_owned(),
            provider: entry.provider,
        })
        .collect()
    }

    fn definition(id: &str, default: bool) -> ToolDefinition {
        ToolDefinition {
            id: id.to_owned(),
            name: id.to_owned(),
            description: format!("Use {id} to complete work."),
            default,
            dnf_packages: vec![id.to_owned()],
            download: None,
            github_release: None,
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
            agents: agent_definitions(),
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
            show_agents: false,
            selected_agent: 0,
            show_help: false,
            status_message: None,
            save_report: None,
            minimum_release_age: 1_440,
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
        assert!(app.state.agents.iter().all(|agent| agent.selected));
        assert!(app.state.agents.iter().all(|agent| agent.touched));
    }

    #[test]
    fn agent_panel_toggles_agents_without_changing_profession_tabs() {
        let mut app = app();

        app.handle_key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::NONE));
        assert!(app.show_agents);
        app.handle_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));
        assert!(!app.state.agents[0].selected);
        assert!(app.state.agents[0].touched);
        assert_eq!(app.current_group, 0);

        app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char(' '), KeyModifiers::NONE));
        assert!(!app.state.agents[1].selected);
        assert_eq!(app.state.selected_agent_count(), 3);

        app.handle_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::NONE));
        assert_eq!(app.state.selected_agent_count(), Agent::INSTALLABLE.len());
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(!app.show_agents);
    }

    #[test]
    fn agent_panel_renders_catalog_metadata() {
        let mut app = app();
        app.state.agents[0].definition.name = "Catalog Pi".to_owned();
        app.state.agents[0].definition.description = "Catalog-defined purpose.".to_owned();
        app.show_agents = true;
        let backend = TestBackend::new(120, 30);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal.draw(|frame| app.render(frame)).unwrap();

        let rendered = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("Catalog Pi"));
        assert!(rendered.contains("Catalog-defined purpose."));
    }

    #[test]
    fn only_key_press_events_change_selection() {
        let mut app = app();
        let key = |kind| {
            Event::Key(KeyEvent::new_with_kind(
                KeyCode::Char(' '),
                KeyModifiers::NONE,
                kind,
            ))
        };

        app.handle_event(key(KeyEventKind::Press));
        assert!(app.state.tools[0].selected);

        app.handle_event(key(KeyEventKind::Release));
        app.handle_event(key(KeyEventKind::Repeat));
        assert!(app.state.tools[0].selected);
    }

    #[test]
    fn uppercase_letter_bindings_match_lowercase_actions() {
        let mut app = app();

        app.handle_key(KeyEvent::new(KeyCode::Char('L'), KeyModifiers::NONE));
        assert_eq!(app.current_group, 1);
        app.handle_key(KeyEvent::new(KeyCode::Char('H'), KeyModifiers::NONE));
        assert_eq!(app.current_group, 0);

        app.handle_key(KeyEvent::new(KeyCode::Char('J'), KeyModifiers::NONE));
        assert_eq!(app.selected_row, 3);
        app.handle_key(KeyEvent::new(KeyCode::Char('K'), KeyModifiers::NONE));
        assert_eq!(app.selected_row, 1);

        app.state.tools[0].selected = false;
        app.handle_key(KeyEvent::new(KeyCode::Char('D'), KeyModifiers::NONE));
        assert!(app.state.tools[0].selected);

        app.handle_key(KeyEvent::new(KeyCode::Char('Q'), KeyModifiers::NONE));
        assert!(!app.running);
    }

    #[test]
    fn uppercase_save_binding_writes_the_selection() {
        let dir = tempfile::tempdir().unwrap();
        let config = dir.path().join("config.toml");
        std::fs::write(&config, "[sandbox]\nextra_dnf_packages = []\n").unwrap();
        let mut app = app();
        app.config_path = config.clone();

        app.handle_key(KeyEvent::new(KeyCode::Char('S'), KeyModifiers::NONE));

        assert!(!app.running);
        assert!(app.save_report.is_some());
        assert!(std::fs::read_to_string(config)
            .unwrap()
            .contains("tool_download_lock"));
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
            agents: agent_definitions(),
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
            show_agents: false,
            selected_agent: 0,
            show_help: false,
            status_message: None,
            save_report: None,
            minimum_release_age: 1_440,
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
            member_match: ArchiveMemberMatch::Exact,
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

        let rendered = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();
        assert!(rendered.contains("General (0/2)"));
        assert!(rendered.contains("Software Development (0/2)"));
        assert!(rendered.contains("Operations and DevOps (0/1)"));
        assert!(rendered.contains("── First"));
        assert!(rendered.contains("recommended"));
        assert!(!rendered.contains("internal-rpm-name"));
        assert!(!rendered.contains("downloads.example.com"));
        assert!(!rendered.contains("secret-member"));
    }
}
