impl App {
    fn render(&self, frame: &mut Frame) {
        let area = frame.area();
        let outer = Layout::vertical([
            Constraint::Length(4),
            Constraint::Min(1),
            Constraint::Length(1),
        ])
        .split(area);

        self.render_top_bar(frame, outer[0]);
        self.render_package_screen(frame, outer[1]);
        if self.show_help {
            self.render_help_overlay(frame, outer[1]);
        }
        self.render_bottom_bar(frame, outer[2]);
    }

    fn render_top_bar(&self, frame: &mut Frame, area: Rect) {
        let block = Block::bordered()
            .title(" AGS Sandbox Tools ")
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
            Span::raw("    Selected: "),
            Span::styled(
                self.state.selected_tool_count().to_string(),
                Style::default().fg(Color::Green),
            ),
        ])];

        if !self.state.packages.is_empty() {
            lines.push(self.package_tabs_line());
        }
        frame.render_widget(Paragraph::new(lines).block(block), area);
    }

    fn package_tabs_line(&self) -> Line<'static> {
        let spans = self
            .state
            .packages
            .iter()
            .enumerate()
            .map(|(index, package)| {
                let text = format!(
                    " {} ({}/{}) ",
                    package.package,
                    package.selected_count(),
                    package.tools.len()
                );
                let style = if index == self.current_package {
                    Style::default().fg(Color::Black).bg(Color::Cyan).bold()
                } else if package.all_selected() {
                    Style::default().fg(Color::Green)
                } else {
                    Style::default().fg(Color::Yellow)
                };
                Span::styled(text, style)
            })
            .collect::<Vec<_>>();
        Line::from(spans)
    }

    fn render_package_screen(&self, frame: &mut Frame, area: Rect) {
        let Some(package) = self.current_package() else {
            frame.render_widget(
                Paragraph::new("No tool packages loaded.")
                    .style(Style::default().fg(Color::DarkGray)),
                area,
            );
            return;
        };

        let chunks = Layout::vertical([
            Constraint::Length(4),
            Constraint::Min(5),
            Constraint::Length(7),
        ])
        .split(area);
        self.render_package_header(frame, chunks[0], package);
        self.render_tool_table(frame, chunks[1], package);
        self.render_tool_details(frame, chunks[2]);
    }

    fn render_package_header(&self, frame: &mut Frame, area: Rect, package: &PackageState) {
        let selected = package.selected_count();
        let total = package.tools.len();
        let status = if selected == total && total > 0 {
            Span::styled("selected", Style::default().fg(Color::Green))
        } else if selected == 0 {
            Span::styled("deselected", Style::default().fg(Color::Yellow))
        } else {
            Span::styled("partial", Style::default().fg(Color::Yellow))
        };
        let text = vec![
            Line::from(vec![
                Span::raw(" Group: "),
                Span::styled(
                    package.package.clone(),
                    Style::default().fg(Color::Cyan).bold(),
                ),
                Span::raw("    State: "),
                status,
            ]),
            Line::from(format!(" Selected tool options: {selected}/{total}")),
        ];
        frame.render_widget(
            Paragraph::new(text).block(
                Block::bordered()
                    .title(" Package group ")
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(Color::DarkGray)),
            ),
            area,
        );
    }

    fn render_tool_table(&self, frame: &mut Frame, area: Rect, package: &PackageState) {
        let header = Row::new(vec!["", "Tool", "DNF packages", "Description"])
            .style(Style::default().fg(Color::DarkGray).bold());
        let rows = package.tools.iter().enumerate().map(|(index, tool)| {
            let row_style = if index == self.selected_tool {
                Style::default().fg(Color::Black).bg(Color::White)
            } else {
                Style::default()
            };
            let checkbox = if tool.selected { "[x]" } else { "[ ]" };
            let checkbox_style = if tool.selected {
                Style::default().fg(Color::Green)
            } else {
                Style::default().fg(Color::Yellow)
            };
            Row::new(vec![
                Cell::from(checkbox).style(checkbox_style),
                Cell::from(tool.definition.name.clone()),
                Cell::from(tool.definition.dnf_packages.join(", ")),
                Cell::from(tool.definition.description.clone()),
            ])
            .style(row_style)
        });
        frame.render_widget(
            Table::new(
                rows,
                [
                    Constraint::Length(4),
                    Constraint::Length(22),
                    Constraint::Length(34),
                    Constraint::Min(20),
                ],
            )
            .header(header)
            .block(
                Block::bordered()
                    .title(" Image tools ")
                    .border_type(BorderType::Rounded)
                    .border_style(Style::default().fg(Color::Cyan)),
            ),
            area,
        );
    }

    fn render_tool_details(&self, frame: &mut Frame, area: Rect) {
        let lines = if let Some(tool) = self.current_tool() {
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
                ]),
                Line::from(tool.definition.description.clone()),
                Line::from(vec![
                    Span::raw("DNF packages: "),
                    Span::styled(
                        tool.definition.dnf_packages.join(", "),
                        Style::default().fg(Color::Green),
                    ),
                ]),
                Line::from(Span::styled(
                    "Changes apply after `ags update-image` or the next automatic image build.",
                    Style::default().fg(Color::DarkGray),
                )),
            ]
        } else {
            vec![Line::from(Span::styled(
                "No tools in this package group.",
                Style::default().fg(Color::DarkGray),
            ))]
        };
        frame.render_widget(
            Paragraph::new(lines)
                .block(
                    Block::bordered()
                        .title(" Details ")
                        .border_type(BorderType::Rounded)
                        .border_style(Style::default().fg(Color::DarkGray)),
                )
                .wrap(Wrap { trim: true }),
            area,
        );
    }

    fn render_help_overlay(&self, frame: &mut Frame, area: Rect) {
        let popup = centered_rect(72, 70, area);
        frame.render_widget(Clear, popup);
        let text = vec![
            Line::from("Configure DNF packages baked into the sandbox image."),
            Line::from(""),
            Line::from("Left/Right or h/l  Change package group"),
            Line::from("Up/Down or j/k     Move through tools"),
            Line::from("Space              Toggle selected tool"),
            Line::from("p                  Toggle entire package group"),
            Line::from("s                  Save package selection and quit"),
            Line::from("q or Esc           Quit without saving"),
            Line::from("?                  Show/close this help"),
            Line::from(""),
            Line::from("Saving does not rebuild the image; run `ags update-image` afterwards."),
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
                "h/l group  j/k tool  Space toggle  p group  s save  q quit  ? help".to_owned(),
                Style::default().fg(Color::DarkGray),
            ),
        };
        frame.render_widget(Paragraph::new(text).style(style), area);
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
