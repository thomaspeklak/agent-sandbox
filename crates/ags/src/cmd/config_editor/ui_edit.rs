impl App {
    // -------------------------------------------------------------------
    // Scalar field editing
    // -------------------------------------------------------------------

    /// Resolve the currently selected scalar field for editing. Returns the
    /// section index, field key, and schema, or sets a status message and returns `None`.
    fn resolve_editable_scalar(&mut self) -> Option<(usize, String, &'static ScalarFieldSchema)> {
        if self.reject_if_merged() {
            return None;
        }
        let info = &SECTIONS[self.selected_section];
        if info.is_array {
            return None;
        }
        let content = self.filtered_section_content(self.selected_section);
        let fields = match &*content {
            SectionContent::Scalar(f, _) => f,
            _ => return None,
        };
        if self.selected_field >= fields.len() {
            return None;
        }
        let field = &fields[self.selected_field];
        if !field.editable {
            self.status_message = Some((
                "Unknown fields are preserved but not editable in the structured editor.".into(),
                StatusKind::Warning,
            ));
            return None;
        }
        let field_key = field.key.clone();
        let schema = scalar_field(info.toml_key, &field_key)?;
        Some((self.selected_section, field_key, schema))
    }

    fn start_edit_field(&mut self) {
        let Some((section_idx, field_key, schema)) = self.resolve_editable_scalar() else {
            return;
        };
        let info = &SECTIONS[section_idx];

        let raw_value = {
            let doc = self.state.active_doc();
            doc.get(info.toml_key)
                .and_then(|t| t.get(&field_key))
                .map(|item| item_to_editor_text(item, schema))
                .unwrap_or_else(|| schema.default_input.to_string())
        };

        let mut input = Input::new(raw_value);
        input.handle(InputRequest::GoToEnd);

        self.edit_mode = EditMode::EditingField {
            input,
            section_key: info.toml_key.to_string(),
            field_key,
            kind: schema.kind,
        };
    }

    fn confirm_edit_field(&mut self) {
        let (new_text, section_key, field_key, kind) = match &self.edit_mode {
            EditMode::EditingField {
                input,
                section_key,
                field_key,
                kind,
            } => (
                input.value().to_string(),
                section_key.clone(),
                field_key.clone(),
                *kind,
            ),
            _ => return,
        };

        if let Err(message) = apply_scalar_value(
            self.state.active_doc_mut(),
            &section_key,
            &field_key,
            kind,
            &new_text,
        ) {
            self.status_message = Some((message, StatusKind::Error));
            return;
        }

        self.state.modified = true;
        self.invalidate_cache();
        self.edit_mode = EditMode::None;
    }

    fn toggle_scalar_field(&mut self) {
        let Some((section_idx, field_key, schema)) = self.resolve_editable_scalar() else {
            return;
        };
        let info = &SECTIONS[section_idx];

        let action = next_toggle_action(self.state.active_doc(), info.toml_key, &field_key, schema);

        if let Some(action) = action {
            let doc = self.state.active_doc_mut();
            ensure_table(doc, info.toml_key);
            match action {
                ToggleAction::Bool(b) => {
                    doc[info.toml_key][&field_key] = toml_edit::value(b);
                }
                ToggleAction::Str(s) => {
                    doc[info.toml_key][&field_key] = toml_edit::value(s);
                }
            }
            self.state.modified = true;
            self.invalidate_cache();
        }
    }

    // -------------------------------------------------------------------
    // Add / delete array entries
    // -------------------------------------------------------------------

    /// Returns the current section index if it's an editable (non-agents) array section,
    /// or `None` (with status message) if edits should be blocked.
    fn require_editable_array(&mut self) -> Option<usize> {
        if self.reject_if_merged() {
            return None;
        }
        let info = &SECTIONS[self.selected_section];
        if !info.is_array || self.is_agents_section() {
            return None;
        }
        Some(self.selected_section)
    }

    fn start_add_entry(&mut self) {
        let Some(section_idx) = self.require_editable_array() else {
            return;
        };
        let info = &SECTIONS[section_idx];

        let fields = build_entry_form_fields(info.toml_key, None);
        if fields.is_empty() {
            return;
        }

        let max_label = fields.iter().map(|f| f.label.len()).max().unwrap_or(0);
        self.edit_mode = EditMode::AddingEntry {
            fields,
            active_field: 0,
            section_key: info.toml_key.to_string(),
            max_label,
            edit_index: None,
        };
    }

    fn start_edit_entry(&mut self) {
        let Some(section_idx) = self.require_editable_array() else {
            return;
        };
        let info = &SECTIONS[section_idx];

        let content = self.filtered_section_content(self.selected_section);
        let entries = match &*content {
            SectionContent::Array(entries) => entries,
            _ => return,
        };
        let Some(entry) = entries.get(self.selected_field) else {
            return;
        };

        let fields = {
            let doc = self.state.active_doc();
            let table = doc
                .get(info.toml_key)
                .and_then(|item| item.as_array_of_tables())
                .and_then(|entries| entries.get(entry.raw_index));
            build_entry_form_fields(info.toml_key, table)
        };
        if fields.is_empty() {
            return;
        }

        let max_label = fields.iter().map(|f| f.label.len()).max().unwrap_or(0);
        self.edit_mode = EditMode::AddingEntry {
            fields,
            active_field: 0,
            section_key: info.toml_key.to_string(),
            max_label,
            edit_index: Some(entry.raw_index),
        };
    }

    fn confirm_add_entry(&mut self) {
        let (field_values, section_key, edit_index) = match &self.edit_mode {
            EditMode::AddingEntry {
                fields,
                section_key,
                edit_index,
                ..
            } => {
                for field in fields {
                    if field.required && field.input.value().is_empty() {
                        self.status_message =
                            Some((format!("{} is required", field.key), StatusKind::Error));
                        return;
                    }
                }
                let values = fields
                    .iter()
                    .map(|field| (field.key, field.kind, field.input.value().to_string()))
                    .collect::<Vec<_>>();
                (values, section_key.clone(), *edit_index)
            }
            _ => return,
        };

        let doc = self.state.active_doc_mut();
        if doc.get(&section_key).is_none() || doc[&section_key].as_array_of_tables().is_none() {
            doc[&section_key] = toml_edit::Item::ArrayOfTables(toml_edit::ArrayOfTables::new());
        }

        let Some(aot) = doc[&section_key].as_array_of_tables_mut() else {
            return;
        };

        if let Some(index) = edit_index {
            let Some(entry) = aot.get_mut(index) else {
                self.status_message = Some(("Entry no longer exists.".into(), StatusKind::Error));
                return;
            };
            if let Err(error) = apply_entry_form(section_key.as_str(), entry, &field_values) {
                self.status_message = Some((error, StatusKind::Error));
                return;
            }
        } else {
            let mut entry = toml_edit::Table::new();
            if let Err(error) = apply_entry_form(section_key.as_str(), &mut entry, &field_values) {
                self.status_message = Some((error, StatusKind::Error));
                return;
            }
            aot.push(entry);
        }

        self.state.modified = true;
        self.invalidate_cache();
        self.edit_mode = EditMode::None;
        self.status_message = Some((
            if edit_index.is_some() {
                "Entry updated.".into()
            } else {
                "Entry added.".into()
            },
            StatusKind::Success,
        ));
    }

    fn start_delete_entry(&mut self) {
        let Some(section_idx) = self.require_editable_array() else {
            return;
        };
        let info = &SECTIONS[section_idx];

        let content = self.filtered_section_content(self.selected_section);
        let entries = match &*content {
            SectionContent::Array(entries) => entries,
            _ => return,
        };

        if entries.is_empty() || self.selected_field >= entries.len() {
            return;
        }

        self.edit_mode = EditMode::ConfirmDelete {
            index: entries[self.selected_field].raw_index,
            section_key: info.toml_key.to_string(),
        };
        self.status_message = Some((
            format!("Delete entry {}? [y/N]", self.selected_field + 1),
            StatusKind::Warning,
        ));
    }

    fn confirm_delete(&mut self) {
        let (index, section_key) = match &self.edit_mode {
            EditMode::ConfirmDelete { index, section_key } => (*index, section_key.clone()),
            _ => return,
        };

        let (removed, new_len) = {
            let doc = self.state.active_doc_mut();
            if let Some(aot) = doc[&section_key].as_array_of_tables_mut() {
                if index < aot.len() {
                    aot.remove(index);
                    (true, aot.len())
                } else {
                    (false, aot.len())
                }
            } else {
                (false, 0)
            }
        };

        if removed {
            self.state.modified = true;
            self.invalidate_cache();
            self.status_message = Some(("Entry deleted.".into(), StatusKind::Success));
            if self.selected_field >= new_len && new_len > 0 {
                self.selected_field = new_len - 1;
            }
        }
        self.edit_mode = EditMode::None;
    }

    fn jump_to_origin(&mut self) {
        if self.state.view_mode != ViewMode::Merged || self.focus != Focus::MainPanel {
            return;
        }

        let info = self.current_section_info();
        let content = self.filtered_section_content(self.selected_section);

        let (target, selected_field, message) = if info.is_array {
            let SectionContent::Array(entries) = &*content else {
                return;
            };
            let Some(entry) = entries.get(self.selected_field) else {
                return;
            };
            let Some(source) = entry.source else {
                return;
            };
            let target = match source {
                ValueSource::Global => EditTarget::Global,
                ValueSource::Local | ValueSource::LocalOverridesGlobal => EditTarget::Local,
            };
            let selected_field = match source {
                ValueSource::Global => entry.raw_index,
                ValueSource::Local | ValueSource::LocalOverridesGlobal => entry
                    .raw_index
                    .saturating_sub(global_array_len(&self.state.global_doc, info.toml_key)),
            };
            (
                target,
                selected_field,
                format!("Jumped to {} raw value.", edit_target_label(target)),
            )
        } else {
            let SectionContent::Scalar(fields, _) = &*content else {
                return;
            };
            let Some(field) = fields.get(self.selected_field) else {
                return;
            };
            let Some(source) = field.source else {
                return;
            };
            let target = match source {
                ValueSource::Global => EditTarget::Global,
                ValueSource::Local | ValueSource::LocalOverridesGlobal => EditTarget::Local,
            };
            let selected_field =
                scalar_field_index_for_target(&self.state, target, info.toml_key, &field.key)
                    .unwrap_or(self.selected_field);
            (
                target,
                selected_field,
                format!("Jumped to {} raw value.", edit_target_label(target)),
            )
        };

        self.state.edit_target = target;
        self.state.view_mode = ViewMode::Raw;
        self.selected_field = selected_field;
        self.invalidate_cache();
        self.status_message = Some((message, StatusKind::Info));
    }
}
