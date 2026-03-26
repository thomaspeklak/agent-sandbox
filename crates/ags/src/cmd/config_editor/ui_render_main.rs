impl App {
    // -------------------------------------------------------------------
    // Rendering
    // -------------------------------------------------------------------

    fn render(&self, frame: &mut Frame) {
        let area = frame.area();

        // Layout: top bar (3) | middle (fill) | bottom bar (1)
        let outer = Layout::vertical([
            Constraint::Length(3),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(area);

        self.render_top_bar(frame, outer[0]);

        if self.show_help {
            self.render_help_overlay(frame, outer[1]);
        } else {
            self.render_middle(frame, outer[1]);
        }

        // Render dialog overlay on top of content if active
        match &self.dialog {
            DialogState::ValidationError(msg) => {
                self.render_validation_dialog(frame, outer[1], msg);
            }
            DialogState::QuitConfirm => {
                self.render_quit_dialog(frame, outer[1]);
            }
            DialogState::CreateLocalPrompt => {
                self.render_create_local_dialog(frame, outer[1]);
            }
            DialogState::None => {}
        }

        self.render_bottom_bar(frame, outer[2]);
    }

    fn render_top_bar(&self, frame: &mut Frame, area: Rect) {
        let target_label = match self.state.edit_target {
            EditTarget::Global => "Global (~/.config/ags/config.toml)",
            EditTarget::Local if self.state.local_exists => "Local (.ags/config.toml)",
            EditTarget::Local => "Local (.ags/config.toml, creates on save)",
        };
        let view_label = match self.state.view_mode {
            ViewMode::Raw => "Raw",
            ViewMode::Merged => "Merged",
        };
        let mut spans = vec![Span::raw(format!(
            " Target: {}    View: {}",
            target_label, view_label
        ))];
        if !self.repo_local_available {
            spans.push(Span::styled(
                "    Repo-local: disabled (not in a git repo)",
                Style::default().fg(Color::DarkGray),
            ));
        }
        if self.state.modified {
            spans.push(Span::styled(
                "  [modified]",
                Style::default().fg(Color::Yellow).bold(),
            ));
        }

        let block = Block::bordered()
            .title(" AGS Config Editor ")
            .title_alignment(Alignment::Center)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::Cyan));

        let text = Paragraph::new(Line::from(spans)).block(block);
        frame.render_widget(text, area);
    }

    fn render_middle(&self, frame: &mut Frame, area: Rect) {
        // Horizontal split: sidebar (22 cols) | main panel (rest)
        let chunks = Layout::horizontal([Constraint::Length(22), Constraint::Min(1)]).split(area);

        self.render_sidebar(frame, chunks[0]);
        self.render_main_panel(frame, chunks[1]);
    }

    fn render_sidebar(&self, frame: &mut Frame, area: Rect) {
        let items: Vec<ListItem> = self
            .filtered_section_indices()
            .into_iter()
            .map(|i| {
                let info = &SECTIONS[i];
                let count_suffix = if info.is_array {
                    if info.toml_key == "agent_mount" {
                        // Show count of enabled agents (from cache)
                        let enabled = self.agent_enabled_cache.iter().filter(|&&e| e).count();
                        format!(" ({}/{})", enabled, KNOWN_AGENTS.len())
                    } else {
                        match self.section_content(i) {
                            SectionContent::Array(entries) => {
                                format!(" ({})", entries.len())
                            }
                            _ => String::new(),
                        }
                    }
                } else {
                    String::new()
                };

                let label = format!("  {}{}", info.label, count_suffix);
                let style = if i == self.selected_section {
                    if self.focus == Focus::Sidebar {
                        Style::default().fg(Color::Black).bg(Color::Cyan).bold()
                    } else {
                        Style::default().fg(Color::Cyan).bold()
                    }
                } else {
                    Style::default()
                };
                ListItem::new(label).style(style)
            })
            .collect();

        let sidebar = List::new(items).block(
            Block::bordered()
                .title(" Sections ")
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(self.panel_border_color(Focus::Sidebar))),
        );
        frame.render_widget(sidebar, area);
    }

    fn render_main_panel(&self, frame: &mut Frame, area: Rect) {
        let info = &SECTIONS[self.selected_section];

        let block = Block::bordered()
            .title(format!(" {} ", info.label))
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(self.panel_border_color(Focus::MainPanel)));

        let inner = block.inner(area);
        frame.render_widget(block, area);

        // Check if we're in add mode for this section
        let in_add_mode = matches!(
            &self.edit_mode,
            EditMode::AddingEntry { section_key, .. } if section_key == info.toml_key
        );

        if in_add_mode {
            let form_height = match &self.edit_mode {
                EditMode::AddingEntry { fields, .. } => fields.len() as u16 + 5,
                _ => 0,
            };
            let chunks = Layout::vertical([Constraint::Min(1), Constraint::Length(form_height)])
                .split(inner);

            self.render_section_content(frame, chunks[0]);
            self.render_add_form(frame, chunks[1]);
        } else {
            self.render_section_content(frame, inner);
        }
    }

    fn render_section_content(&self, frame: &mut Frame, area: Rect) {
        if self.state.local_missing_on_disk()
            && self.state.view_mode == ViewMode::Raw
            && !self.state.has_local_layer()
        {
            let hint = Paragraph::new(
                "  Repo-local config does not exist yet.\n\n  Any edits here stay local and will create .ags/config.toml on save.\n\n  Press Ctrl-S to create it now, or Ctrl-T to switch back to Global.",
            )
            .style(Style::default().fg(Color::DarkGray));
            frame.render_widget(hint, area);
            return;
        }

        if self.is_agents_section() {
            self.render_agents_section(frame, area);
        } else {
            let content = self.filtered_section_content(self.selected_section);
            match &*content {
                SectionContent::Scalar(fields, max_key) => {
                    self.render_scalar_fields(frame, area, fields, *max_key)
                }
                SectionContent::Array(entries) => self.render_array_entries(frame, area, entries),
            }
        }
    }

    fn render_agents_section(&self, frame: &mut Frame, area: Rect) {
        let items: Vec<ListItem> = self
            .filtered_agent_indices()
            .into_iter()
            .enumerate()
            .map(|(visible_index, i)| {
                let agent = &KNOWN_AGENTS[i];
                let enabled = self.agent_enabled_cache[i];
                let checkbox = if enabled { "[x]" } else { "[ ]" };

                let mut mounts_desc = String::new();
                for (j, m) in agent.mounts.iter().enumerate() {
                    if j > 0 {
                        mounts_desc.push_str(", ");
                    }
                    mounts_desc.push_str(m.host);
                }
                let host_status = self.agent_host_status(i);

                let style = self.item_style(visible_index);

                let checkbox_style = if enabled {
                    Style::default().fg(Color::Green)
                } else {
                    Style::default().fg(Color::DarkGray)
                };

                ListItem::new(Line::from(vec![
                    Span::raw("  "),
                    Span::styled(checkbox, checkbox_style),
                    Span::raw(format!(" {:<10} {}", agent.name, mounts_desc)),
                    Span::styled(format!("  [{}]", host_status), host_status.style()),
                ]))
                .style(style)
            })
            .collect();

        let list = List::new(items);
        frame.render_widget(list, area);
    }

    fn render_scalar_fields(
        &self,
        frame: &mut Frame,
        area: Rect,
        fields: &[FieldDisplay],
        max_key: usize,
    ) {
        if fields.is_empty() {
            let empty = Paragraph::new("  (no fields)").style(Style::default().fg(Color::DarkGray));
            frame.render_widget(empty, area);
            return;
        }

        let rows: Vec<Row> = fields
            .iter()
            .enumerate()
            .map(|(i, field)| {
                let style = self.item_style(i);
                let key_style = if field.editable {
                    Style::default().fg(Color::Yellow)
                } else {
                    Style::default().fg(Color::DarkGray)
                };
                let value_style = if field.present {
                    Style::default()
                } else {
                    Style::default().fg(Color::DarkGray).italic()
                };

                let source_marker = source_suffix(field.source);

                Row::new(vec![
                    Cell::from(format!("  {:<width$}", field.key, width = max_key))
                        .style(key_style),
                    Cell::from(" = "),
                    Cell::from(field.value.clone()).style(value_style),
                    Cell::from(source_marker.0).style(source_marker.1),
                ])
                .style(style)
            })
            .collect();

        let widths = [
            Constraint::Length((max_key + 2) as u16),
            Constraint::Length(3),
            Constraint::Min(1),
            Constraint::Length(6),
        ];

        let table = Table::new(rows, widths).column_spacing(0);
        frame.render_widget(table, area);

        // Overlay input widget if editing a field in this section
        if let EditMode::EditingField {
            input,
            section_key,
            field_key,
            ..
        } = &self.edit_mode
        {
            let info = &SECTIONS[self.selected_section];
            if section_key == info.toml_key
                && let Some(i) = fields.iter().position(|f| f.key == *field_key)
            {
                let y = area.y + i as u16;
                let value_x = area.x + (max_key + 2) as u16 + 3;
                let value_width = area.width.saturating_sub((max_key + 2) as u16 + 3 + 6);
                let input_area = Rect::new(value_x, y, value_width, 1);

                let scroll = input.visual_scroll(value_width.saturating_sub(1) as usize);
                let display = Paragraph::new(input.value())
                    .scroll((0, scroll as u16))
                    .style(Style::default().fg(Color::White).bg(Color::DarkGray));
                frame.render_widget(display, input_area);

                let cursor_x = value_x + (input.visual_cursor().saturating_sub(scroll)) as u16;
                frame.set_cursor_position((cursor_x, y));
            }
        }
    }

    fn render_array_entries(&self, frame: &mut Frame, area: Rect, entries: &[ArrayEntry]) {
        if entries.is_empty() {
            let empty = Paragraph::new("  (no entries)\n\n  Press 'a' to add")
                .style(Style::default().fg(Color::DarkGray));
            frame.render_widget(empty, area);
            return;
        }

        let items: Vec<ListItem> = entries
            .iter()
            .enumerate()
            .map(|(i, entry)| {
                let style = self.item_style(i);

                let marker = source_suffix(entry.source);

                ListItem::new(Line::from(vec![
                    Span::raw(format!("  {}. {}", i + 1, entry.summary)),
                    if !marker.0.is_empty() {
                        Span::styled(format!("  {}", marker.0), marker.1)
                    } else {
                        Span::raw("")
                    },
                ]))
                .style(style)
            })
            .collect();

        let list = List::new(items);
        frame.render_widget(list, area);
    }

    fn render_add_form(&self, frame: &mut Frame, area: Rect) {
        let (fields, active_field, section_key, max_label, edit_index) = match &self.edit_mode {
            EditMode::AddingEntry {
                fields,
                active_field,
                section_key,
                max_label,
                edit_index,
            } => (
                fields,
                *active_field,
                section_key.as_str(),
                *max_label,
                *edit_index,
            ),
            _ => return,
        };

        let title = if edit_index.is_some() {
            format!(" Edit {} ", capitalize_first(section_key))
        } else {
            format!(" Add {} ", capitalize_first(section_key))
        };
        let block = Block::bordered()
            .title(title)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::Cyan));

        let inner = block.inner(area);
        frame.render_widget(block, area);

        for (i, field) in fields.iter().enumerate() {
            if i as u16 >= inner.height.saturating_sub(1) {
                break;
            }

            let y = inner.y + i as u16;
            let is_active = i == active_field;

            // Label
            let label_width = (max_label + 4) as u16;
            let label_area = Rect::new(inner.x, y, label_width.min(inner.width), 1);
            let label_style = if is_active {
                Style::default().fg(Color::Yellow).bold()
            } else {
                Style::default().fg(Color::DarkGray)
            };
            frame.render_widget(
                Paragraph::new(format!("  {:<width$}:", field.label, width = max_label))
                    .style(label_style),
                label_area,
            );

            // Value area
            let value_x = inner.x + label_width;
            let value_width = inner.width.saturating_sub(label_width + 1);
            if value_width == 0 {
                continue;
            }
            let value_area = Rect::new(value_x, y, value_width, 1);

            match &field.kind {
                FieldKind::Text => {
                    let scroll = field
                        .input
                        .visual_scroll(value_width.saturating_sub(1) as usize);
                    let style = if is_active {
                        Style::default().fg(Color::White).bg(Color::DarkGray)
                    } else {
                        Style::default().fg(Color::White)
                    };
                    let display = Paragraph::new(field.input.value())
                        .scroll((0, scroll as u16))
                        .style(style);
                    frame.render_widget(display, value_area);

                    if is_active {
                        let cursor_x =
                            value_x + (field.input.visual_cursor().saturating_sub(scroll)) as u16;
                        frame.set_cursor_position((cursor_x, y));
                    }
                }
                FieldKind::Toggle(options) => {
                    let current = field.input.value();
                    let mut spans = vec![Span::raw(" ")];
                    for (j, opt) in options.iter().enumerate() {
                        if j > 0 {
                            spans.push(Span::styled(" | ", Style::default().fg(Color::DarkGray)));
                        }
                        if *opt == current {
                            spans.push(Span::styled(
                                format!(" {} ", opt),
                                Style::default().fg(Color::Black).bg(Color::Cyan).bold(),
                            ));
                        } else {
                            spans.push(Span::styled(
                                format!(" {} ", opt),
                                Style::default().fg(Color::DarkGray),
                            ));
                        }
                    }
                    frame.render_widget(Paragraph::new(Line::from(spans)), value_area);
                }
                FieldKind::Checkbox => {
                    let checked = field.input.value() == "true";
                    let display = if checked { "[x]" } else { "[ ]" };
                    let style = if checked {
                        Style::default().fg(Color::Green)
                    } else {
                        Style::default().fg(Color::DarkGray)
                    };
                    frame.render_widget(
                        Paragraph::new(format!(" {}", display)).style(style),
                        value_area,
                    );
                }
            }
        }

        // Footer hint
        let footer_y = inner.y + inner.height.saturating_sub(1);
        if footer_y > inner.y + fields.len() as u16 {
            let footer =
                Paragraph::new("  Enter: Confirm  Esc: Cancel  Tab: Next  Shift-Tab: Prev")
                    .style(Style::default().fg(Color::DarkGray));
            frame.render_widget(footer, Rect::new(inner.x, footer_y, inner.width, 1));
        }
    }
}
