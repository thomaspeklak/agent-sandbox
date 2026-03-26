impl App {
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

        if let Some(request) = key_to_input_request(key.code, true)
            && let EditMode::Search { input } = &mut self.edit_mode
        {
            input.handle(request);
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
                            EditTarget::Local => "Restored from backup. Layered validation passed.",
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
