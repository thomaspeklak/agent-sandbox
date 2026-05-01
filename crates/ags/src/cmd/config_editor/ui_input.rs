impl App {
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
            KeyCode::BackTab if self.focus == Focus::MainPanel => {
                // Shift-Tab always goes to sidebar
                self.focus = Focus::Sidebar;
                self.selected_field = 0;
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
            KeyCode::Char(' ') if self.focus == Focus::MainPanel => {
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
            KeyCode::Char('a') if self.focus == Focus::MainPanel => {
                self.start_add_entry();
            }
            KeyCode::Char('d') if self.focus == Focus::MainPanel => {
                self.start_delete_entry();
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

        if let Some(request) = key_to_input_request(key.code, true)
            && let EditMode::EditingField { input, .. } = &mut self.edit_mode
        {
            input.handle(request);
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
                    && *active_field + 1 < fields.len()
                {
                    *active_field += 1;
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
        if kind_is_text
            && let Some(request) = key_to_input_request(key.code, false)
            && let EditMode::AddingEntry {
                fields,
                active_field,
                ..
            } = &mut self.edit_mode
        {
            fields[*active_field].input.handle(request);
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
}
