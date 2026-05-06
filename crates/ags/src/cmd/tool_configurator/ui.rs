use std::path::{Path, PathBuf};

use crossterm::event::{self, Event, KeyCode, KeyEvent};
use ratatui::prelude::*;
use ratatui::widgets::*;

use super::model::{
    PackageState, SaveReport, SecretInput, ToolConfigError, ToolResolver, ToolSelectionState,
    ToolState, load_package_file, write_selected_tools,
};

#[derive(Clone, Copy)]
enum StatusKind {
    Info,
    Success,
    Warning,
    Error,
}

pub struct App {
    config_path: PathBuf,
    packages_path: PathBuf,
    state: ToolSelectionState,
    running: bool,
    current_package: usize,
    selected_tool: usize,
    show_help: bool,
    status_message: Option<(String, StatusKind)>,
    save_report: Option<SaveReport>,
}

impl App {
    pub fn new(
        config_path: &Path,
        packages_path: &Path,
        resolver: &dyn ToolResolver,
    ) -> Result<Self, ToolConfigError> {
        if !config_path.exists() {
            return Err(ToolConfigError::Config(format!(
                "config file does not exist: {} (run `ags config` first)",
                config_path.display()
            )));
        }

        let packages = load_package_file(packages_path)?;
        let state = ToolSelectionState::from_packages(packages, resolver)?;

        Ok(Self {
            config_path: config_path.to_owned(),
            packages_path: packages_path.to_owned(),
            state,
            running: true,
            current_package: 0,
            selected_tool: 0,
            show_help: false,
            status_message: Some((
                "Available tools were preselected. Missing tools are disabled.".to_owned(),
                StatusKind::Info,
            )),
            save_report: None,
        })
    }

    pub fn run(&mut self) -> Result<Option<SaveReport>, Box<dyn std::error::Error>> {
        let mut terminal = ratatui::init();
        let result = self.event_loop(&mut terminal);
        ratatui::restore();
        result?;
        Ok(self.save_report)
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
        if self.state.packages.is_empty() {
            self.current_package = 0;
            self.selected_tool = 0;
            return;
        }

        if self.current_package >= self.state.packages.len() {
            self.current_package = self.state.packages.len() - 1;
        }

        let tool_count = self.current_package().map(|p| p.tools.len()).unwrap_or(0);
        if tool_count == 0 {
            self.selected_tool = 0;
        } else if self.selected_tool >= tool_count {
            self.selected_tool = tool_count - 1;
        }
    }

    fn current_package(&self) -> Option<&PackageState> {
        self.state.packages.get(self.current_package)
    }

    fn current_package_mut(&mut self) -> Option<&mut PackageState> {
        self.state.packages.get_mut(self.current_package)
    }

    fn current_tool(&self) -> Option<&ToolState> {
        self.current_package()
            .and_then(|package| package.tools.get(self.selected_tool))
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
            KeyCode::Char('p') | KeyCode::Char('P') => self.toggle_current_package(),
            KeyCode::Char(' ') => self.toggle_current_tool(),
            KeyCode::Right | KeyCode::Char('l') => self.move_package(1),
            KeyCode::Left | KeyCode::Char('h') => self.move_package(-1),
            KeyCode::Down | KeyCode::Char('j') => self.move_tool(1),
            KeyCode::Up | KeyCode::Char('k') => self.move_tool(-1),
            _ => {}
        }
    }

    fn move_package(&mut self, delta: isize) {
        let count = self.state.packages.len();
        if count == 0 {
            return;
        }
        let current = self.current_package as isize;
        let next = (current + delta).clamp(0, (count - 1) as isize) as usize;
        if next != self.current_package {
            self.current_package = next;
            self.selected_tool = 0;
        }
    }

    fn move_tool(&mut self, delta: isize) {
        let Some(package) = self.current_package() else {
            return;
        };
        if package.tools.is_empty() {
            return;
        }
        let current = self.selected_tool as isize;
        self.selected_tool =
            (current + delta).clamp(0, (package.tools.len() - 1) as isize) as usize;
    }

    fn toggle_current_package(&mut self) {
        let Some(package) = self.current_package_mut() else {
            return;
        };
        let available = package.available_count();
        if available == 0 {
            self.status_message = Some((
                format!("No tools in '{}' are available on PATH.", package.package),
                StatusKind::Warning,
            ));
            return;
        }

        let select = !package.all_available_selected();
        for tool in &mut package.tools {
            if tool.available() {
                tool.selected = select;
            }
        }

        let action = if select { "Selected" } else { "Deselected" };
        self.status_message = Some((
            format!("{action} all available tools in '{}'.", package.package),
            StatusKind::Info,
        ));
    }

    fn toggle_current_tool(&mut self) {
        let selected_tool = self.selected_tool;
        let Some(package) = self.current_package_mut() else {
            return;
        };
        let Some(tool) = package.tools.get_mut(selected_tool) else {
            return;
        };
        if !tool.available() {
            self.status_message = Some((
                format!(
                    "{} is not available on PATH and cannot be selected.",
                    tool.definition.name
                ),
                StatusKind::Warning,
            ));
            return;
        }

        tool.selected = !tool.selected;
        let state = if tool.selected {
            "selected"
        } else {
            "deselected"
        };
        self.status_message = Some((
            format!("{} {state}.", tool.definition.name),
            StatusKind::Info,
        ));
    }

    fn save_and_quit(&mut self) {
        match write_selected_tools(&self.config_path, &self.state) {
            Ok(report) => {
                self.save_report = Some(report);
                self.status_message = Some((
                    format!("Saved {} selected tools.", report.added_tools),
                    StatusKind::Success,
                ));
                self.running = false;
            }
            Err(error) => {
                self.status_message = Some((format!("Save failed: {error}"), StatusKind::Error));
            }
        }
    }
}

include!("ui_render.rs");
