use std::path::{Path, PathBuf};

use crossterm::event::{self, Event, KeyCode, KeyEvent};
use ratatui::prelude::*;
use ratatui::widgets::*;

use super::model::{
    GroupRow, SaveReport, ToolConfigError, ToolSelectionState, load_package_file,
    write_selected_tools,
};
use crate::config::LockedToolDownload;

#[derive(Clone, Copy)]
enum StatusKind {
    Info,
    Success,
    Error,
}

pub struct App {
    config_path: PathBuf,
    legacy_cleanup_path: Option<PathBuf>,
    packages_path: PathBuf,
    state: ToolSelectionState,
    running: bool,
    current_group: usize,
    selected_row: usize,
    table_state: TableState,
    show_help: bool,
    status_message: Option<(String, StatusKind)>,
    save_report: Option<SaveReport>,
}

impl App {
    pub fn new(
        config_path: &Path,
        legacy_cleanup_path: Option<&Path>,
        packages_path: &Path,
        configured_packages: Option<&[String]>,
        configured_downloads: Option<&[LockedToolDownload]>,
    ) -> Result<Self, ToolConfigError> {
        if !config_path.exists() {
            return Err(ToolConfigError::Config(format!(
                "config file does not exist: {} (run `ags config` first)",
                config_path.display()
            )));
        }

        let catalog = load_package_file(packages_path)?;
        let state = ToolSelectionState::from_catalog_with_config(
            catalog,
            configured_packages,
            configured_downloads,
        )?;
        let mut app = Self {
            config_path: config_path.to_owned(),
            legacy_cleanup_path: legacy_cleanup_path.map(Path::to_owned),
            packages_path: packages_path.to_owned(),
            state,
            running: true,
            current_group: 0,
            selected_row: 0,
            table_state: TableState::default(),
            show_help: false,
            status_message: Some((
                "Choose tools by profession; shared tools stay synchronized across tabs."
                    .to_owned(),
                StatusKind::Info,
            )),
            save_report: None,
        };
        app.normalize_selection();
        Ok(app)
    }

    pub fn run(&mut self) -> Result<Option<SaveReport>, Box<dyn std::error::Error>> {
        let mut terminal = ratatui::init();
        let result = self.event_loop(&mut terminal);
        ratatui::restore();
        result?;
        Ok(self.save_report.take())
    }

    fn event_loop(
        &mut self,
        terminal: &mut ratatui::DefaultTerminal,
    ) -> Result<(), Box<dyn std::error::Error>> {
        while self.running {
            self.normalize_selection();
            terminal.draw(|frame| self.render(frame))?;

            if let Event::Key(key) = event::read()? {
                self.handle_key(key);
            }
        }
        Ok(())
    }

    fn normalize_selection(&mut self) {
        if self.state.groups.is_empty() {
            self.current_group = 0;
            self.selected_row = 0;
            self.table_state.select(None);
            return;
        }
        self.current_group = self.current_group.min(self.state.groups.len() - 1);
        let rows = self.state.group_rows(self.current_group);
        if rows.is_empty() {
            self.selected_row = 0;
            self.table_state.select(None);
            return;
        }
        self.selected_row = self.selected_row.min(rows.len() - 1);
        if !matches!(rows[self.selected_row], GroupRow::Tool(_)) {
            self.selected_row = rows
                .iter()
                .position(|row| matches!(row, GroupRow::Tool(_)))
                .unwrap_or(0);
        }
        self.table_state.select(Some(self.selected_row));
    }

    fn current_tool_index(&self) -> Option<usize> {
        match self
            .state
            .group_rows(self.current_group)
            .get(self.selected_row)
        {
            Some(GroupRow::Tool(index)) => Some(*index),
            _ => None,
        }
    }

    fn handle_key(&mut self, key: KeyEvent) {
        if self.show_help {
            self.show_help = false;
            return;
        }

        match key.code {
            KeyCode::Char('?') => self.show_help = true,
            KeyCode::Char('q') | KeyCode::Esc => self.running = false,
            KeyCode::Char('s') => self.save_and_quit(),
            KeyCode::Char('d') | KeyCode::Char('D') => self.restore_defaults(),
            KeyCode::Char(' ') => self.toggle_current_tool(),
            KeyCode::Right | KeyCode::Char('l') => self.move_group(1),
            KeyCode::Left | KeyCode::Char('h') => self.move_group(-1),
            KeyCode::Down | KeyCode::Char('j') => self.move_tool(1),
            KeyCode::Up | KeyCode::Char('k') => self.move_tool(-1),
            _ => {}
        }
    }

    fn move_group(&mut self, delta: isize) {
        let count = self.state.groups.len();
        if count == 0 {
            return;
        }
        let next = (self.current_group as isize + delta).clamp(0, (count - 1) as isize) as usize;
        if next != self.current_group {
            self.current_group = next;
            self.selected_row = 0;
            self.table_state = TableState::default();
            self.status_message = None;
            self.normalize_selection();
        }
    }

    fn move_tool(&mut self, delta: isize) {
        let rows = self.state.group_rows(self.current_group);
        if rows.is_empty() {
            return;
        }
        let mut next = self.selected_row as isize;
        loop {
            next += delta;
            if next < 0 || next >= rows.len() as isize {
                return;
            }
            if matches!(rows[next as usize], GroupRow::Tool(_)) {
                self.selected_row = next as usize;
                self.table_state.select(Some(self.selected_row));
                self.status_message = None;
                return;
            }
        }
    }

    fn toggle_current_tool(&mut self) {
        let Some(tool_index) = self.current_tool_index() else {
            return;
        };
        let tool = &mut self.state.tools[tool_index];
        tool.selected = !tool.selected;
        tool.touched = true;
        let state = if tool.selected {
            "selected"
        } else {
            "deselected"
        };
        self.status_message = Some((
            format!("{} {state} in every profession view.", tool.definition.name),
            StatusKind::Info,
        ));
    }

    fn restore_defaults(&mut self) {
        self.state.reset_to_defaults();
        self.status_message = Some((
            "Restored the catalog's recommended tool selection.".to_owned(),
            StatusKind::Info,
        ));
    }

    fn save_and_quit(&mut self) {
        match write_selected_tools(
            &self.config_path,
            self.legacy_cleanup_path.as_deref(),
            &self.state,
        ) {
            Ok(report) => {
                self.status_message = Some((
                    format!("Saved {} selected tools.", report.selected_tools),
                    StatusKind::Success,
                ));
                self.save_report = Some(report);
                self.running = false;
            }
            Err(error) => {
                self.status_message = Some((format!("Save failed: {error}"), StatusKind::Error));
            }
        }
    }
}

include!("ui_render.rs");

#[cfg(test)]
include!("ui_tests.rs");
