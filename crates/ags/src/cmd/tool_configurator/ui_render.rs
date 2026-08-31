impl App {
    fn render(&mut self, frame: &mut Frame) {
        let area = frame.area();
        let outer = Layout::vertical([
            Constraint::Length(4),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(area);

        self.render_top_bar(frame, outer[0]);
        self.render_group_screen(frame, outer[1]);
        if self.show_agents {
            self.render_agent_overlay(frame, outer[1]);
        }
        if self.show_help {
            self.render_help_overlay(frame, outer[1]);
        }
        self.render_bottom_bar(frame, outer[2]);
    }

    fn render_top_bar(&self, frame: &mut Frame, area: Rect) {
        let block = Block::bordered()
            .title(" Choose Tools for Your AGS Sandbox ")
            .title_alignment(Alignment::Center)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Color::Cyan));

        let mut lines = vec![Line::from(vec![
            Span::raw(" Config: "),
            Span::styled(
                self.config_path.display().to_string(),
                Style::default().fg(Color::Yellow),
            ),
            Span::raw("    Catalog: "),
            Span::styled(
                self.packages_path.display().to_string(),
                Style::default().fg(Color::Yellow),
            ),
            Span::raw("    Tools: "),
            Span::styled(
                self.state.selected_tool_count().to_string(),
                Style::default().fg(Color::Green),
            ),
            Span::raw("    Agents: "),
            Span::styled(
                format!(
                    "{}/{}",
                    self.state.selected_agent_count(),
                    self.state.agents.len()
                ),
                Style::default().fg(Color::Green),
            ),
            Span::styled("    [a] Agent CLIs", Style::default().fg(Color::Cyan)),
        ])];

        if !self.state.groups.is_empty() {
            lines.push(self.group_tabs_line());
        }
        frame.render_widget(Paragraph::new(lines).block(block), area);
    }

    fn group_tabs_line(&self) -> Line<'static> {
        let spans = self
            .state
            .groups
            .iter()
            .enumerate()
            .map(|(index, group)| {
                let selected = self.state.group_selected_count(index);
                let total = self.state.group_tool_count(index);
                let text = format!(" {} ({selected}/{total}) ", group.name);
                let style = if index == self.current_group {
                    Style::default().fg(Color::Black).bg(Color::Cyan).bold()
                } else if selected == total {
                    Style::default().fg(Color::Green)
                } else {
                    Style::default().fg(Color::Yellow)
                };
                Span::styled(text, style)
            })
            .collect::<Vec<_>>();
        Line::from(spans)
    }

    fn render_group_screen(&mut self, frame: &mut Frame, area: Rect) {
        if self.state.groups.get(self.current_group).is_none() {
            frame.render_widget(
                Paragraph::new("No profession groups loaded.")
                    .style(Style::default().fg(Color::DarkGray)),
                area,
            );
            return;
        }

        let chunks = Layout::vertical([
            Constraint::Length(3),
            Constraint::Min(5),
            Constraint::Length(8),
        ])
        .split(area);
        self.render_group_header(frame, chunks[0]);
        self.render_tool_table(frame, chunks[1]);
        self.render_tool_details(frame, chunks[2]);
    }

    fn render_group_header(&self, frame: &mut Frame, area: Rect) {
        let group = &self.state.groups[self.current_group];
        let selected = self.state.group_selected_count(self.current_group);
        let total = self.state.group_tool_count(self.current_group);
        let text = Line::from(vec![
            Span::raw(" Profession: "),
            Span::styled(group.name.clone(), Style::default().fg(Color::Cyan).bold()),
            Span::raw("    Selected tools visible here: "),
            Span::styled(format!("{selected}/{total}"), Style::default().fg(Color::Green)),
        ]);
        frame.render_widget(
            Paragraph::new(text).block(
                Block::bordered()
                    .title(" Profession view ")
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(Color::DarkGray)),
            ),
            area,
        );
    }

    fn render_tool_table(&mut self, frame: &mut Frame, area: Rect) {
        let header = Row::new(vec!["", "Tool", "Default", "Purpose"])
            .style(Style::default().fg(Color::DarkGray).bold());
        let rows = self
            .state
            .group_rows(self.current_group)
            .into_iter()
            .map(|row| match row {
                GroupRow::Divider(name) => Row::new(vec![
                    Cell::from(""),
                    Cell::from(format!("── {name} ")).style(
                        Style::default().fg(Color::Cyan).bold(),
                    ),
                    Cell::from(""),
                    Cell::from(""),
                ]),
                GroupRow::Tool(index) => {
                    let tool = &self.state.tools[index];
                    let checkbox = if tool.selected { "[x]" } else { "[ ]" };
                    let checkbox_style = if tool.selected {
                        Style::default().fg(Color::Green)
                    } else {
                        Style::default().fg(Color::Yellow)
                    };
                    Row::new(vec![
                        Cell::from(checkbox).style(checkbox_style),
                        Cell::from(tool.definition.name.clone()),
                        Cell::from(if tool.definition.default {
                            "recommended"
                        } else {
                            ""
                        }),
                        Cell::from(tool.definition.description.clone()),
                    ])
                }
            })
            .collect::<Vec<_>>();
        let table = Table::new(
            rows,
            [
                Constraint::Length(4),
                Constraint::Length(24),
                Constraint::Length(13),
                Constraint::Min(24),
            ],
        )
        .header(header)
        .row_highlight_style(Style::default().fg(Color::Black).bg(Color::White))
        .block(
            Block::bordered()
                .title(" Tools by area ")
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::Cyan)),
        );
        frame.render_stateful_widget(table, area, &mut self.table_state);
    }

    fn render_tool_details(&self, frame: &mut Frame, area: Rect) {
        let lines = if let Some(tool_index) = self.current_tool_index() {
            let tool = &self.state.tools[tool_index];
            let default_label = if tool.definition.default {
                "recommended by default"
            } else {
                "optional"
            };
            vec![
                Line::from(vec![
                    Span::styled(
                        tool.definition.name.clone(),
                        Style::default().fg(Color::Cyan).bold(),
                    ),
                    Span::raw(if tool.selected {
                        " selected"
                    } else {
                        " deselected"
                    }),
                    Span::styled(
                        format!("    {default_label}"),
                        Style::default().fg(Color::DarkGray),
                    ),
                ]),
                Line::from(tool.definition.description.clone()),
                Line::from(vec![
                    Span::raw("Appears in: "),
                    Span::styled(
                        self.state.placements_for_tool(tool_index).join("; "),
                        Style::default().fg(Color::Green),
                    ),
                ]),
                Line::from(Span::styled(
                    "A shared tool has one selection across every profession view.",
                    Style::default().fg(Color::DarkGray),
                )),
                Line::from(Span::styled(
                    "Changes apply after `ags update-image` or the next automatic image build.",
                    Style::default().fg(Color::DarkGray),
                )),
            ]
        } else {
            vec![Line::from(Span::styled(
                "No tools in this profession view.",
                Style::default().fg(Color::DarkGray),
            ))]
        };
        frame.render_widget(
            Paragraph::new(lines)
                .block(
                    Block::bordered()
                        .title(" Tool details ")
                        .border_type(BorderType::Rounded)
                        .border_style(Style::default().fg(Color::DarkGray)),
                )
                .wrap(Wrap { trim: true }),
            area,
        );
    }

    fn render_help_overlay(&self, frame: &mut Frame, area: Rect) {
        let popup = centered_rect(74, 72, area);
        frame.render_widget(Clear, popup);
        let text = vec![
            Line::from("Choose image tools and persistent agent CLI runtimes for AGS."),
            Line::from(""),
            Line::from("Left/Right or h/l  Change profession view"),
            Line::from("Up/Down or j/k     Move through tools and areas"),
            Line::from("Space              Toggle the selected tool everywhere"),
            Line::from("a                  Open/close the Agent CLIs panel"),
            Line::from("d                  Restore catalog defaults"),
            Line::from("s                  Save tool selection and quit"),
            Line::from("q or Esc           Quit without saving"),
            Line::from("?                  Show/close this help"),
            Line::from(""),
            Line::from("Area dividers are informational and are skipped by navigation."),
            Line::from(
                "Saving does not apply changes; run `ags update-image` and `ags update-agents`.",
            ),
        ];
        frame.render_widget(
            Paragraph::new(text)
                .block(
                    Block::bordered()
                        .title(" Help ")
                        .title_alignment(Alignment::Center)
                        .border_type(BorderType::Rounded)
                        .border_style(Style::default().fg(Color::Cyan)),
                )
                .wrap(Wrap { trim: true }),
            popup,
        );
    }

    fn render_bottom_bar(&self, frame: &mut Frame, area: Rect) {
        let (text, style) = match &self.status_message {
            Some((message, kind)) => (message.clone(), status_style(*kind)),
            None => (
                if self.show_agents {
                    "j/k agent  Space toggle  a/Esc close  d defaults  s save  q quit  ? help"
                        .to_owned()
                } else {
                    "h/l profession  j/k tool  Space toggle  a agents  d defaults  s save  q quit  ? help"
                        .to_owned()
                },
                Style::default().fg(Color::DarkGray),
            ),
        };
        frame.render_widget(Paragraph::new(text).style(style), area);
    }

    fn render_agent_overlay(&self, frame: &mut Frame, area: Rect) {
        let popup = centered_rect(82, 78, area);
        frame.render_widget(Clear, popup);
        let rows = self
            .state
            .agents
            .iter()
            .enumerate()
            .map(|(index, agent)| {
                let checkbox = if agent.selected { "[x]" } else { "[ ]" };
                let style = if index == self.selected_agent {
                    Style::default().fg(Color::Black).bg(Color::White)
                } else if agent.selected {
                    Style::default().fg(Color::Green)
                } else {
                    Style::default().fg(Color::Yellow)
                };
                Row::new(vec![
                    checkbox.to_owned(),
                    agent.definition.name.clone(),
                    agent.definition.description.clone(),
                ])
                .style(style)
            })
            .collect::<Vec<_>>();
        let table = Table::new(
            rows,
            [
                Constraint::Length(4),
                Constraint::Length(16),
                Constraint::Min(28),
            ],
        )
        .header(Row::new(vec!["", "Agent CLI", "Purpose"]).style(Style::default().bold()))
        .block(
            Block::bordered()
                .title(" Agent CLIs (persistent runtime volumes, not the base image) ")
                .title_alignment(Alignment::Center)
                .border_type(BorderType::Rounded)
                .border_style(Style::default().fg(Color::Cyan)),
        );
        frame.render_widget(table, popup);
    }
}

fn status_style(kind: StatusKind) -> Style {
    match kind {
        StatusKind::Info => Style::default().fg(Color::Cyan),
        StatusKind::Success => Style::default().fg(Color::Green),
        StatusKind::Error => Style::default().fg(Color::Red),
    }
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::vertical([
        Constraint::Percentage((100 - percent_y) / 2),
        Constraint::Percentage(percent_y),
        Constraint::Percentage((100 - percent_y) / 2),
    ])
    .split(area);
    let horizontal = Layout::horizontal([
        Constraint::Percentage((100 - percent_x) / 2),
        Constraint::Percentage(percent_x),
        Constraint::Percentage((100 - percent_x) / 2),
    ])
    .split(vertical[1]);
    horizontal[1]
}
