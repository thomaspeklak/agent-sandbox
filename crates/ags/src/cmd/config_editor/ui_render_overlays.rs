impl App {
    fn render_bottom_bar(&self, frame: &mut Frame, area: Rect) {
        if let Some((ref msg, ref kind)) = self.status_message {
            let fg = match kind {
                StatusKind::Info => Color::White,
                StatusKind::Success => Color::Green,
                StatusKind::Warning => Color::Yellow,
                StatusKind::Error => Color::Red,
            };
            let bar = Paragraph::new(format!(" {msg}"))
                .style(Style::default().fg(fg).bg(Color::DarkGray));
            frame.render_widget(bar, area);
            return;
        }

        if let EditMode::Search { input } = &self.edit_mode {
            let bar = Paragraph::new(format!(
                " / Search: {}  Enter: Apply  Esc: Clear",
                input.value()
            ))
            .style(Style::default().fg(Color::White).bg(Color::DarkGray));
            frame.render_widget(bar, area);
            return;
        }

        let mut shortcuts = match &self.edit_mode {
            EditMode::EditingField { .. } => "Enter: Confirm  Esc: Cancel".to_string(),
            EditMode::AddingEntry { .. } => {
                "Enter: Confirm  Esc: Cancel  Tab: Next  Shift-Tab: Prev".to_string()
            }
            EditMode::ConfirmDelete { .. } => "y: Delete  Any key: Cancel".to_string(),
            EditMode::Search { .. } => unreachable!(),
            EditMode::None => match self.focus {
                Focus::Sidebar => {
                    "^S Save  ^Z Undo  ^T Target  ^V View  Tab Panel  ? Help  q Quit".to_string()
                }
                Focus::MainPanel => {
                    let info = &SECTIONS[self.selected_section];
                    if self.state.view_mode == ViewMode::Merged {
                        "Enter Jump to raw  ^V Raw view  Tab Sidebar  Esc Back  ? Help  q Quit"
                            .to_string()
                    } else if self.is_agents_section() {
                        "Space Toggle  ^S Save  ^Z Undo  Tab Sidebar  Esc Back  ? Help  q Quit"
                            .to_string()
                    } else if info.is_array {
                        "Enter Edit  a Add  d Delete  ^S Save  ^Z Undo  Tab Sidebar  Esc Back  ? Help  q Quit"
                            .to_string()
                    } else {
                        "Enter Edit  Space Toggle  ^S Save  ^Z Undo  Tab Sidebar  Esc Back  ? Help  q Quit"
                            .to_string()
                    }
                }
            },
        };

        if let Some(suggestion) = &self.current_suggestion {
            shortcuts.push_str(&format!("  Ctrl-G Suggest: {}", suggestion));
        }

        let bar = Paragraph::new(format!(" {shortcuts}"))
            .style(Style::default().fg(Color::White).bg(Color::DarkGray));
        frame.render_widget(bar, area);
    }

    fn render_help_overlay(&self, frame: &mut Frame, area: Rect) {
        let help_text = vec![
            Line::from("Navigation").style(Style::default().bold().fg(Color::Cyan)),
            Line::from("  j / Down      Move down"),
            Line::from("  k / Up        Move up"),
            Line::from("  Tab           Move focus to main panel"),
            Line::from("  Shift-Tab     Move focus to sidebar"),
            Line::from("  Enter         Select / edit / jump to raw in merged view"),
            Line::from("  Escape        Back to sidebar / cancel edit"),
            Line::from(""),
            Line::from("Actions").style(Style::default().bold().fg(Color::Cyan)),
            Line::from("  a             Add entry (array sections)"),
            Line::from("  d             Delete entry (array sections)"),
            Line::from("  Space         Toggle (agents / booleans)"),
            Line::from("  Ctrl-G        Accept top discovery suggestion (text inputs)"),
            Line::from(""),
            Line::from("Global").style(Style::default().bold().fg(Color::Cyan)),
            Line::from("  Ctrl-S        Save current target"),
            Line::from("  Ctrl-Z        Undo (restore from backup)"),
            Line::from("  Ctrl-T        Switch target (Global/Local)"),
            Line::from("  Ctrl-V        Switch view (Raw/Merged)"),
            Line::from("  ?             Toggle this help"),
            Line::from("  q             Quit (prompts if unsaved)"),
        ];

        let block = Block::bordered()
            .title(" Help ")
            .title_alignment(Alignment::Center)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::Cyan));

        let help = Paragraph::new(help_text).block(block);
        frame.render_widget(help, area);
    }

    /// Render a centered dialog box and return its inner rect.
    fn render_centered_dialog(
        frame: &mut Frame,
        area: Rect,
        max_width: u16,
        title: &str,
        border_color: Color,
    ) -> Rect {
        let dialog_width = (area.width.saturating_sub(8)).min(max_width);
        let dialog_height = 7_u16;
        let x = area.x + (area.width.saturating_sub(dialog_width)) / 2;
        let y = area.y + (area.height.saturating_sub(dialog_height)) / 2;
        let dialog_area = Rect::new(x, y, dialog_width, dialog_height);

        frame.render_widget(Clear, dialog_area);

        let block = Block::bordered()
            .title(title)
            .title_alignment(Alignment::Center)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(border_color));

        let inner = block.inner(dialog_area);
        frame.render_widget(block, dialog_area);
        inner
    }

    fn render_validation_dialog(&self, frame: &mut Frame, area: Rect, error_msg: &str) {
        let inner = Self::render_centered_dialog(frame, area, 60, " Validation Error ", Color::Red);

        // Truncate error message to fit
        let max_msg_len = inner.width.saturating_sub(2) as usize;
        let truncated = if error_msg.len() > max_msg_len {
            format!("{}...", &error_msg[..max_msg_len.saturating_sub(3)])
        } else {
            error_msg.to_string()
        };

        let text = vec![
            Line::from(""),
            Line::from(Span::styled(
                format!("  {truncated}"),
                Style::default().fg(Color::Red),
            )),
            Line::from(""),
            Line::from(vec![
                Span::styled("[K]", Style::default().fg(Color::Yellow).bold()),
                Span::raw(" Keep  "),
                Span::styled("[R]", Style::default().fg(Color::Yellow).bold()),
                Span::raw(" Restore backup"),
            ]),
        ];

        frame.render_widget(Paragraph::new(text), inner);
    }

    fn render_quit_dialog(&self, frame: &mut Frame, area: Rect) {
        let inner =
            Self::render_centered_dialog(frame, area, 56, " Unsaved Changes ", Color::Yellow);

        let text = vec![
            Line::from(""),
            Line::from("  Save before quitting?"),
            Line::from(""),
            Line::from(vec![
                Span::raw("  "),
                Span::styled("[Y]", Style::default().fg(Color::Green).bold()),
                Span::raw(" Save & quit  "),
                Span::styled("[N]", Style::default().fg(Color::Red).bold()),
                Span::raw(" Discard & quit  "),
                Span::styled("[C]", Style::default().fg(Color::Yellow).bold()),
                Span::raw(" Cancel"),
            ]),
        ];

        frame.render_widget(Paragraph::new(text), inner);
    }

    fn render_create_local_dialog(&self, frame: &mut Frame, area: Rect) {
        let inner = Self::render_centered_dialog(
            frame,
            area,
            64,
            " Create repo-local config ",
            Color::Cyan,
        );

        let text = vec![
            Line::from(""),
            Line::from("  Create .ags/config.toml now for this repo?"),
            Line::from(""),
            Line::from(vec![
                Span::raw("  "),
                Span::styled("[C]", Style::default().fg(Color::Green).bold()),
                Span::raw(" Create now  "),
                Span::styled("[L]", Style::default().fg(Color::Yellow).bold()),
                Span::raw(" Later (create on save)  "),
                Span::styled("[Esc]", Style::default().fg(Color::Red).bold()),
                Span::raw(" Cancel"),
            ]),
        ];

        frame.render_widget(Paragraph::new(text), inner);
    }

}
